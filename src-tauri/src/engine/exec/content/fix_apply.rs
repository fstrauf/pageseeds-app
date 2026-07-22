/// Deterministic application of agent-generated content fix patch.
///
/// 1. Parse ContentFixPatch from content_fix_patch artifact (preferred) or latest_raw (legacy)
/// 2. Pin identity from content_fix_context when present (defense-in-depth vs model paths)
/// 3. Resolve absolute file path from project_path + patch.file
/// 4. Read original file content
/// 5. Apply changes deterministically
/// 6. rebuild_mdx → write file
/// 7. validate_mdx_structure → if fail, restore snapshot, return failed
/// 8. Empty apply with open unsatisfied suggestions → fail (not soft success)
use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::models::content_review::{ContentFixChanges, ContentFixPatch, ReviewSuggestion};
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
        return StepResult::fail(format!("Agent reported error: {}", error));
    }

    // Defense-in-depth: prefer context path over any model-hallucinated path.
    pin_patch_from_context_artifact(task, &mut patch);

    let repo_root = Path::new(project_path);
    let file_path =
        match crate::engine::exec::audit_health::resolve_content_file(repo_root, &patch.file) {
            Some(p) => p,
            None => {
                return StepResult::fail(format!(
                        "File not found: {}. Run sanitize_content to repair paths.",
                        patch.file
                    ));
            }
        };

    let original_content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(_e) => {
            return StepResult::fail(format!("File not found: {}", file_path.display()));
        }
    };

    let (fm, body) = match crate::content::frontmatter::split_mdx(&original_content) {
        Some((f, b)) => (f.to_string(), b.to_string()),
        None => {
            return StepResult::fail("Could not parse frontmatter from MDX file".to_string());
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
        return StepResult::fail(format!("Failed to write file: {}", e));
    }

    // Validate
    if let Err(e) = crate::content::cleaner::validate_mdx_structure(&new_content) {
        // Restore snapshot
        let _ = std::fs::rename(&snapshot_path, &file_path);
        return StepResult::fail(format!(
                "Applied changes produced invalid MDX structure: {}. Original restored.",
                e
            ));
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

    if applied.is_empty() {
        let suggestions = suggestions_from_context_artifact(task);
        let unsatisfied = super::fix_suggestion_coverage::unsatisfied_suggestion_fields(
            &suggestions,
            None,
            &original_content,
        );
        if !unsatisfied.is_empty() {
            return StepResult::fail(
                super::fix_suggestion_coverage::empty_apply_unsatisfied_message(&unsatisfied),
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
        artifact_key: None,
    }
}

// ─── Context identity pin (defense-in-depth) ──────────────────────────────────

/// Overwrite `patch.file` / `article_id` from `content_fix_context` when present.
fn pin_patch_from_context_artifact(task: &Task, patch: &mut ContentFixPatch) {
    let context_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "content_fix_context")
        .and_then(|a| a.content.as_deref())
        .unwrap_or("");
    if context_json.is_empty() {
        return;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(context_json) else {
        return;
    };
    if let Some(file) = value["article_file"].as_str() {
        if !file.is_empty() {
            patch.file = file.to_string();
        }
    }
    if let Some(id) = value["article_id"].as_i64() {
        // Keep model id only when context has no usable id.
        if id != 0 {
            patch.article_id = id;
        }
    }
}

fn suggestions_from_context_artifact(task: &Task) -> Vec<ReviewSuggestion> {
    let context_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "content_fix_context")
        .and_then(|a| a.content.as_deref())
        .unwrap_or("");
    if context_json.is_empty() {
        return Vec::new();
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(context_json) else {
        return Vec::new();
    };
    value["suggestions"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|s| serde_json::from_value(s.clone()).ok())
        .collect()
}

// ─── Patch resolution ─────────────────────────────────────────────────────────

fn resolve_patch(task: &Task, latest_raw: Option<&str>) -> Result<ContentFixPatch, StepResult> {
    // Prefer artifact
    if let Some(artifact) = task.artifacts.iter().find(|a| a.key == "content_fix_patch") {
        if let Some(content) = &artifact.content {
            match serde_json::from_str::<ContentFixPatch>(content) {
                Ok(p) => return Ok(p),
                Err(e) => {
                    return Err(StepResult::fail_with_output(format!(
                            "content_fix_patch artifact exists but is invalid JSON: {}",
                            e
                        ), content.clone()));
                }
            }
        }
    }

    // Fallback to latest_raw (legacy / direct mode)
    if let Some(raw) = latest_raw {
        match serde_json::from_str::<ContentFixPatch>(raw) {
            Ok(p) => return Ok(p),
            Err(e) => {
                return Err(StepResult::fail_with_output(format!(
                        "latest_raw is not a valid ContentFixPatch JSON: {}",
                        e
                    ), raw.to_string()));
            }
        }
    }

    Err(StepResult::fail("No content_fix_patch artifact or latest_raw found. \
             Run the generate step first."
            .to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::content_review::ContentFixChanges;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, TaskArtifact, TaskRun, TaskReviewSurface,
        TaskRunPolicy, TaskStatus,
    };
    use std::fs;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn sample_task(artifacts: Vec<TaskArtifact>) -> Task {
        let now = chrono::Utc::now().to_rfc3339();
        Task {
            id: "task-fix-apply".to_string(),
            project_id: "p1".to_string(),
            task_type: "fix_content_article".to_string(),
            phase: "fix".to_string(),
            status: TaskStatus::InProgress,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::None,
            title: Some("Fix article".to_string()),
            description: None,
            depends_on: vec![],
            artifacts,
            run: TaskRun::default(),
            created_at: now.clone(),
            not_before: None,
            updated_at: now,
        }
    }

    #[test]
    fn pin_from_context_overwrites_hallucinated_path() {
        let task = sample_task(vec![TaskArtifact {
            key: "content_fix_context".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("fix_content_article".to_string()),
            content: Some(
                serde_json::json!({
                    "article_id": 42,
                    "article_file": "content/blog/real-slug.mdx",
                    "suggestions": []
                })
                .to_string(),
            ),
        }]);
        let mut patch = ContentFixPatch {
            article_id: 1,
            file: "content/001_article.mdx".to_string(),
            error: None,
            changes: ContentFixChanges::default(),
        };
        pin_patch_from_context_artifact(&task, &mut patch);
        assert_eq!(patch.file, "content/blog/real-slug.mdx");
        assert_eq!(patch.article_id, 42);
    }

    #[test]
    fn empty_apply_fails_when_open_suggestions_unsatisfied() {
        let dir = std::env::temp_dir().join(format!("pageseeds-fix-apply-{}", Uuid::new_v4()));
        fs::create_dir_all(dir.join("content/blog")).unwrap();
        let rel = "content/blog/slug.mdx";
        let abs: PathBuf = dir.join(rel);
        // Short meta + short intro → description/intro fail health.
        fs::write(
            &abs,
            "---\ntitle: \"Ok Title\"\ndescription: \"Short.\"\ndate: \"2026-01-01\"\n---\n\n# Ok Title\n\nTiny intro.\n",
        )
        .unwrap();

        let patch = ContentFixPatch {
            article_id: 7,
            file: "content/001_article.mdx".to_string(), // wrong path — should be pinned
            error: None,
            changes: ContentFixChanges::default(),
        };
        let task = sample_task(vec![
            TaskArtifact {
                key: "content_fix_context".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("fix_content_article".to_string()),
                content: Some(
                    serde_json::json!({
                        "article_id": 7,
                        "article_file": rel,
                        "suggestions": [
                            {
                                "category": "description",
                                "current": "Short.",
                                "proposed": "A much longer meta description that meets SEO length.",
                                "reason": "meta too short"
                            },
                            {
                                "category": "title",
                                "current": "Ok Title",
                                "proposed": "Better Title",
                                "reason": "prefer keyword"
                            }
                        ]
                    })
                    .to_string(),
                ),
            },
            TaskArtifact {
                key: "content_fix_patch".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("fix_content_article".to_string()),
                content: Some(serde_json::to_string(&patch).unwrap()),
            },
        ]);

        let result = exec_fix_content_article_apply(&task, dir.to_str().unwrap(), None);
        let _ = fs::remove_dir_all(&dir);

        assert!(
            !result.success,
            "expected fail on empty apply with open suggestions, got: {}",
            result.message
        );
        assert!(
            result.message.contains("remain unsatisfied")
                || result.message.contains("Empty/no-op"),
            "unexpected message: {}",
            result.message
        );
        // Title is healthy in file → only description should be called out, but either is fine.
        assert!(
            result.message.contains("description") || result.message.contains("title"),
            "message should name fields: {}",
            result.message
        );
    }

    #[test]
    fn empty_apply_soft_success_when_suggestions_already_healthy() {
        let dir = std::env::temp_dir().join(format!("pageseeds-fix-apply-ok-{}", Uuid::new_v4()));
        fs::create_dir_all(dir.join("content/blog")).unwrap();
        let rel = "content/blog/slug.mdx";
        let abs: PathBuf = dir.join(rel);
        // Title within limit — only title suggestion, already healthy.
        fs::write(
            &abs,
            "---\ntitle: \"Ok Title\"\ndescription: \"Short.\"\ndate: \"2026-01-01\"\n---\n\n# Ok Title\n\nTiny intro.\n",
        )
        .unwrap();

        let patch = ContentFixPatch {
            article_id: 7,
            file: rel.to_string(),
            error: None,
            changes: ContentFixChanges::default(),
        };
        let task = sample_task(vec![
            TaskArtifact {
                key: "content_fix_context".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("fix_content_article".to_string()),
                content: Some(
                    serde_json::json!({
                        "article_id": 7,
                        "article_file": rel,
                        "suggestions": [{
                            "category": "title",
                            "current": "Ok Title",
                            "proposed": "Still Ok",
                            "reason": "optional"
                        }]
                    })
                    .to_string(),
                ),
            },
            TaskArtifact {
                key: "content_fix_patch".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("fix_content_article".to_string()),
                content: Some(serde_json::to_string(&patch).unwrap()),
            },
        ]);

        let result = exec_fix_content_article_apply(&task, dir.to_str().unwrap(), None);
        let _ = fs::remove_dir_all(&dir);

        assert!(
            result.success,
            "empty apply should soft-succeed when open fields are already healthy: {}",
            result.message
        );
    }
}
