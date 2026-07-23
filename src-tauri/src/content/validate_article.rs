//! Deterministic structural SEO quality gates for a single MDX article.
//!
//! Shared source of truth used by write verify, optionally fix verify, CLI
//! (`validate-article`), and the investigate RO tool. No LLM scoring — only
//! structural floors (issue #122 / epic #117).

use std::collections::HashSet;
use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Meta description length floors (SEO structural gate — content domain SoT).
/// Re-exported from `engine::exec::audit_health` for existing call sites.
pub const META_MIN_LEN: usize = 120;
pub const META_MAX_LEN: usize = 155;

/// Default minimum body word count (shared with `content_audit` word_count).
pub const DEFAULT_MIN_WORD_COUNT: usize = 800;

/// One deterministic check result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ArticleCheck {
    pub id: String,
    pub pass: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Full validation report for one article.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ValidateArticleResult {
    pub slug: String,
    pub ok: bool,
    pub checks: Vec<ArticleCheck>,
}

/// Optional inputs that tune or enable individual checks.
#[derive(Debug, Clone, Default)]
pub struct ValidateArticleInput {
    /// Target keyword for presence check (optional).
    pub target_keyword: Option<String>,
    /// Normalized url_slugs that are valid internal link targets.
    /// If `None`, `internal_links_resolve` auto-passes.
    pub valid_link_targets: Option<HashSet<String>>,
    /// Default [`DEFAULT_MIN_WORD_COUNT`] (800).
    pub min_word_count: Option<usize>,
}

fn check(id: &str, pass: bool, detail: Option<String>) -> ArticleCheck {
    ArticleCheck {
        id: id.to_string(),
        pass,
        detail,
    }
}

/// Pure validation over already-loaded MDX. Prefer this in unit tests and write_verify.
pub fn validate_article_content(
    slug: &str,
    content: &str,
    input: &ValidateArticleInput,
) -> ValidateArticleResult {
    let mut checks = Vec::with_capacity(7);

    // ── mdx_structure ──────────────────────────────────────────────────────
    match crate::content::cleaner::validate_mdx_structure(content) {
        Ok(()) => checks.push(check("mdx_structure", true, None)),
        Err(e) => checks.push(check("mdx_structure", false, Some(e))),
    }

    let (fm_text, body) = crate::content::frontmatter::split_mdx(content)
        .map(|(f, b)| (Some(f), b))
        .unwrap_or((None, content));

    // ── has_h1 ─────────────────────────────────────────────────────────────
    let has_h1 = body.lines().any(|l| {
        let t = l.trim_start();
        t.starts_with("# ") && !t.starts_with("## ")
    });
    checks.push(check(
        "has_h1",
        has_h1,
        if has_h1 {
            None
        } else {
            Some("no H1 heading (# …) in body".to_string())
        },
    ));

    // ── frontmatter_title ──────────────────────────────────────────────────
    let title = fm_text
        .and_then(|fm| {
            crate::content::frontmatter::top_level_scalars(fm)
                .into_iter()
                .find(|f| f.key == "title")
                .map(|f| {
                    f.raw_value
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string()
                })
        })
        .filter(|t| !t.is_empty());
    let title_ok = title.is_some();
    checks.push(check(
        "frontmatter_title",
        title_ok,
        if title_ok {
            None
        } else {
            Some("missing or empty title".to_string())
        },
    ));

    // ── meta_description_length ────────────────────────────────────────────
    let description = fm_text
        .and_then(|fm| {
            let scalars = crate::content::frontmatter::top_level_scalars(fm);
            for key in ["description", "metaDescription", "meta_description"] {
                if let Some(f) = scalars.iter().find(|f| f.key == key) {
                    let v = f
                        .raw_value
                        .trim()
                        .trim_matches('"')
                        .trim_matches('\'')
                        .to_string();
                    if !v.is_empty() {
                        return Some(v);
                    }
                }
            }
            None
        })
        .unwrap_or_default();
    let desc_len = description.chars().count();
    let meta_ok = desc_len >= META_MIN_LEN && desc_len <= META_MAX_LEN;
    checks.push(check(
        "meta_description_length",
        meta_ok,
        Some(format!(
            "len={}, want {}–{}",
            desc_len, META_MIN_LEN, META_MAX_LEN
        )),
    ));

    // ── target_keyword_in_body ─────────────────────────────────────────────
    let kw = input
        .target_keyword
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match kw {
        None => checks.push(check(
            "target_keyword_in_body",
            true,
            Some("no target keyword".to_string()),
        )),
        Some(keyword) => {
            let body_lower = body.to_lowercase();
            let present = crate::content::keyword_match::keyword_present(&body_lower, keyword);
            checks.push(check(
                "target_keyword_in_body",
                present,
                if present {
                    None
                } else {
                    Some(format!("keyword not found in body: {keyword}"))
                },
            ));
        }
    }

    // ── internal_links_resolve ─────────────────────────────────────────────
    let links = crate::content::linking::extract_blog_link_hrefs(content);
    match &input.valid_link_targets {
        None => {
            // No target set provided → auto-pass.
            checks.push(check(
                "internal_links_resolve",
                true,
                if links.is_empty() {
                    Some("no valid-target set; zero internal blog links".to_string())
                } else {
                    Some("no valid-target set provided".to_string())
                },
            ));
        }
        Some(_) if links.is_empty() => {
            checks.push(check(
                "internal_links_resolve",
                true,
                Some("zero internal blog links".to_string()),
            ));
        }
        Some(targets) => {
            let mut broken: Vec<String> = Vec::new();
            for (_anchor, _href, slug_written) in &links {
                // Canonical resolution: exact match first, then normalize_url_slug fallback
                // (protects numeric-leading slugs like `5-best-coffees`).
                if crate::content::slug::resolve_slug(slug_written, targets).is_none() {
                    broken.push(slug_written.clone());
                }
            }
            broken.sort();
            broken.dedup();
            if broken.is_empty() {
                checks.push(check("internal_links_resolve", true, None));
            } else {
                checks.push(check(
                    "internal_links_resolve",
                    false,
                    Some(format!("broken: {}", broken.join(", "))),
                ));
            }
        }
    }

    // ── min_word_count ─────────────────────────────────────────────────────
    let min_words = input.min_word_count.unwrap_or(DEFAULT_MIN_WORD_COUNT);
    let word_count = crate::content::ops::count_words(body);
    let words_ok = word_count >= min_words;
    checks.push(check(
        "min_word_count",
        words_ok,
        Some(if words_ok {
            word_count.to_string()
        } else {
            format!("{word_count} (want ≥ {min_words})")
        }),
    ));

    let ok = checks.iter().all(|c| c.pass);
    ValidateArticleResult {
        slug: slug.to_string(),
        ok,
        checks,
    }
}

