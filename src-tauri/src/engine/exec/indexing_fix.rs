/// Canonical 4-step pipeline for fix_indexing and fix_technical tasks.
///
/// Step 1 (deterministic): Load the target MDX file and extract structured context
/// (including the parsed task description fields) so the agent doesn't waste
/// time hunting for files.
///
/// Step 2 (agentic): Generate a structured `IndexingFixPlan` JSON. The agent
/// NEVER edits files — direct mode has no file I/O on most providers (Kimi
/// bridge `direct` advertises `file_io: false`; Claude/OpenAI/Ollama rig
/// agents are built with no tools). It only proposes changes.
///
/// Step 3 (deterministic): Apply the plan to the MDX file with
/// snapshot/restore. Fails loudly when the plan produces no effective change.
///
/// Step 4 (deterministic): Re-read the file and verify every planned change
/// landed. Fails loudly when the file is unchanged.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexingFixContext {
    pub url: String,
    pub file_path: Option<String>,
    pub exists: bool,
    pub word_count: usize,
    pub h1: Option<String>,
    pub title: Option<String>,
    pub meta_description: Option<String>,
    pub canonical: Option<String>,
    pub publish_date: Option<String>,
    pub internal_links: Vec<String>,
    pub internal_link_count: usize,
    // ─── Parsed from the task description (by prefix, any line) ─────────────
    #[serde(default)]
    pub issue: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub recommended_action: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub suggested_title: Option<String>,
    #[serde(default)]
    pub suggested_h1: Option<String>,
}

/// Structured fields parsed from a fix task description.
///
/// Descriptions come from two spawn sites with different formats:
/// - `gsc_diagnostics`: `URL:` / `Issue:` / `Action:` / `Verdict:`
/// - indexing health campaign (`build_rewrite_spec`): `URL:` /
///   `Recommended action:` / `Reason:` / `Parent campaign:` /
///   `Suggested title:` / `Suggested H1:`
///
/// Parsing matches by prefix on ANY line — never by fixed line index.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct FixTaskDescription {
    pub url: String,
    pub issue: Option<String>,
    pub action: Option<String>,
    pub verdict: Option<String>,
    pub recommended_action: Option<String>,
    pub reason: Option<String>,
    pub suggested_title: Option<String>,
    pub suggested_h1: Option<String>,
}

pub(crate) fn parse_fix_task_description(description: &str) -> FixTaskDescription {
    let mut out = FixTaskDescription::default();
    for line in description.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("URL: ") {
            out.url = v.trim().to_string();
        } else if let Some(v) = line.strip_prefix("Issue: ") {
            out.issue = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Action: ") {
            out.action = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Verdict: ") {
            out.verdict = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Recommended action: ") {
            out.recommended_action = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Reason: ") {
            out.reason = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Suggested title: ") {
            out.suggested_title = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Suggested H1: ") {
            out.suggested_h1 = Some(v.trim().to_string());
        }
    }
    out
}

/// Typed fix plan returned by the agentic generate step (step 2).
///
/// The agent returns this as JSON; it never edits files directly. The
/// deterministic apply step (step 3) performs all writes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct IndexingFixPlan {
    /// One-line summary of the root cause being addressed.
    #[serde(default)]
    pub diagnosis: String,
    #[serde(default)]
    pub changes: IndexingFixChanges,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct IndexingFixChanges {
    /// New frontmatter `title`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// New top-level `# ` heading in the body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h1: Option<String>,
    /// New frontmatter `description` (meta description).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Replacement first paragraph of the body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intro: Option<String>,
    /// Frontmatter scalar updates for technical fixes (e.g. set `canonical`,
    /// change `robots` from noindex to index).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frontmatter: Option<Vec<FrontmatterEdit>>,
}

