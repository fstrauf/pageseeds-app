//! Step 3 (deterministic): apply the plan with snapshot/restore.

use crate::engine::workflows::StepResult;
use crate::models::task::Task;

use super::{resolve_plan, resolve_target_file};

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
        return StepResult::fail("indexing_fix_plan contains no changes — refusing to report success \
                 without any edit."
                .to_string());
    }

    let file_path = match resolve_target_file(task, project_path) {
        Ok(p) => p,
        Err(result) => return result,
    };

    let original_content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult::fail(format!("Failed to read {}: {}", file_path.display(), e))
        }
    };

    let (fm, body) = match crate::content::frontmatter::split_mdx(&original_content) {
        Some((f, b)) => (f.to_string(), b.to_string()),
        None => {
            return StepResult::fail("Could not parse frontmatter from MDX file".to_string())
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
        return StepResult::fail(format!(
                "Plan produced no effective change to {} — refusing to report success. \
                 The planned values may already be present, or the plan was empty.",
                file_path.display()
            ));
    }

    // Snapshot original
    let snapshot_path = file_path.with_extension("mdx.snapshot");
    let _ = std::fs::write(&snapshot_path, &original_content);

    // Write
    if let Err(e) = std::fs::write(&file_path, &new_content) {
        let _ = std::fs::remove_file(&snapshot_path);
        return StepResult::fail(format!("Failed to write file: {}", e));
    }

    // Validate structure; restore snapshot on corruption
    if let Err(e) = crate::content::cleaner::validate_mdx_structure(&new_content) {
        let _ = std::fs::rename(&snapshot_path, &file_path);
        return StepResult::fail(format!(
                "Applied changes produced invalid MDX structure: {}. Original restored.",
                e
            ));
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
        artifact_key: None,
    }
}