/// Load article by slug from project + DB, then validate.
///
/// Prefer `load_article_by_slug` when registered (gets `target_keyword` from the
/// article row). Fall back to filesystem resolve for orphan MDX files (CLI).
pub fn validate_article(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    slug: &str,
) -> Result<ValidateArticleResult> {
    let (resolved_slug, content, target_keyword) =
        match crate::content::ops::load_article_by_slug(conn, project_id, project_path, None, slug) {
            Ok((article, file_content, _path)) => {
                let kw = article.target_keyword.filter(|k| !k.trim().is_empty());
                let resolved = if article.url_slug.is_empty() {
                    slug.to_string()
                } else {
                    article.url_slug
                };
                (resolved, file_content, kw)
            }
            Err(_) => {
                // Orphan / not in DB: resolve path and read file.
                let path = crate::content::ops::resolve_slug_or_path(project_path, slug)
                    .map_err(crate::error::Error::Other)?;
                let file_content = std::fs::read_to_string(&path).map_err(|e| {
                    crate::error::Error::Other(format!("Failed to read article file: {e}"))
                })?;
                let file_name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(slug);
                let resolved = crate::content::ops::slug_from_filename(file_name);
                let kw = crate::content::frontmatter::extract_frontmatter_string(
                    &file_content,
                    "target_keyword",
                )
                .filter(|k| !k.trim().is_empty());
                (resolved, file_content, kw)
            }
        };

    let valid_link_targets =
        match crate::engine::task_store::load_valid_link_targets(conn, project_id, &project_path.to_string_lossy())
        {
            Ok(set) => Some(set),
            Err(e) => {
                log::warn!(
                    "[validate_article] load_valid_link_targets failed (internal links auto-pass): {e}"
                );
                None
            }
        };

    let input = ValidateArticleInput {
        target_keyword,
        valid_link_targets,
        min_word_count: None,
    };

    Ok(validate_article_content(&resolved_slug, &content, &input))
}

