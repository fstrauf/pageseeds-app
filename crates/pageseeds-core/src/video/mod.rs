//! Video clip workflow — content intelligence for short-form vertical videos
//! (docs/video_clip_spec.md).
//!
//! This module is the **deterministic context half** of the video clip
//! workflow: `video_clip_context` turns one article into structured JSON for
//! the session agent. The **agentic half** is the embedded `video-script`
//! skill, which turns that context into a clip definition (schema v1).
//!
//! `video_clip_render` is an **operator-tier** tool (docs/CLI_COMMERCIAL.md):
//! dev-machine only, not part of the commercial free/paid promise. It spawns
//! the external render engine (`video-engine/generate-clip.sh`, Node/FFmpeg
//! toolchain) per the AGENTS.md §5 operator-tier subprocess allowance.

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use rusqlite::Connection;
use serde::Serialize;

use crate::content::{frontmatter, ops};
use crate::error::{Error, Result};

// ─── Types ───────────────────────────────────────────────────────────────────

/// Structured article context for the `video-script` skill (schema input).
#[derive(Debug, Clone, Serialize)]
pub struct VideoClipContext {
    pub slug: String,
    pub title: String,
    /// First `# ` heading in the body, if present.
    pub h1: Option<String>,
    /// Article file path relative to the project root.
    pub file_path: String,
    pub published_at: Option<String>,
    pub status: String,
    pub word_count: usize,
    pub frontmatter: VideoClipFrontmatter,
    /// MDX body with frontmatter stripped.
    pub body: String,
    /// Fetchable site base URL (`models::project::site_base_url`), if configured.
    pub site_base_url: Option<String>,
    pub packaging_hints: VideoClipPackagingHints,
}

/// Frontmatter fields relevant to clip scripting.
#[derive(Debug, Clone, Serialize)]
pub struct VideoClipFrontmatter {
    pub target_keyword: Option<String>,
    pub description: Option<String>,
    pub summary: Option<String>,
    pub canonical: Option<String>,
    /// Raw `faq:` YAML value as JSON (list of Q/A pairs), if present.
    pub faq: Option<serde_json::Value>,
    pub last_updated: Option<String>,
}

/// Deterministic upload-metadata hints derived from keyword + canonical.
#[derive(Debug, Clone, Serialize)]
pub struct VideoClipPackagingHints {
    pub hashtags: Vec<String>,
    pub canonical_url: Option<String>,
}

/// Result of a successful operator-tier render run.
#[derive(Debug, Clone, Serialize)]
pub struct VideoClipRenderResult {
    /// Absolute path of the rendered MP4 (parsed from the engine's
    /// `video-engine: output=<path>` stdout contract line).
    pub output_path: String,
    /// Thumbnail path from `video-engine: thumbnail=<path>` when present;
    /// otherwise a sibling filesystem guess (`png`/`jpg` next to the output).
    pub thumbnail_path: Option<String>,
    /// Duration in seconds via ffprobe; `None` when ffprobe is unavailable.
    pub duration_s: Option<f64>,
    /// Clip definition file the render ran from (absolute).
    pub clip_path: String,
}

// ─── Desk read: video_clip_context ───────────────────────────────────────────

/// Build the video clip context for one article slug.
///
/// Mirrors the `article` desk read (`engine::site_state::get_article_package`):
/// catalog lookup by slug, then the same file resolution (`resolve_content_file`
/// handles both repo-root-relative and content-dir-relative `article.file`
/// values — `content::ops::load_article_by_slug` alone breaks on the former).
/// Local reads only — free tier.
pub fn video_clip_context(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    slug: &str,
) -> Result<VideoClipContext> {
    let project_root = Path::new(project_path);
    let want = crate::content::slug::normalize_url_slug(slug);
    if want.is_empty() {
        return Err(Error::Validation("slug is required".into()));
    }
    let articles = crate::engine::task_store::list_articles(conn, project_id)?;
    let article = articles
        .iter()
        .find(|a| crate::content::slug::normalize_url_slug(&a.url_slug) == want || a.url_slug == slug)
        .ok_or_else(|| Error::Other(format!("Article not found for slug '{slug}'")))?;

    let file_path =
        crate::engine::exec::audit_health::resolve_content_file(project_root, &article.file)
            .ok_or_else(|| {
                Error::Other(format!(
                    "Article file not found for '{}' (catalog file: {})",
                    article.url_slug, article.file
                ))
            })?;
    let raw = std::fs::read_to_string(&file_path)
        .map_err(|e| Error::Other(format!("Failed to read article file: {e}")))?;

    let site_base_url = crate::engine::task_store::get_project(conn, project_id)
        .ok()
        .and_then(|p| p.site_base_url());

    Ok(build_context(
        &article.url_slug,
        &article.title,
        article.published_date.clone(),
        &article.status,
        article.target_keyword.as_deref(),
        &raw,
        &file_path,
        project_root,
        site_base_url,
    ))
}