impl IndexingFixChanges {
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.h1.is_none()
            && self.description.is_none()
            && self.intro.is_none()
            && self.frontmatter.as_ref().map(|f| f.is_empty()).unwrap_or(true)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FrontmatterEdit {
    pub key: String,
    pub value: String,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 1 (deterministic): context
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministic pre-step: gather structured context for the target URL.
pub(crate) fn exec_indexing_fix_context(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let desc = parse_fix_task_description(task.description.as_deref().unwrap_or(""));
    let url = desc.url.clone();

    if url.is_empty() {
        return StepResult {
            success: false,
            message: "Task description missing URL".to_string(),
            output: None,
        };
    }

    // Resolve content directory
    let content_dir = crate::content::locator::resolve(Path::new(project_path), None)
        .selected
        .unwrap_or_else(|| paths.repo_root.clone());

    // Try to find the MDX file matching the URL slug
    let slug = crate::content::slug::extract_slug_from_url(&url);
    let file_match = find_mdx_by_slug(&content_dir, &slug);

    log::info!(
        "[indexing_fix_context] url={} content_dir={} slug={} matched={}",
        url,
        content_dir.display(),
        slug,
        file_match
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "none".to_string())
    );

    let mut ctx = IndexingFixContext {
        url: url.clone(),
        file_path: file_match.as_ref().map(|p| p.to_string_lossy().to_string()),
        exists: file_match.is_some(),
        word_count: 0,
        h1: None,
        title: None,
        meta_description: None,
        canonical: None,
        publish_date: None,
        internal_links: vec![],
        internal_link_count: 0,
        issue: desc.issue,
        action: desc.action,
        recommended_action: desc.recommended_action,
        reason: desc.reason,
        suggested_title: desc.suggested_title,
        suggested_h1: desc.suggested_h1,
    };

    if let Some(ref path) = file_match {
        if let Ok(content) = std::fs::read_to_string(path) {
            ctx.word_count = crate::content::ops::count_words(&content);
            ctx.h1 = extract_first_h1(&content);
            ctx.title = crate::content::frontmatter::extract_frontmatter_string(&content, "title");
            ctx.meta_description = crate::content::frontmatter::extract_frontmatter_string(&content, "description");
            ctx.canonical = crate::content::frontmatter::extract_frontmatter_string(&content, "canonical");
            ctx.publish_date = crate::content::frontmatter::extract_frontmatter_string(&content, "date");
            ctx.internal_links = extract_internal_links(&content);
            ctx.internal_link_count = ctx.internal_links.len();
        }
    }

    let output = serde_json::to_string_pretty(&ctx).unwrap_or_default();

    if !ctx.exists {
        return StepResult {
            success: false,
            message: format!(
                "No MDX file found for {} (slug={}). Cannot fix indexing for a page that has no content file.",
                url, slug
            ),
            output: Some(output),
        };
    }

    StepResult {
        success: true,
        message: format!(
            "Context loaded for {}: {} words, {} internal links{}",
            url,
            ctx.word_count,
            ctx.internal_link_count,
            if ctx.exists { "" } else { " (file not found)" }
        ),
        output: Some(output),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 2 (agentic): generate structured fix plan
// ═══════════════════════════════════════════════════════════════════════════════

/// Agentic step: produce a structured `IndexingFixPlan` JSON.
///
/// Cannot be deterministic: the fix depends on intent, content quality, and
/// site-specific conventions. The agent returns JSON only — it does NOT edit
/// files (direct mode has no file I/O on most providers).
///
/// Input contract: `IndexingFixContext` JSON from step 1 (via latest_raw) plus
/// the optional `indexing_target_context` cluster artifact.
/// Output contract: `IndexingFixPlan` JSON (see the indexing-fix skill).
pub(crate) fn exec_indexing_fix_generate(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: Option<&str>,
) -> StepResult {
    let ctx: IndexingFixContext = match context_json {
        Some(j) => match serde_json::from_str(j) {
            Ok(c) => c,
            Err(e) => {
                return StepResult {
                    success: false,
                    message: format!(
                        "indexing_fix_context output is not valid IndexingFixContext JSON: {}",
                        e
                    ),
                    output: None,
                }
            }
        },
        None => {
            return StepResult {
                success: false,
                message: "No context from indexing_fix_context step. Run the context step first."
                    .to_string(),
                output: None,
            }
        }
    };

    let context_block = format!(
        "\n\n## Page Context (Deterministic)\n\n```json\n{}\n```",
        serde_json::to_string_pretty(&ctx).unwrap_or_default()
    );

    let cluster_context_block = build_cluster_context_block(task);

    // Surface campaign-provided suggestions prominently so the agent uses them
    // instead of rewriting blind.
    let mut suggestions_block = String::new();
    if ctx.suggested_title.is_some() || ctx.suggested_h1.is_some() {
        suggestions_block.push_str("\n\n## Suggested Values (from site-wide audit)\n\n");
        if let Some(ref t) = ctx.suggested_title {
            suggestions_block.push_str(&format!("- Suggested title: {}\n", t));
        }
        if let Some(ref h) = ctx.suggested_h1 {
            suggestions_block.push_str(&format!("- Suggested H1: {}\n", h));
        }
        suggestions_block.push_str(
            "\nUse these suggested values as the basis for your `title` / `h1` changes. \
             Adjust only when they violate the skill rules.",
        );
    }

    let context = format!(
        "Task: Fix Indexing Issue\n\
         - Task ID: {}\n\
         - URL: {}\n\
         - Issue: {}\n\
         - Recommended Action: {}\n\
         - Reason: {}\n\
         - Repo: {}\n\
         {}\n\
         {}\n\
         {}",
        task.id,
        ctx.url,
        ctx.issue.as_deref().unwrap_or("unknown"),
        ctx.recommended_action
            .as_deref()
            .or(ctx.action.as_deref())
            .unwrap_or("unknown"),
        ctx.reason.as_deref().unwrap_or(""),
        project_path,
        suggestions_block,
        context_block,
        cluster_context_block,
    );

    let repo_root = Path::new(project_path);
    // The indexing-fix skill file contains the canonical Output Contract
    // (IndexingFixPlan JSON). The agent returns JSON only — no file edits.
    let raw = match crate::engine::agent::run_agent_with_skill(
        "indexing-fix",
        repo_root,
        &context,
        agent_provider,
        None,
    ) {
        Ok(output) => output,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Agent failed to generate fix plan: {}", e),
                output: None,
            }
        }
    };

    let plan: IndexingFixPlan = match crate::engine::text::extract_json_as(&raw) {
        Some(p) => p,
        None => {
            return StepResult {
                success: false,
                message: format!(
                    "Agent output did not contain a valid IndexingFixPlan JSON: {}",
                    crate::engine::text::char_prefix(&raw, 300)
                ),
                output: Some(raw),
            }
        }
    };

    if plan.changes.is_empty() {
        return StepResult {
            success: false,
            message: "Agent returned an IndexingFixPlan with no changes. \
                 Refusing to report success without any planned edit."
                .to_string(),
            output: serde_json::to_string_pretty(&plan).ok(),
        };
    }

    let plan_json = match serde_json::to_string_pretty(&plan) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize IndexingFixPlan: {}", e),
                output: None,
            }
        }
    };

    StepResult {
        success: true,
        message: format!("Generated IndexingFixPlan for {}", ctx.url),
        output: Some(plan_json),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3 (deterministic): apply plan with snapshot/restore
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministic step: apply the planned edits to the target MDX file.
///
/// Snapshots the original, applies title/description/H1/intro/frontmatter
/// changes, validates MDX structure, and restores the snapshot on corruption.
/// Fails loudly when the plan produces no effective change — a fix_indexing
/// task must never silently succeed without editing the file.
pub(crate) fn exec_indexing_fix_apply(
    task: &Task,
    project_path: &str,
    latest_raw: Option<&str>,
) -> StepResult {
    let plan = match resolve_plan(task, latest_raw) {
        Ok(p) => p,
        Err(result) => return result,
    };

    if plan.changes.is_empty() {
        return StepResult {
            success: false,
            message: "indexing_fix_plan contains no changes — refusing to report success \
                 without any edit."
                .to_string(),
            output: None,
        };
    }

    let file_path = match resolve_target_file(task, project_path) {
        Ok(p) => p,
        Err(result) => return result,
    };

    let original_content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to read {}: {}", file_path.display(), e),
                output: None,
            }
        }
    };

    let (fm, body) = match crate::content::frontmatter::split_mdx(&original_content) {
        Some((f, b)) => (f.to_string(), b.to_string()),
        None => {
            return StepResult {
                success: false,
                message: "Could not parse frontmatter from MDX file".to_string(),
                output: None,
            }
        }
    };

    let mut new_fm = fm.clone();
    let mut new_body = body.clone();
    let mut applied = Vec::new();

    if let Some(ref new_title) = plan.changes.title {
        new_fm = crate::content::frontmatter::replace_scalar(&new_fm, "title", new_title);
        applied.push("title".to_string());
    }

    if let Some(ref new_desc) = plan.changes.description {
        new_fm = crate::content::frontmatter::replace_scalar(&new_fm, "description", new_desc);
        applied.push("description".to_string());
    }

    if let Some(edits) = plan.changes.frontmatter {
        for edit in edits {
            if edit.key == "title" || edit.key == "description" {
                continue; // already handled above
            }
            new_fm = crate::content::frontmatter::replace_scalar(&new_fm, &edit.key, &edit.value);
            applied.push(format!("frontmatter:{}", edit.key));
        }
    }

    if let Some(ref new_h1) = plan.changes.h1 {
        // Replace the first `# ` line; insert at top if the body has no H1.
        let lines: Vec<String> = new_body.lines().map(|s| s.to_string()).collect();
        let mut new_lines = Vec::new();
        let mut replaced = false;
        for line in lines {
            if !replaced && line.trim_start().starts_with("# ") {
                new_lines.push(format!("# {}", new_h1));
                replaced = true;
            } else {
                new_lines.push(line);
            }
        }
        if !replaced {
            new_lines.insert(0, format!("# {}", new_h1));
        }
        new_body = new_lines.join("\n");
        applied.push("h1".to_string());
    }

    if let Some(ref new_intro) = plan.changes.intro {
        let body_before = new_body.clone();
        new_body = crate::content::cleaner::ensure_first_paragraph(&new_body, new_intro);
        if new_body != body_before {
            applied.push("intro".to_string());
        }
    }

    let new_content = crate::content::cleaner::rebuild_mdx(&new_fm, &new_body);

    if new_fm == fm && new_body == body {
        return StepResult {
            success: false,
            message: format!(
                "Plan produced no effective change to {} — refusing to report success. \
                 The planned values may already be present, or the plan was empty.",
                file_path.display()
            ),
            output: None,
        };
    }

    // Snapshot original
    let snapshot_path = file_path.with_extension("mdx.snapshot");
    let _ = std::fs::write(&snapshot_path, &original_content);

    // Write
    if let Err(e) = std::fs::write(&file_path, &new_content) {
        let _ = std::fs::remove_file(&snapshot_path);
        return StepResult {
            success: false,
            message: format!("Failed to write file: {}", e),
            output: None,
        };
    }

    // Validate structure; restore snapshot on corruption
    if let Err(e) = crate::content::cleaner::validate_mdx_structure(&new_content) {
        let _ = std::fs::rename(&snapshot_path, &file_path);
        return StepResult {
            success: false,
            message: format!(
                "Applied changes produced invalid MDX structure: {}. Original restored.",
                e
            ),
            output: None,
        };
    }

    let _ = std::fs::remove_file(&snapshot_path);

    StepResult {
        success: true,
        message: format!(
            "Applied {} change(s) to {}: {}",
            applied.len(),
            file_path.display(),
            applied.join(", ")
        ),
        output: Some(
            serde_json::json!({
                "file": file_path.to_string_lossy(),
                "applied": applied,
            })
            .to_string(),
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 4 (deterministic): verify
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministic step: re-read the file and confirm every planned change
/// actually landed. Fails loudly when the file is unchanged or a planned
/// value is missing — this is what makes silent success impossible.
pub(crate) fn exec_indexing_fix_verify(task: &Task, project_path: &str) -> StepResult {
    let plan = match resolve_plan(task, None) {
        Ok(p) => p,
        Err(result) => return result,
    };

    let file_path = match resolve_target_file(task, project_path) {
        Ok(p) => p,
        Err(result) => return result,
    };

    let content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to read {}: {}", file_path.display(), e),
                output: None,
            }
        }
    };

    if let Err(e) = crate::content::cleaner::validate_mdx_structure(&content) {
        return StepResult {
            success: false,
            message: format!("MDX structure invalid after fix: {}", e),
            output: None,
        };
    }

    let (fm, body) = match crate::content::frontmatter::split_mdx(&content) {
        Some((f, b)) => (f, b),
        None => {
            return StepResult {
                success: false,
                message: "Could not parse frontmatter from MDX file".to_string(),
                output: None,
            }
        }
    };

    let scalars = crate::content::frontmatter::top_level_scalars(fm);
    let get_scalar = |key: &str| -> String {
        scalars
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.raw_value.trim_matches('"').trim_matches('\'').to_string())
            .unwrap_or_default()
    };

    let mut verified: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    if let Some(ref expected) = plan.changes.title {
        let actual = get_scalar("title");
        if actual == expected.trim() {
            verified.push("title".to_string());
        } else {
            failed.push(format!(
                "title: expected {:?}, found {:?}",
                crate::engine::text::char_prefix(expected, 60),
                crate::engine::text::char_prefix(&actual, 60)
            ));
        }
    }

    if let Some(ref expected) = plan.changes.description {
        let actual = get_scalar("description");
        if actual == expected.trim() {
            verified.push("description".to_string());
        } else {
            failed.push(format!(
                "description: expected {:?}, found {:?}",
                crate::engine::text::char_prefix(expected, 60),
                crate::engine::text::char_prefix(&actual, 60)
            ));
        }
    }

    if let Some(ref edits) = plan.changes.frontmatter {
        for edit in edits {
            if edit.key == "title" || edit.key == "description" {
                continue; // covered above
            }
            let actual = get_scalar(&edit.key);
            if actual == edit.value.trim() {
                verified.push(format!("frontmatter:{}", edit.key));
            } else {
                failed.push(format!(
                    "frontmatter {}: expected {:?}, found {:?}",
                    edit.key, edit.value, actual
                ));
            }
        }
    }

    if let Some(ref expected) = plan.changes.h1 {
        let actual = extract_first_h1(&content).unwrap_or_default();
        if actual == expected.trim() {
            verified.push("h1".to_string());
        } else {
            failed.push(format!(
                "h1: expected {:?}, found {:?}",
                crate::engine::text::char_prefix(expected, 60),
                crate::engine::text::char_prefix(&actual, 60)
            ));
        }
    }

    if let Some(ref expected) = plan.changes.intro {
        let first_para = crate::content::cleaner::find_first_paragraph_range(body)
            .map(|(start, end)| body[start..end].trim().to_string())
            .unwrap_or_default();
        if normalize_ws(&first_para) == normalize_ws(expected) {
            verified.push("intro".to_string());
        } else {
            failed.push("intro: first paragraph does not match the planned intro".to_string());
        }
    }

    let report = serde_json::json!({
        "file": file_path.to_string_lossy(),
        "verified": verified,
        "failed": failed,
    });

    if !failed.is_empty() {
        return StepResult {
            success: false,
            message: format!(
                "Fix verification FAILED for {}: {}. The file was not changed as planned.",
                file_path.display(),
                failed.join("; ")
            ),
            output: Some(report.to_string()),
        };
    }

    if verified.is_empty() {
        return StepResult {
            success: false,
            message: format!(
                "Fix verification FAILED for {}: plan contained no verifiable changes.",
                file_path.display()
            ),
            output: Some(report.to_string()),
        };
    }

    StepResult {
        success: true,
        message: format!(
            "Verified {} change(s) landed in {}: {}",
            verified.len(),
            file_path.display(),
            verified.join(", ")
        ),
        output: Some(report.to_string()),
    }
}