/// Format failed checks for a human-readable StepResult message.
pub fn format_failed_checks(result: &ValidateArticleResult) -> String {
    result
        .checks
        .iter()
        .filter(|c| !c.pass)
        .map(|c| match &c.detail {
            Some(d) => format!("{} ({})", c.id, d),
            None => c.id.clone(),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta_ok() -> String {
        // Exactly 130 chars in [120, 155].
        "a".repeat(130)
    }

    fn body_with_words(n: usize, keyword: &str) -> String {
        // count_words strips markdown; plain words count 1:1.
        format!(
            "# {keyword}\n\n{keyword} is great.\n\n{}",
            "word ".repeat(n)
        )
    }

    fn good_article(keyword: &str) -> String {
        format!(
            "---\ntitle: Best Cold Brew Maker\ndescription: {}\n---\n\n{}",
            meta_ok(),
            body_with_words(900, keyword)
        )
    }

    fn check_by_id<'a>(
        result: &'a ValidateArticleResult,
        id: &str,
    ) -> &'a ArticleCheck {
        result
            .checks
            .iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("missing check id {id}"))
    }

    #[test]
    fn good_article_passes_all_checks() {
        let kw = "cold brew maker";
        let content = format!(
            "---\ntitle: Best Cold Brew Maker\ndescription: {}\n---\n\n# Best cold brew maker\n\n\
             cold brew maker guide here.\n\nSee [related](/blog/other-post).\n\n{}",
            meta_ok(),
            "word ".repeat(900)
        );
        let mut targets = HashSet::new();
        targets.insert("other-post".to_string());
        let input = ValidateArticleInput {
            target_keyword: Some(kw.to_string()),
            valid_link_targets: Some(targets),
            min_word_count: None,
        };
        let result = validate_article_content("best-cold-brew-maker", &content, &input);
        assert!(result.ok, "{:?}", result.checks);
        for id in [
            "mdx_structure",
            "has_h1",
            "frontmatter_title",
            "meta_description_length",
            "target_keyword_in_body",
            "internal_links_resolve",
            "min_word_count",
        ] {
            assert!(check_by_id(&result, id).pass, "expected {id} to pass");
        }
        assert_eq!(result.slug, "best-cold-brew-maker");
    }

    #[test]
    fn missing_h1_fails() {
        let content = format!(
            "---\ntitle: Title Here\ndescription: {}\n---\n\n## Sub only\n\n{}",
            meta_ok(),
            "word ".repeat(900)
        );
        let result = validate_article_content(
            "no-h1",
            &content,
            &ValidateArticleInput::default(),
        );
        assert!(!result.ok);
        assert!(!check_by_id(&result, "has_h1").pass);
    }

    #[test]
    fn broken_frontmatter_fails_mdx_structure() {
        let content = "---\ntitle: Open only\n# Body without closed frontmatter\n";
        let result = validate_article_content(
            "broken",
            content,
            &ValidateArticleInput::default(),
        );
        assert!(!result.ok);
        assert!(!check_by_id(&result, "mdx_structure").pass);
    }

    #[test]
    fn short_meta_fails() {
        let content = format!(
            "---\ntitle: Title\ndescription: too short\n---\n\n# H1\n\n{}",
            "word ".repeat(900)
        );
        let result = validate_article_content(
            "short-meta",
            &content,
            &ValidateArticleInput::default(),
        );
        assert!(!result.ok);
        let c = check_by_id(&result, "meta_description_length");
        assert!(!c.pass);
        let detail = c.detail.as_deref().unwrap_or("");
        assert!(detail.contains("len="), "{detail}");
        assert!(detail.contains("120"), "{detail}");
    }

    #[test]
    fn keyword_missing_from_body_fails() {
        let content = format!(
            "---\ntitle: Title\ndescription: {}\n---\n\n# Something else\n\n{}",
            meta_ok(),
            "word ".repeat(900)
        );
        let input = ValidateArticleInput {
            target_keyword: Some("cold brew maker".to_string()),
            ..Default::default()
        };
        let result = validate_article_content("kw-miss", &content, &input);
        assert!(!result.ok);
        assert!(!check_by_id(&result, "target_keyword_in_body").pass);
    }

    #[test]
    fn broken_internal_link_fails() {
        let content = format!(
            "---\ntitle: Title\ndescription: {}\n---\n\n# H1\n\nSee [x](/blog/ghost-page).\n\n{}",
            meta_ok(),
            "word ".repeat(900)
        );
        let mut targets = HashSet::new();
        targets.insert("real-page".to_string());
        let input = ValidateArticleInput {
            valid_link_targets: Some(targets),
            ..Default::default()
        };
        let result = validate_article_content("broken-link", &content, &input);
        assert!(!result.ok);
        let c = check_by_id(&result, "internal_links_resolve");
        assert!(!c.pass);
        assert!(
            c.detail.as_deref().unwrap_or("").contains("ghost-page"),
            "{:?}",
            c.detail
        );
    }

    #[test]
    fn short_body_fails_min_word_count() {
        let content = format!(
            "---\ntitle: Title\ndescription: {}\n---\n\n# H1\n\nshort body only.\n",
            meta_ok()
        );
        let result = validate_article_content(
            "short-body",
            &content,
            &ValidateArticleInput::default(),
        );
        assert!(!result.ok);
        assert!(!check_by_id(&result, "min_word_count").pass);
    }

    #[test]
    fn no_keyword_provided_passes_keyword_check() {
        let content = good_article("anything");
        let result = validate_article_content(
            "no-kw",
            &content,
            &ValidateArticleInput {
                target_keyword: None,
                ..Default::default()
            },
        );
        let c = check_by_id(&result, "target_keyword_in_body");
        assert!(c.pass);
        assert_eq!(c.detail.as_deref(), Some("no target keyword"));
    }

    #[test]
    fn empty_keyword_string_treated_as_missing() {
        let content = good_article("anything");
        let result = validate_article_content(
            "empty-kw",
            &content,
            &ValidateArticleInput {
                target_keyword: Some("   ".to_string()),
                ..Default::default()
            },
        );
        let c = check_by_id(&result, "target_keyword_in_body");
        assert!(c.pass);
        assert_eq!(c.detail.as_deref(), Some("no target keyword"));
    }

    #[test]
    fn zero_links_with_targets_passes() {
        let content = good_article("cold brew");
        let mut targets = HashSet::new();
        targets.insert("other".to_string());
        let result = validate_article_content(
            "no-links",
            &content,
            &ValidateArticleInput {
                target_keyword: Some("cold brew".to_string()),
                valid_link_targets: Some(targets),
                ..Default::default()
            },
        );
        assert!(check_by_id(&result, "internal_links_resolve").pass);
        assert!(result.ok, "{:?}", result.checks);
    }

    /// Numeric-leading slug must resolve via exact match first (`resolve_slug`),
    /// not bare `normalize_url_slug` (which would strip `5-` → `best-coffees`).
    #[test]
    fn internal_links_resolve_numeric_leading_exact_match() {
        let content = format!(
            "---\ntitle: Title\ndescription: {}\n---\n\n# H1\n\n\
             See [coffees](/blog/5-best-coffees).\n\n{}",
            meta_ok(),
            "word ".repeat(900)
        );
        let mut targets = HashSet::new();
        targets.insert("5-best-coffees".to_string());
        let input = ValidateArticleInput {
            valid_link_targets: Some(targets),
            ..Default::default()
        };
        let result = validate_article_content("num-exact", &content, &input);
        assert!(
            check_by_id(&result, "internal_links_resolve").pass,
            "exact-match numeric-leading slug should resolve: {:?}",
            check_by_id(&result, "internal_links_resolve")
        );
    }

    /// Prefixed / underscore form resolves via `normalize_url_slug` fallback.
    #[test]
    fn internal_links_resolve_normalized_fallback() {
        let content = format!(
            "---\ntitle: Title\ndescription: {}\n---\n\n# H1\n\n\
             See [profile](/blog/001_roast_profile_management).\n\n{}",
            meta_ok(),
            "word ".repeat(900)
        );
        let mut targets = HashSet::new();
        targets.insert("roast-profile-management".to_string());
        let input = ValidateArticleInput {
            valid_link_targets: Some(targets),
            ..Default::default()
        };
        let result = validate_article_content("num-fallback", &content, &input);
        assert!(
            check_by_id(&result, "internal_links_resolve").pass,
            "normalized fallback should resolve: {:?}",
            check_by_id(&result, "internal_links_resolve")
        );
    }
}