/// Pure context assembly — no DB, no IO. Kept separate so the mapping is
/// unit-testable without fixtures.
#[allow(clippy::too_many_arguments)]
fn build_context(
    slug: &str,
    title: &str,
    published_at: Option<String>,
    status: &str,
    db_target_keyword: Option<&str>,
    raw_mdx: &str,
    file_path: &Path,
    project_root: &Path,
    site_base_url: Option<String>,
) -> VideoClipContext {
    let (fm_text, body) = frontmatter::split_mdx(raw_mdx).unwrap_or(("", raw_mdx));
    let parsed = frontmatter::parse(fm_text).ok();

    let fm_str = |key: &str| -> Option<String> {
        parsed
            .as_ref()
            .and_then(|f| f.parsed.get(key))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    };

    let target_keyword = fm_str("target_keyword").or_else(|| db_target_keyword.map(str::to_string));
    let canonical = fm_str("canonical");
    let faq = parsed
        .as_ref()
        .and_then(|f| f.parsed.get("faq"))
        .and_then(|v| serde_json::to_value(v).ok())
        .filter(|v| !v.is_null());

    let h1 = body
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("# ") && !l.starts_with("## "))
        .map(|l| l.trim_start_matches('#').trim().to_string());

    let relative_path = file_path
        .strip_prefix(project_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| file_path.to_string_lossy().to_string());
    let relative_path = relative_path
        .strip_prefix("./")
        .unwrap_or(&relative_path)
        .to_string();

    let canonical_url = canonical.clone().or_else(|| {
        site_base_url
            .as_deref()
            .filter(|b| !b.is_empty())
            .map(|b| format!("{b}/blog/{slug}"))
    });

    VideoClipContext {
        slug: slug.to_string(),
        title: title.to_string(),
        h1,
        file_path: relative_path,
        published_at,
        status: status.to_string(),
        word_count: ops::count_words(body),
        frontmatter: VideoClipFrontmatter {
            target_keyword: target_keyword.clone(),
            description: fm_str("description"),
            summary: fm_str("summary"),
            canonical,
            faq,
            last_updated: fm_str("lastUpdated"),
        },
        body: body.to_string(),
        site_base_url,
        packaging_hints: VideoClipPackagingHints {
            hashtags: derive_hashtags(target_keyword.as_deref().unwrap_or("")),
            canonical_url,
        },
    }
}

/// Trivial hashtag derivation from the target keyword: one tag from the whole
/// phrase concatenated, plus one per word (≥4 chars), lowercase alphanumeric,
/// deduped, capped at 5.
fn derive_hashtags(target_keyword: &str) -> Vec<String> {
    let words: Vec<String> = target_keyword
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_lowercase())
        .collect();
    let mut tags: Vec<String> = Vec::new();
    let mut push = |tag: String| {
        if tag.len() > 1 && !tags.contains(&tag) && tags.len() < 5 {
            tags.push(tag);
        }
    };
    if !words.is_empty() {
        push(format!("#{}", words.concat()));
    }
    for w in words.iter().filter(|w| w.len() >= 4) {
        push(format!("#{w}"));
    }
    tags
}

// ─── Operator tier: video_clip_render ────────────────────────────────────────