// ─── Shared helpers ───────────────────────────────────────────────────────────

/// Load the IndexingFixPlan from the task artifact (preferred, persisted by
/// the executor from the generate step) or fall back to latest_raw.
fn resolve_plan(task: &Task, latest_raw: Option<&str>) -> Result<IndexingFixPlan, StepResult> {
    if let Some(artifact) = task.artifacts.iter().find(|a| a.key == "indexing_fix_plan") {
        if let Some(content) = &artifact.content {
            match serde_json::from_str::<IndexingFixPlan>(content) {
                Ok(p) => return Ok(p),
                Err(e) => {
                    return Err(StepResult {
                        success: false,
                        message: format!(
                            "indexing_fix_plan artifact exists but is invalid JSON: {}",
                            e
                        ),
                        output: Some(content.clone()),
                    })
                }
            }
        }
    }

    if let Some(raw) = latest_raw {
        if let Some(p) = crate::engine::text::extract_json_as::<IndexingFixPlan>(raw) {
            return Ok(p);
        }
    }

    Err(StepResult {
        success: false,
        message: "No indexing_fix_plan artifact or latest_raw found. \
             Run the generate step first."
            .to_string(),
        output: None,
    })
}

/// Re-resolve the target MDX file deterministically from the task description
/// URL (same logic as the context step). Never trust an agent-provided path.
fn resolve_target_file(task: &Task, project_path: &str) -> Result<PathBuf, StepResult> {
    let desc = parse_fix_task_description(task.description.as_deref().unwrap_or(""));
    if desc.url.is_empty() {
        return Err(StepResult {
            success: false,
            message: "Task description missing URL".to_string(),
            output: None,
        });
    }

    let paths = ProjectPaths::from_path(project_path);
    let content_dir = crate::content::locator::resolve(Path::new(project_path), None)
        .selected
        .unwrap_or_else(|| paths.repo_root.clone());

    let slug = crate::content::slug::extract_slug_from_url(&desc.url);
    match find_mdx_by_slug(&content_dir, &slug) {
        Some(p) => Ok(p),
        None => Err(StepResult {
            success: false,
            message: format!(
                "No MDX file found for {} (slug={}). Cannot apply indexing fix.",
                desc.url, slug
            ),
            output: None,
        }),
    }
}

