/// Deterministic application of agent-generated content fix patch.
///
/// 1. Parse ContentFixPatch from content_fix_patch artifact (preferred) or latest_raw (legacy)
/// 2. Resolve absolute file path from project_path + patch.file
/// 3. Read original file content
/// 4. Apply changes deterministically
/// 5. rebuild_mdx → write file
/// 6. validate_mdx_structure → if fail, restore snapshot, return failed
/// 7. Return success with summary
use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::models::content_review::{ContentFixChanges, ContentFixPatch};
use crate::models::task::Task;

pub(crate) fn exec_fix_content_article_apply(
    task: &Task,
    project_path: &str,
    latest_raw: Option<&str>,
) -> StepResult {
    let mut patch = match resolve_patch(task, latest_raw) {
        Ok(p) => p,
        Err(result) => return result,
    };

    if let Some(error) = patch.error.as_ref() {
        return StepResult {
            success: false,
            message: format!("Agent reported error: {}", error),
            output: None,
        };
    }

    let repo_root = Path::new(project_path);
    let file_path =
        match crate::engine::exec::audit_health::resolve_content_file(repo_root, &patch.file) {
            Some(p) => p,
            None => {
                return StepResult {
                    success: false,
                    message: format!(
                        "File not found: {}. Run sanitize_content to repair paths.",
                        patch.file
                    ),
                    output: None,
                };
            }
        };

    let original_content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(_e) => {
            return StepResult {
                success: false,
                message: format!("File not found: {}", file_path.display()),
                output: None,
            };
        }
    };

    let (fm, body) = match crate::content::frontmatter::split_mdx(&original_content) {
        Some((f, b)) => (f.to_string(), b.to_string()),
        None => {
            return StepResult {
                success: false,
                message: "Could not parse frontmatter from MDX file".to_string(),
                output: None,
            };
        }
    };

    let ContentFixChanges {
        title,
        h1,
        description,
        intro,
        internal_links,
        faq_questions,
        eeat_signal,
        cta,
    } = patch.changes;

    let mut new_fm = fm;
    let mut new_body = body;
    let mut applied = Vec::new();

    if let Some(new_title) = title {
        new_fm = crate::content::frontmatter::replace_scalar(&new_fm, "title", &new_title);
        applied.push("title".to_string());
    }

    if let Some(new_desc) = description {
        new_fm = crate::content::frontmatter::replace_scalar(&new_fm, "description", &new_desc);
        applied.push("description".to_string());
    }

    if let Some(new_h1) = h1 {
        // Simple H1 replacement: find first line starting with "# " and replace it.
        // If no H1 exists (template-based themes use frontmatter title), insert at top.
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
            // No H1 found — insert at the beginning of the body
            new_lines.insert(0, format!("# {}", new_h1));
        }
        new_body = new_lines.join("\n");
        applied.push("h1".to_string());
    }

    if let Some(new_intro) = intro {
        let body_before = new_body.clone();
        new_body = crate::content::cleaner::ensure_first_paragraph(&new_body, &new_intro);
        if new_body != body_before {
            applied.push("intro".to_string());
        } else {
            log::warn!(
                "[fix_content_article_apply] intro patch did not change body for {}",
                patch.file
            );
        }
    }

    if let Some(links) = internal_links {
        let link_count = links.len();
        for link in links {
            let blog_link = crate::content::slug::format_blog_link(&link.target_slug);
            let anchor = format!("[{}]({})", link.anchor_text, blog_link);
            // Simple append-at-end strategy for now; could be smarter
            new_body.push_str(&format!("\n\n{}", anchor));
        }
        applied.push(format!("internal_links ({})", link_count));
    }

    if let Some(faqs) = faq_questions {
        let faq_pairs: Vec<(String, String)> = faqs
            .iter()
            .map(|f| (f.question.clone(), f.answer.clone()))
            .collect();
        new_fm = crate::content::frontmatter::replace_faq_block(&new_fm, &faq_pairs);
        applied.push(format!("faq ({} questions)", faqs.len()));
    }

    if let Some(eeat) = eeat_signal {
        // Append EEAT signal as a small blockquote at the end
        new_body.push_str(&format!("\n\n> **Expertise:** {}\n", eeat));
        applied.push("eeat".to_string());
    }

    if let Some(new_cta) = cta {
        new_body.push_str(&format!("\n\n---\n\n{}", new_cta));
        applied.push("cta".to_string());
    }

    // Rebuild MDX
    let new_content = crate::content::cleaner::rebuild_mdx(&new_fm, &new_body);

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

    // Validate
    if let Err(e) = crate::content::cleaner::validate_mdx_structure(&new_content) {
        // Restore snapshot
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

    // Clean up snapshot on success
    let _ = std::fs::remove_file(&snapshot_path);

    // Update last_edited_at in articles table
    if let Ok(db) = rusqlite::Connection::open(crate::db::default_db_path()) {
        let now = chrono::Utc::now().to_rfc3339();
        // Try to resolve article_id from the file path
        let article_id = task.artifacts.iter().find_map(|a| {
            a.content.as_deref().and_then(|c| {
                serde_json::from_str::<serde_json::Value>(c).ok()
                    .and_then(|v| v["article_id"].as_i64())
            })
        });
        if let Some(id) = article_id {
            let _ = db.execute(
                "UPDATE articles SET last_edited_at = ?1 WHERE id = ?2 AND project_id = ?3",
                rusqlite::params![&now, id, &task.project_id],
            );
        }
    }

    let summary = if applied.is_empty() {
        "No changes applied — patch was empty or already satisfied".to_string()
    } else {
        format!(
            "Applied {} change(s) to {}: {}",
            applied.len(),
            patch.file,
            applied.join(", ")
        )
    };

    StepResult {
        success: true,
        message: summary,
        output: Some(
            serde_json::json!({
                "file": patch.file,
                "applied": applied,
            })
            .to_string(),
        ),
    }
}

// ─── Patch resolution ─────────────────────────────────────────────────────────

fn resolve_patch(task: &Task, latest_raw: Option<&str>) -> Result<ContentFixPatch, StepResult> {
    // Prefer artifact
    if let Some(artifact) = task.artifacts.iter().find(|a| a.key == "content_fix_patch") {
        if let Some(content) = &artifact.content {
            match serde_json::from_str::<ContentFixPatch>(content) {
                Ok(p) => return Ok(p),
                Err(e) => {
                    return Err(StepResult {
                        success: false,
                        message: format!(
                            "content_fix_patch artifact exists but is invalid JSON: {}",
                            e
                        ),
                        output: Some(content.clone()),
                    });
                }
            }
        }
    }

    // Fallback to latest_raw (legacy / direct mode)
    if let Some(raw) = latest_raw {
        match serde_json::from_str::<ContentFixPatch>(raw) {
            Ok(p) => return Ok(p),
            Err(e) => {
                return Err(StepResult {
                    success: false,
                    message: format!(
                        "latest_raw is not a valid ContentFixPatch JSON: {}",
                        e
                    ),
                    output: Some(raw.to_string()),
                });
            }
        }
    }

    Err(StepResult {
        success: false,
        message: "No content_fix_patch artifact or latest_raw found. \
             Run the generate step first."
            .to_string(),
        output: None,
    })
}
