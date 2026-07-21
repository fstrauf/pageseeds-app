use std::path::{Path, PathBuf};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::merge_patch::{
    ContentMergePatch, ExtractedExample, ExtractedFaq, ExtractedHeading, ExtractedTable,
    MergePreflightReport, MergeValidationReport, RedirectRule, SectionInventory,
};
use crate::models::task::Task;

use super::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Step 5: Apply Patch
// ═══════════════════════════════════════════════════════════════════════════════

/// Apply one or more ContentMergePatches to the keeper file.
///
/// Safety contract (mirrors the canonical fix pipeline in
/// `engine/exec/content/fix_apply.rs`):
///   1. All patch rounds are accumulated against a single in-memory string —
///      nothing touches the disk between rounds, so a failing round can never
///      leave a half-applied merge behind.
///   2. The original keeper is snapshotted to `keeper.mdx.snapshot` before any write.
///   3. The fully-accumulated content is validated in memory BEFORE the file is
///      touched; any failure leaves the keeper byte-identical.
///   4. On write failure the snapshot is restored; on success it is removed.
///
/// When `merge_draft_patch` ran multiple batch rounds, `patch_json` is
/// `{"patches": [...]}`; a single round is a bare ContentMergePatch object.
pub(crate) fn exec_merge_apply_patch(
    _task: &Task,
    project_path: &str,
    patch_json: &str,
) -> StepResult {
    let patches: Vec<ContentMergePatch> = match parse_patches(patch_json) {
        Ok(p) => p,
        Err(e) => {
            return StepResult::fail(format!("Invalid ContentMergePatch JSON: {}", e));
        }
    };

    let total_additions: usize = patches.iter().map(|p| p.additions.len()).sum();
    let total_transitions: usize = patches.iter().map(|p| p.transitions.len()).sum();

    let Some(first_patch) = patches.first() else {
        return StepResult::fail("Merge patch contains no rounds to apply.".to_string());
    };

    let keeper_path = Path::new(&first_patch.keeper_file);
    let keeper_path = if keeper_path.is_absolute() {
        keeper_path.to_path_buf()
    } else {
        Path::new(project_path).join(keeper_path)
    };

    if !keeper_path.exists() {
        return StepResult::fail(format!("Keeper file not found: {}", keeper_path.display()));
    }

    let original = match std::fs::read_to_string(&keeper_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult::fail(format!("Failed to read keeper file: {}", e));
        }
    };

    // Accumulate every round against one in-memory string — the disk is
    // touched exactly once, after all rounds have been applied and validated.
    let mut modified = original.clone();
    for patch in &patches {
        apply_patch_in_memory(&mut modified, patch);
    }

    // Snapshot the original before touching the file on disk.
    let snapshot_path = keeper_path.with_extension("mdx.snapshot");
    if let Err(e) = std::fs::write(&snapshot_path, &original) {
        return StepResult::fail(format!("Failed to write keeper snapshot: {}", e));
    }

    // Validate the accumulated content in memory FIRST — a corrupt patch must
    // never reach the live keeper file.
    if let Err(e) = crate::content::cleaner::validate_mdx_structure(&modified) {
        let _ = std::fs::remove_file(&snapshot_path);
        return StepResult::fail(format!(
                "Patch produced invalid MDX structure: {}. Original keeper left untouched.",
                e
            ));
    }

    // Write modified file; restore the snapshot if the write fails.
    if let Err(e) = std::fs::write(&keeper_path, &modified) {
        let _ = std::fs::rename(&snapshot_path, &keeper_path);
        return StepResult::fail(format!(
                "Failed to write modified keeper: {}. Original restored from snapshot.",
                e
            ));
    }

    // Clean up snapshot on success.
    let _ = std::fs::remove_file(&snapshot_path);

    let word_count = crate::content::ops::count_words(&modified);

    StepResult {
        success: true,
        message: format!(
            "Patch applied: {} round(s), {} additions, {} transitions, {} words",
            patches.len(), total_additions, total_transitions, word_count,
        ),
        output: Some(
            serde_json::json!({
                "keeper_file": keeper_path.to_string_lossy(),
                "rounds_applied": patches.len(),
                "word_count": word_count,
                "validation_valid": true,
            })
            .to_string(),
        ),
    }
}

/// Parse either a bare ContentMergePatch or a `{"patches": [...]}` wrapper.
fn parse_patches(patch_json: &str) -> std::result::Result<Vec<ContentMergePatch>, String> {
    let value: serde_json::Value =
        serde_json::from_str(patch_json).map_err(|e| e.to_string())?;
    if let Some(arr) = value["patches"].as_array() {
        return arr
            .iter()
            .map(|p| {
                serde_json::from_value::<ContentMergePatch>(p.clone()).map_err(|e| e.to_string())
            })
            .collect();
    }
    serde_json::from_value::<ContentMergePatch>(value)
        .map(|p| vec![p])
        .map_err(|e| e.to_string())
}

/// Apply one patch round to the in-memory content string.
fn apply_patch_in_memory(modified: &mut String, patch: &ContentMergePatch) {
    // Apply transitions first — first occurrence only, so a repeated phrase
    // elsewhere in the article is never rewritten by accident.
    for transition in &patch.transitions {
        *modified = modified.replacen(&transition.find, &transition.replace, 1);
    }

    // Apply additions
    for addition in &patch.additions {
        let section_text = format!("\n\n## {}\n\n{}", addition.heading, addition.content);

        match addition.position.as_str() {
            pos if pos.starts_with("after:") => {
                let target = pos.strip_prefix("after:").unwrap_or("").trim();
                let pattern = format!("## {}", target);
                if let Some(idx) = modified.find(&pattern) {
                    // Find end of that section (next ## or EOF)
                    let rest = &modified[idx + pattern.len()..];
                    let next_heading = rest.find("\n## ").unwrap_or(rest.len());
                    let insert_pos = idx + pattern.len() + next_heading;
                    modified.insert_str(insert_pos, &section_text);
                } else {
                    modified.push_str(&section_text);
                }
            }
            pos if pos.starts_with("before:") => {
                let target = pos.strip_prefix("before:").unwrap_or("").trim();
                let pattern = format!("## {}", target);
                if let Some(idx) = modified.find(&pattern) {
                    modified.insert_str(idx, &section_text);
                } else {
                    modified.push_str(&section_text);
                }
            }
            _ => {
                // Default: append at end of body (before any Related Articles section if present)
                if let Some(idx) = modified.to_lowercase().find("\n## related articles") {
                    modified.insert_str(idx, &section_text);
                } else {
                    modified.push_str(&section_text);
                }
            }
        }
    }
}