/// Run the external render engine for one clip definition.
///
/// **Operator tier** — assumes a source checkout of pageseeds-app: the engine
/// is located at `<repo>/video-engine/generate-clip.sh` relative to this
/// crate's manifest. Not part of the commercial free/paid boundary; the
/// prebuilt customer binary does not promise it.
///
/// Pre-flight fails fast with install hints when the Node/FFmpeg toolchain,
/// the engine script, or the project's `video.config.json` is missing.
pub fn video_clip_render(project_path: &Path, clip_path: &Path) -> Result<VideoClipRenderResult> {
    let engine = resolve_engine_script()?;
    check_video_config(project_path)?;
    require_tool_on_path(
        "node",
        "install Node.js (https://nodejs.org or `brew install node`)",
    )?;
    require_tool_on_path("ffmpeg", "install FFmpeg (`brew install ffmpeg`)")?;
    let clip_abs = resolve_clip_path(project_path, clip_path)?;

    let engine_dir = engine
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));

    // stdout is piped and tee'd to the operator's terminal (progress), stderr
    // inherited; lines are kept to parse the `video-engine: output=` /
    // `video-engine: thumbnail=` contract at the end.
    let mut child = Command::new("bash")
        .arg(&engine)
        .arg(project_path)
        .arg(&clip_abs)
        .current_dir(&engine_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| Error::Other(format!("failed to spawn {}: {e}", engine.display())))?;

    let mut lines: Vec<String> = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        for line in std::io::BufReader::new(stdout).lines() {
            match line {
                Ok(l) => {
                    println!("{l}");
                    lines.push(l);
                }
                Err(_) => break,
            }
        }
    }
    let status = child
        .wait()
        .map_err(|e| Error::Other(format!("failed to wait on render engine: {e}")))?;

    if !status.success() {
        let tail: Vec<&str> = lines.iter().rev().take(20).rev().map(String::as_str).collect();
        return Err(Error::Other(format!(
            "video render failed (exit {}); engine stage output tail:\n{}",
            status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string()),
            tail.join("\n")
        )));
    }

    let output_path = parse_output_path(&lines).ok_or_else(|| {
        Error::Other(
            "render succeeded but no `video-engine: output=<path>` line found on engine stdout — \
             stdout contract (video-engine/generate-clip.sh) violated"
                .to_string(),
        )
    })?;

    let output = PathBuf::from(&output_path);
    // Prefer the engine's contractual thumbnail line; fall back to a sibling
    // filesystem guess only when the engine did not print one.
    let thumbnail_path = parse_thumbnail_path(&lines).or_else(|| {
        ["png", "jpg"]
            .iter()
            .map(|ext| output.with_extension(ext))
            .find(|p| p.is_file())
            .map(|p| p.to_string_lossy().to_string())
    });

    Ok(VideoClipRenderResult {
        output_path,
        thumbnail_path,
        duration_s: probe_duration_s(&output),
        clip_path: clip_abs.to_string_lossy().to_string(),
    })
}

/// Engine entry point in the pageseeds-app source checkout
/// (`crates/pageseeds-core` → `<repo>/video-engine/generate-clip.sh`).
fn resolve_engine_script() -> Result<PathBuf> {
    let engine = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../video-engine/generate-clip.sh");
    if !engine.is_file() {
        return Err(Error::Other(format!(
            "video engine not found at {} — video-clip-render is an operator-tier tool \
             that assumes a source checkout of pageseeds-app with video-engine/ present \
             (docs/CLI_COMMERCIAL.md \"Operator tier\")",
            engine.display()
        )));
    }
    Ok(engine)
}

/// The project's `video.config.json` (engine input) must exist.
fn check_video_config(project_path: &Path) -> Result<PathBuf> {
    let config = project_path.join("video.config.json");
    if !config.is_file() {
        return Err(Error::Other(format!(
            "video.config.json not found in {} — create one per docs/video_clip_spec.md \
             before rendering clips for this project",
            project_path.display()
        )));
    }
    Ok(config)
}

/// Resolve the clip definition path: absolute as-is, else relative to the
/// project root.
fn resolve_clip_path(project_path: &Path, clip_path: &Path) -> Result<PathBuf> {
    let resolved = if clip_path.is_absolute() {
        clip_path.to_path_buf()
    } else {
        project_path.join(clip_path)
    };
    if !resolved.is_file() {
        return Err(Error::Other(format!(
            "clip definition not found: {} (pass --clip relative to the project root or absolute)",
            resolved.display()
        )));
    }
    Ok(resolved)
}

/// Operator-tier PATH probe with an install hint.
fn require_tool_on_path(tool: &str, install_hint: &str) -> Result<()> {
    let on_path = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(tool);
                candidate.is_file()
            })
        })
        .unwrap_or(false);
    if !on_path {
        return Err(Error::Other(format!(
            "'{tool}' not found on PATH — {install_hint}. video-clip-render is an \
             operator-tier tool and requires external toolchains (docs/CLI_COMMERCIAL.md)"
        )));
    }
    Ok(())
}

