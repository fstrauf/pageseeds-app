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

/// Apply a ContentMergePatch to the keeper file, snapshotting the original.
pub(crate) fn exec_merge_apply_patch(
    _task: &Task,
    project_path: &str,
    patch_json: &str,
) -> StepResult {
    let patch: ContentMergePatch = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid ContentMergePatch JSON: {}", e),
                output: None,
            };
        }
    };

    let keeper_path = Path::new(&patch.keeper_file);
    let keeper_path = if keeper_path.is_absolute() {
        keeper_path.to_path_buf()
    } else {
        Path::new(project_path).join(keeper_path)
    };

    if !keeper_path.exists() {
        return StepResult {
            success: false,
            message: format!("Keeper file not found: {}", keeper_path.display()),
            output: None,
        };
    }

    let original = match std::fs::read_to_string(&keeper_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to read keeper file: {}", e),
                output: None,
            };
        }
    };

    // Apply patch
    let mut modified = original.clone();

    // Apply transitions first
    for transition in &patch.transitions {
        modified = modified.replace(&transition.find, &transition.replace);
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

    // Write modified file
    if let Err(e) = std::fs::write(&keeper_path, &modified) {
        return StepResult {
            success: false,
            message: format!("Failed to write modified keeper: {}", e),
            output: None,
        };
    }

    // Validate MDX structure
    let validation = crate::content::cleaner::validate_mdx_structure(&modified);

    let word_count = crate::content::ops::count_words(&modified);

    StepResult {
        success: validation.is_ok(),
        message: format!(
            "Patch applied: {} additions, {} transitions, {} words",
            patch.additions.len(),
            patch.transitions.len(),
            word_count,
        ),
        output: Some(
            serde_json::json!({
                "keeper_file": keeper_path.to_string_lossy().to_string(),
                "word_count": word_count,
                "validation_valid": validation.is_ok(),
                "validation_issues": validation.err().map(|e| vec![e]).unwrap_or_default(),
            })
            .to_string(),
        ),
    }
}