/// Load cluster context from task artifacts (set by indexing_health_campaign)
/// and format it as a prompt block.
fn build_cluster_context_block(task: &Task) -> String {
    task.artifacts
        .iter()
        .find(|a| a.key == "indexing_target_context")
        .and_then(|a| a.content.as_ref())
        .and_then(|json| serde_json::from_str::<crate::models::indexing_health::IndexingTargetContext>(json).ok())
        .map(|ctx| {
            let siblings = match &ctx.cluster {
                Some(c) => serde_json::to_string_pretty(&c.siblings).unwrap_or_default(),
                None => "[]".to_string(),
            };
            format!(
                "\n\n## Cluster Context (from site-wide audit)\n\nThis page belongs to the '{}' cluster.\n\nSibling articles that may overlap topically:\n```json\n{}```\n\nShared headings detected in cluster: {:?}\n\nWhen planning changes, ensure the title, H1, and opening sections are DISTINCT from these siblings.",
                ctx.cluster.as_ref().map(|c| c.theme.clone()).unwrap_or_default(),
                siblings,
                ctx.cluster.as_ref().and_then(|c| c.shared_headings.clone()).unwrap_or_default()
            )
        })
        .unwrap_or_default()
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find_mdx_by_slug(content_dir: &Path, slug: &str) -> Option<std::path::PathBuf> {
    if slug.is_empty() {
        return None;
    }

    // Strip numeric prefix from URL segments too (e.g. "127_net_worth_tracker" → "net_worth_tracker")
    let last_segment = crate::content::slug::strip_numeric_prefix(
        slug.trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(slug),
    )
    .replace('_', "-");

    let full_slug_dashed = crate::content::slug::strip_numeric_prefix(slug.trim_end_matches('/'))
        .replace('/', "-")
        .replace('_', "-");

    let mut best_match: Option<std::path::PathBuf> = None;

    for entry in walkdir::WalkDir::new(content_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mdx") {
            continue;
        }

        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let stem_clean = crate::content::slug::strip_numeric_prefix(stem).replace('_', "-");

        // Exact stem match on last segment — highest confidence
        if stem_clean == last_segment {
            return Some(path.to_path_buf());
        }

        // Full slug match (for flat structures)
        if stem_clean == full_slug_dashed && best_match.is_none() {
            best_match = Some(path.to_path_buf());
        }

        // Also check if the relative path (without extension) matches the slug
        if let Ok(rel) = path.strip_prefix(content_dir) {
            let rel_str = rel
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            let rel_without_ext = rel_str.trim_end_matches(".mdx").trim_end_matches(".md");
            let rel_clean = crate::content::slug::strip_numeric_prefix(rel_without_ext)
                .replace('/', "-")
                .replace('_', "-");
            if rel_clean == full_slug_dashed {
                return Some(path.to_path_buf());
            }
        }
    }

    best_match
}