/// Last `video-engine: output=<path>` line wins (engine stdout contract).
///
/// Only matches the `output=` key after the `video-engine: ` prefix so stage
/// lines like `video-engine: stage=record status=ok` are ignored. Bare
/// `OUTPUT: …` markers are not contractual and are rejected.
fn parse_output_path(lines: &[String]) -> Option<String> {
    parse_video_engine_kv(lines, "output=")
}

/// Last `video-engine: thumbnail=<path>` line wins (engine stdout contract).
fn parse_thumbnail_path(lines: &[String]) -> Option<String> {
    parse_video_engine_kv(lines, "thumbnail=")
}

/// Shared parser for `video-engine: <key>=<value>` contract lines.
/// `key_prefix` is the key including `=`, e.g. `"output="`.
fn parse_video_engine_kv(lines: &[String], key_prefix: &str) -> Option<String> {
    lines.iter().rev().find_map(|l| {
        let trimmed = l.trim();
        let rest = trimmed.strip_prefix("video-engine: ")?;
        let value = rest.strip_prefix(key_prefix)?;
        let value = value.trim();
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    })
}

/// ffprobe duration in seconds; `None` when ffprobe is not installed or the
/// probe fails (duration is informational only).
fn probe_duration_s(output: &Path) -> Option<f64> {
    if require_tool_on_path("ffprobe", "").is_err() {
        return None;
    }
    let out = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(output)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_MDX: &str = r#"---
title: Best Stocks for Selling Cash-Secured Puts in 2026
description: The exact checklist before selling puts.
target_keyword: cash-secured puts
lastUpdated: 2026-07-01
faq:
  - question: What is a cash-secured put?
    answer: A put sold with enough cash to buy the shares.
---

# Best Stocks for Selling Cash-Secured Puts in 2026

Selling puts on the wrong stock is how accounts blow up.

## The three filters

Filter one. Filter two.
"#;

    fn ctx(keyword: Option<&str>, site: Option<String>) -> VideoClipContext {
        build_context(
            "best-stocks-csp",
            "Best Stocks for Selling Cash-Secured Puts in 2026",
            Some("2026-06-15".to_string()),
            "published",
            keyword,
            FIXTURE_MDX,
            Path::new("/proj/content/blog/best-stocks-csp.mdx"),
            Path::new("/proj"),
            site,
        )
    }

    #[test]
    fn context_strips_frontmatter_and_counts_body_words() {
        let c = ctx(None, None);
        assert!(!c.body.contains("target_keyword:"), "frontmatter must be stripped");
        assert!(c.body.contains("Selling puts on the wrong stock"));
        assert_eq!(c.word_count, ops::count_words(&c.body));
        assert!(c.word_count > 0);
    }

    #[test]
    fn context_extracts_frontmatter_fields() {
        let c = ctx(None, None);
        assert_eq!(c.frontmatter.target_keyword.as_deref(), Some("cash-secured puts"));
        assert_eq!(
            c.frontmatter.description.as_deref(),
            Some("The exact checklist before selling puts.")
        );
        assert_eq!(c.frontmatter.last_updated.as_deref(), Some("2026-07-01"));
        assert_eq!(
            c.h1.as_deref(),
            Some("Best Stocks for Selling Cash-Secured Puts in 2026")
        );
        let faq = c.frontmatter.faq.expect("faq parsed");
        assert_eq!(faq[0]["question"], "What is a cash-secured put?");
        assert_eq!(c.file_path, "content/blog/best-stocks-csp.mdx");
        assert_eq!(c.status, "published");
    }

    #[test]
    fn db_keyword_is_fallback_when_frontmatter_missing() {
        let raw = "---\ntitle: T\n---\n\n# T\n\nBody words here.\n";
        let c = build_context(
            "s",
            "T",
            None,
            "draft",
            Some("db keyword"),
            raw,
            Path::new("/p/s.mdx"),
            Path::new("/p"),
            None,
        );
        assert_eq!(c.frontmatter.target_keyword.as_deref(), Some("db keyword"));
        assert!(c.packaging_hints.hashtags.contains(&"#dbkeyword".to_string()));
    }

    #[test]
    fn canonical_url_derives_from_site_base_url() {
        let c = ctx(None, Some("https://example.com".to_string()));
        assert_eq!(
            c.packaging_hints.canonical_url.as_deref(),
            Some("https://example.com/blog/best-stocks-csp")
        );
        // Explicit frontmatter canonical wins over the derived URL.
        let raw = "---\ntitle: T\ncanonical: https://other.com/x\n---\n\nBody.\n";
        let c2 = build_context(
            "s",
            "T",
            None,
            "published",
            None,
            raw,
            Path::new("/p/s.mdx"),
            Path::new("/p"),
            Some("https://example.com".to_string()),
        );
        assert_eq!(
            c2.packaging_hints.canonical_url.as_deref(),
            Some("https://other.com/x")
        );
    }

    #[test]
    fn hashtag_derivation_from_keyword() {
        let tags = derive_hashtags("cash-secured puts");
        assert_eq!(tags[0], "#cashsecuredputs");
        assert!(tags.contains(&"#cash".to_string()));
        assert!(tags.contains(&"#secured".to_string()));
        assert!(tags.contains(&"#puts".to_string()));
        assert!(tags.len() <= 5);
        assert!(derive_hashtags("").is_empty());
        // Dedup + case folding
        let dup = derive_hashtags("Puts puts");
        assert_eq!(dup.iter().filter(|t| *t == "#putsputs").count(), 1);
    }

    #[test]
    fn parse_output_path_last_video_engine_line_wins() {
        let lines = vec![
            "video-engine: stage=record status=start".to_string(),
            "video-engine: output=/tmp/first.mp4".to_string(),
            "video-engine: stage=composite status=ok".to_string(),
            "video-engine: output=/tmp/final.mp4".to_string(),
        ];
        assert_eq!(parse_output_path(&lines).as_deref(), Some("/tmp/final.mp4"));
        assert!(parse_output_path(&["no marker here".to_string()]).is_none());
        // Stage lines must not be mistaken for output=.
        assert!(parse_output_path(&[
            "video-engine: stage=record status=ok".to_string()
        ])
        .is_none());
    }

    #[test]
    fn parse_output_path_rejects_bare_output_marker() {
        // Legacy / non-contractual marker — must not be accepted.
        let lines = vec!["OUTPUT: /tmp/x.mp4".to_string()];
        assert!(
            parse_output_path(&lines).is_none(),
            "bare OUTPUT: is not the video-engine stdout contract"
        );
        // Mixed: bare OUTPUT: ignored, contractual line wins.
        let mixed = vec![
            "OUTPUT: /tmp/legacy.mp4".to_string(),
            "video-engine: output=/tmp/contract.mp4".to_string(),
        ];
        assert_eq!(
            parse_output_path(&mixed).as_deref(),
            Some("/tmp/contract.mp4")
        );
    }

    #[test]
    fn parse_thumbnail_path_last_video_engine_line_wins() {
        let lines = vec![
            "video-engine: output=/tmp/final.mp4".to_string(),
            "video-engine: thumbnail=/tmp/first.jpg".to_string(),
            "video-engine: thumbnail=/tmp/final.jpg".to_string(),
        ];
        assert_eq!(
            parse_thumbnail_path(&lines).as_deref(),
            Some("/tmp/final.jpg")
        );
        assert!(parse_thumbnail_path(&["no marker".to_string()]).is_none());
        assert!(parse_thumbnail_path(&[
            "video-engine: output=/tmp/only.mp4".to_string()
        ])
        .is_none());
    }

    #[test]
    fn require_tool_on_path_missing_tool_errors_with_hint() {
        let err = require_tool_on_path(
            "pageseeds-tool-that-does-not-exist-xyz",
            "install it somehow",
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("not found on PATH"), "unexpected: {msg}");
        assert!(msg.contains("install it somehow"), "unexpected: {msg}");
    }

    #[test]
    fn video_config_missing_errors_with_spec_pointer() {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-video-test-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let err = check_video_config(&dir).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("video.config.json"), "unexpected: {msg}");
        assert!(msg.contains("docs/video_clip_spec.md"), "unexpected: {msg}");
        std::fs::write(dir.join("video.config.json"), "{}").unwrap();
        assert!(check_video_config(&dir).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clip_path_resolves_relative_to_project_root() {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-video-clip-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(dir.join("clips")).unwrap();
        std::fs::write(dir.join("clips/clip.json"), "{}").unwrap();
        let rel = resolve_clip_path(&dir, Path::new("clips/clip.json")).unwrap();
        assert!(rel.is_absolute());
        let abs = resolve_clip_path(&dir, &dir.join("clips/clip.json")).unwrap();
        assert_eq!(abs, dir.join("clips/clip.json"));
        assert!(resolve_clip_path(&dir, Path::new("clips/missing.json")).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