fn extract_first_h1(content: &str) -> Option<String> {
    for line in content.lines() {
        if line.trim_start().starts_with("# ") {
            return Some(line.trim_start_matches("# ").trim().to_string());
        }
    }
    None
}

fn extract_internal_links(content: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut seen = HashSet::new();

    // Simple markdown link extraction: [text](path)
    let re = regex::Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").unwrap();
    for cap in re.captures_iter(content) {
        let href = cap[2].to_string();
        if href.starts_with('/') && !href.starts_with("//") && !seen.contains(&href) {
            seen.insert(href.clone());
            links.push(href);
        }
    }
    links
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, TaskArtifact, TaskReviewSurface, TaskRunPolicy,
        TaskStatus,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
    }

    fn dummy_task(description: &str, artifacts: Vec<TaskArtifact>) -> Task {
        Task {
            id: "fix-task-1".to_string(),
            task_type: "fix_indexing".to_string(),
            phase: "implementation".to_string(),
            status: TaskStatus::InProgress,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::Required,
            title: Some("Fix indexing".to_string()),
            description: Some(description.to_string()),
            project_id: "proj-1".to_string(),
            depends_on: vec![],
            artifacts,
            run: crate::models::task::TaskRun::default(),
            not_before: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    const SAMPLE_MDX: &str = "---\ntitle: Old Title\ndescription: Old meta description here.\ndate: 2024-01-01\n---\n\n# Old Heading\n\nFirst paragraph of the article.\n\n## Section\n\nMore text.\n";

    fn write_sample_project(dir: &Path) -> PathBuf {
        // locator auto-discovers `content/` when it contains markdown
        let content_dir = dir.join("content");
        std::fs::create_dir_all(&content_dir).unwrap();
        let file = content_dir.join("test-article.mdx");
        std::fs::write(&file, SAMPLE_MDX).unwrap();
        file
    }

    fn plan_artifact(plan: &IndexingFixPlan) -> TaskArtifact {
        TaskArtifact {
            key: "indexing_fix_plan".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("indexing_fix_generate".to_string()),
            content: Some(serde_json::to_string_pretty(plan).unwrap()),
        }
    }

    // ─── Description parsing (Fix 2) ────────────────────────────────────────

    #[test]
    fn parse_campaign_rewrite_description() {
        let desc = "URL: https://example.com/blog/test-article\n\
                    Recommended action: rewrite_title_h1\n\
                    Reason: Shares H2s with sibling\n\
                    Parent campaign: camp-1\n\
                    Suggested title: Better Title\n\
                    Suggested H1: Better H1";
        let parsed = parse_fix_task_description(desc);
        assert_eq!(parsed.url, "https://example.com/blog/test-article");
        assert_eq!(parsed.recommended_action.as_deref(), Some("rewrite_title_h1"));
        assert_eq!(parsed.reason.as_deref(), Some("Shares H2s with sibling"));
        assert_eq!(parsed.suggested_title.as_deref(), Some("Better Title"));
        assert_eq!(parsed.suggested_h1.as_deref(), Some("Better H1"));
        assert_eq!(parsed.issue, None);
    }

    #[test]
    fn parse_diagnostics_description() {
        let desc = "URL: https://example.com/blog/test-article\n\
                    Issue: not_indexed_crawled\n\
                    Action: improve content\n\
                    Verdict: crawled_but_not_indexed";
        let parsed = parse_fix_task_description(desc);
        assert_eq!(parsed.url, "https://example.com/blog/test-article");
        assert_eq!(parsed.issue.as_deref(), Some("not_indexed_crawled"));
        assert_eq!(parsed.action.as_deref(), Some("improve content"));
        assert_eq!(parsed.verdict.as_deref(), Some("crawled_but_not_indexed"));
        assert_eq!(parsed.recommended_action, None);
    }

    #[test]
    fn parse_description_matches_any_line_not_fixed_index() {
        // "Recommended action:" on line 1 must not be misread as "Issue: ".
        let desc = "URL: https://example.com/blog/a\nRecommended action: rewrite_title_h1";
        let parsed = parse_fix_task_description(desc);
        assert_eq!(parsed.issue, None);
        assert_eq!(parsed.recommended_action.as_deref(), Some("rewrite_title_h1"));
    }

    #[test]
    fn context_step_emits_suggestions() {
        let dir = unique_temp_dir("ifix_ctx");
        write_sample_project(&dir);
        let task = dummy_task(
            "URL: https://example.com/blog/test-article\n\
             Recommended action: rewrite_title_h1\n\
             Reason: overlap\n\
             Suggested title: Better Title\n\
             Suggested H1: Better H1",
            vec![],
        );
        let result = exec_indexing_fix_context(&task, dir.to_str().unwrap());
        assert!(result.success, "context step failed: {}", result.message);
        let ctx: IndexingFixContext = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(ctx.recommended_action.as_deref(), Some("rewrite_title_h1"));
        assert_eq!(ctx.suggested_title.as_deref(), Some("Better Title"));
        assert_eq!(ctx.suggested_h1.as_deref(), Some("Better H1"));
        assert_eq!(ctx.title.as_deref(), Some("Old Title"));
        assert!(ctx.exists);
    }

    // ─── Apply step (deterministic write) ────────────────────────────────────

    #[test]
    fn apply_step_edits_file() {
        let dir = unique_temp_dir("ifix_apply");
        let file = write_sample_project(&dir);
        let plan = IndexingFixPlan {
            diagnosis: "overlap".to_string(),
            changes: IndexingFixChanges {
                title: Some("Better Title".to_string()),
                h1: Some("Better H1".to_string()),
                description: Some("A sharper meta description.".to_string()),
                intro: None,
                frontmatter: None,
            },
        };
        let task = dummy_task(
            "URL: https://example.com/blog/test-article\nRecommended action: rewrite_title_h1",
            vec![plan_artifact(&plan)],
        );

        let result = exec_indexing_fix_apply(&task, dir.to_str().unwrap(), None);
        assert!(result.success, "apply failed: {}", result.message);

        let updated = std::fs::read_to_string(&file).unwrap();
        assert!(updated.contains("Better Title"), "title not updated:\n{}", updated);
        assert!(updated.contains("# Better H1"), "H1 not updated:\n{}", updated);
        assert!(
            updated.contains("A sharper meta description."),
            "description not updated:\n{}",
            updated
        );
        // No leftover snapshot
        assert!(!file.with_extension("mdx.snapshot").exists());
    }

    #[test]
    fn apply_step_fails_loudly_when_plan_has_no_changes() {
        let dir = unique_temp_dir("ifix_noop");
        write_sample_project(&dir);
        let plan = IndexingFixPlan {
            diagnosis: "none".to_string(),
            changes: IndexingFixChanges::default(),
        };
        let task = dummy_task(
            "URL: https://example.com/blog/test-article",
            vec![plan_artifact(&plan)],
        );

        let result = exec_indexing_fix_apply(&task, dir.to_str().unwrap(), None);
        assert!(
            !result.success,
            "apply must fail loudly when the plan contains no changes"
        );
        assert!(result.message.contains("no changes"));
    }

    #[test]
    fn apply_step_fails_loudly_when_no_effective_change() {
        let dir = unique_temp_dir("ifix_noop2");
        // Fixture with quoted frontmatter values so replacing title with the
        // identical value produces byte-identical content.
        let content_dir = dir.join("content");
        std::fs::create_dir_all(&content_dir).unwrap();
        let file = content_dir.join("test-article.mdx");
        std::fs::write(
            &file,
            "---\ntitle: \"Old Title\"\ndescription: \"Old meta description here.\"\ndate: 2024-01-01\n---\n\n# Old Heading\n\nFirst paragraph of the article.\n",
        )
        .unwrap();
        let plan = IndexingFixPlan {
            diagnosis: "none".to_string(),
            changes: IndexingFixChanges {
                title: Some("Old Title".to_string()), // identical to current
                h1: None,
                description: None,
                intro: None,
                frontmatter: None,
            },
        };
        let task = dummy_task(
            "URL: https://example.com/blog/test-article",
            vec![plan_artifact(&plan)],
        );

        let result = exec_indexing_fix_apply(&task, dir.to_str().unwrap(), None);
        assert!(
            !result.success,
            "apply must fail loudly when the plan changes nothing"
        );
        assert!(result.message.contains("no effective change"));
    }

    // ─── Verify step ─────────────────────────────────────────────────────────

    #[test]
    fn verify_step_fails_when_file_unchanged() {
        let dir = unique_temp_dir("ifix_verify_fail");
        write_sample_project(&dir);
        // Plan demands a new title, but the file was never edited.
        let plan = IndexingFixPlan {
            diagnosis: "overlap".to_string(),
            changes: IndexingFixChanges {
                title: Some("Better Title".to_string()),
                h1: Some("Better H1".to_string()),
                description: None,
                intro: None,
                frontmatter: None,
            },
        };
        let task = dummy_task(
            "URL: https://example.com/blog/test-article",
            vec![plan_artifact(&plan)],
        );

        let result = exec_indexing_fix_verify(&task, dir.to_str().unwrap());
        assert!(
            !result.success,
            "verify must fail loudly when the planned change never landed"
        );
        assert!(result.message.contains("verification FAILED"));
    }

    #[test]
    fn verify_step_passes_after_apply() {
        let dir = unique_temp_dir("ifix_verify_ok");
        write_sample_project(&dir);
        let plan = IndexingFixPlan {
            diagnosis: "overlap".to_string(),
            changes: IndexingFixChanges {
                title: Some("Better Title".to_string()),
                h1: Some("Better H1".to_string()),
                description: Some("A sharper meta description.".to_string()),
                intro: None,
                frontmatter: None,
            },
        };
        let task = dummy_task(
            "URL: https://example.com/blog/test-article",
            vec![plan_artifact(&plan)],
        );

        let applied = exec_indexing_fix_apply(&task, dir.to_str().unwrap(), None);
        assert!(applied.success, "apply failed: {}", applied.message);

        let verified = exec_indexing_fix_verify(&task, dir.to_str().unwrap());
        assert!(verified.success, "verify failed: {}", verified.message);
    }
}
