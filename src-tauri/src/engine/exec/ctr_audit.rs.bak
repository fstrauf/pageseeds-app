/// CTR (Click-Through Rate) audit execution module.
///
/// Covers:
///   - exec_ctr_build_context   (deterministic data collection + clicks_lost scoring)
///   - exec_ctr_analyze         (agentic analysis with ctr-optimization skill)
///   - create_ctr_fix_tasks     (spawn follow-up fix tasks)

use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::engine::{agent, skills};
use crate::engine::spawner::{TaskSpawner, TaskSpec};
use crate::models::task::{ExecutionMode, Task, TaskArtifact};

/// Load a skill from the project repo, falling back to app-level default skills.
fn load_skill_with_fallback(repo_root: &Path, skill_name: &str) -> Option<crate::engine::skills::Skill> {
    // 1. Try project-level skill first
    if let Some(skill) = skills::load_skill(repo_root, skill_name) {
        return Some(skill);
    }
    // 2. Fall back to app-level default skills (for dev mode)
    // CARGO_MANIFEST_DIR points to src-tauri/ during compilation
    let app_skills_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or(Path::new("."))
        .join(".github")
        .join("skills")
        .join(skill_name);
    if app_skills_dir.exists() {
        let skill_md = app_skills_dir.join("SKILL.md");
        if let Ok(content) = std::fs::read_to_string(&skill_md) {
            return Some(crate::engine::skills::Skill {
                name: skill_name.to_string(),
                skill_dir: format!(".github/skills/{}", skill_name),
                description: skill_name.to_string(),
                content,
            });
        }
    }
    None
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 1: Build Context
// ═══════════════════════════════════════════════════════════════════════════════

/// Build the CTR audit context by reading articles.json, extracting excerpts,
/// computing clicks_lost per article, and returning structured JSON.
///
/// Uses persistent `article_audit_state` to skip articles that were healthy on the
/// last audit AND have not changed since. This prevents re-flagging already-fixed
/// issues across repeated audit runs.
pub(crate) fn exec_ctr_build_context(
    task: &Task,
    project_path: &str,
    conn: &Connection,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let articles_path = paths.automation_dir.join("articles.json");

    // ── Step 0: Clean stale entries from articles.json ───────────────────────
    // The filesystem is the source of truth. Remove entries whose files no longer exist.
    let mut cleaned_summary = Vec::new();
    match crate::content::ops::clean_stale_articles_json(&paths.automation_dir, Path::new(project_path)) {
        Ok(removed) => {
            if !removed.is_empty() {
                log::info!(
                    "[ctr_audit] Removed {} stale entries from articles.json: {:?}",
                    removed.len(),
                    removed
                );
                cleaned_summary = removed;
            }
        }
        Err(e) => {
            log::warn!("[ctr_audit] Failed to clean stale articles.json entries: {}", e);
        }
    }

    let raw = match std::fs::read_to_string(&articles_path) {
        Ok(s) => s,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("articles.json not found: {}", e),
                output: None,
            };
        }
    };

    let doc: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to parse articles.json: {}", e),
                output: None,
            };
        }
    };

    let empty = vec![];
    let articles = doc["articles"].as_array().unwrap_or(&empty);

    let mut article_records: Vec<serde_json::Value> = Vec::new();
    let mut skipped_healthy = 0usize;
    let mut skipped_unchanged = 0usize;

    for article in articles.iter() {
        let id = article["id"].as_i64().unwrap_or(0);
        let url_slug = article["url_slug"].as_str().unwrap_or("").to_string();
        let target_keyword = article["target_keyword"].as_str().unwrap_or("").to_string();
        let file_ref = article["file"].as_str().unwrap_or("").to_string();

        let gsc = &article["gsc"];
        let impressions = gsc["impressions"].as_f64().unwrap_or(0.0);
        let clicks = gsc["clicks"].as_f64().unwrap_or(0.0);
        let ctr = gsc["ctr"].as_f64().unwrap_or(0.0);
        let avg_position = gsc["avg_position"].as_f64().unwrap_or(0.0);

        // Extract current MDX state
        let (current_title, meta_description, first_paragraph, h1, has_faq_schema, file_found) =
            crate::engine::exec::audit_health::read_article_excerpt(project_path, &file_ref);

        // Compute content hash for change detection
        let content_hash = crate::engine::exec::audit_health::compute_content_hash(
            &current_title,
            &meta_description,
            &first_paragraph,
        );

        // Check stored audit state: if hash matches and was healthy, skip
        if let Ok(Some(stored)) = crate::db::get_article_audit_state(
            conn,
            &task.project_id,
            &file_ref,
            "ctr_audit",
        ) {
            if stored.content_hash == content_hash && stored.was_healthy {
                skipped_unchanged += 1;
                continue;
            }
        }

        // Run deterministic health checks
        let health = crate::engine::exec::audit_health::check_article_health(
            &current_title,
            &meta_description,
            &first_paragraph,
            &target_keyword,
            has_faq_schema,
            file_found,
        );

        // Persist the audit state immediately (healthy or not)
        let _ = crate::db::set_article_audit_state(
            conn,
            &task.project_id,
            &file_ref,
            "ctr_audit",
            health.all_ok(),
            &content_hash,
            &health.issues,
        );

        if health.all_ok() {
            skipped_healthy += 1;
            continue;
        }

        // Compute clicks_lost: impressions * max(0, 0.005 - actual_ctr)
        let clicks_lost = impressions * (0.005_f64 - ctr).max(0.0);

        article_records.push(serde_json::json!({
            "id": id,
            "url_slug": url_slug,
            "title": current_title,
            "target_keyword": target_keyword,
            "meta_description": meta_description,
            "first_paragraph": first_paragraph,
            "h1": h1,
            "file": file_ref,
            "gsc": {
                "impressions": impressions,
                "clicks": clicks,
                "ctr": ctr,
                "avg_position": avg_position,
            },
            "clicks_lost": clicks_lost,
            "issues_detected": {
                "file_not_found": !health.file_found,
                "title_too_long": !health.title_ok,
                "meta_too_short": !health.meta_ok,
                "snippet_suboptimal": !health.snippet_ok,
                "missing_faq_schema": !health.faq_ok,
            },
        }));
    }

    if skipped_healthy > 0 || skipped_unchanged > 0 {
        log::info!(
            "[ctr_audit] Skipped {} healthy + {} unchanged articles",
            skipped_healthy,
            skipped_unchanged
        );
    }

    // Sort by clicks_lost descending
    article_records.sort_by(|a, b| {
        let ca = a["clicks_lost"].as_f64().unwrap_or(0.0);
        let cb = b["clicks_lost"].as_f64().unwrap_or(0.0);
        cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
    });

    let top_20: Vec<&serde_json::Value> = article_records.iter().take(20).collect();

    let now_iso = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let full_doc = serde_json::json!({
        "generated_at": now_iso,
        "total_articles": article_records.len(),
        "articles": article_records,
        "top_20_by_clicks_lost": top_20,
    });

    // Write full context to automation dir for reference
    let out_path = paths.automation_dir.join("ctr_audit_context.json");
    let full_str = serde_json::to_string_pretty(&full_doc).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&out_path, &full_str) {
        log::warn!("[ctr_audit] Failed to write ctr_audit_context.json: {}", e);
    }

    // Return only the top 20 as step output to keep the agentic prompt small
    let summary_doc = serde_json::json!({
        "generated_at": now_iso,
        "total_articles": article_records.len(),
        "top_20_by_clicks_lost": top_20,
        "cleaned_stale_entries": cleaned_summary.len(),
        "cleaned_files": cleaned_summary,
    });
    let summary_str = serde_json::to_string_pretty(&summary_doc).unwrap_or_default() + "\n";

    let clean_msg = if cleaned_summary.is_empty() {
        String::new()
    } else {
        format!(
            " — removed {} stale entries from articles.json",
            cleaned_summary.len()
        )
    };

    StepResult {
        success: true,
        message: format!(
            "CTR context built for {} articles ({} healthy, {} unchanged){}",
            article_records.len(),
            skipped_healthy,
            skipped_unchanged,
            clean_msg
        ),
        output: Some(summary_str),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 2: Analyze
// ═══════════════════════════════════════════════════════════════════════════════

/// Run the CTR optimization analysis using an LLM agent.
///
/// Loads the "ctr-optimization" skill, builds a prompt with the skill content
/// and the provided context JSON, and delegates to the agent.
pub(crate) fn exec_ctr_analyze(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: &str,
) -> StepResult {
    // Quick check: if the context contains zero articles with issues, skip the agent call.
    let context_doc: serde_json::Value = match serde_json::from_str(context_json) {
        Ok(v) => v,
        Err(_) => {
            // If we can't parse the context, still try the agent — it might handle raw text.
            serde_json::Value::Null
        }
    };
    let total_articles = context_doc["total_articles"].as_i64().unwrap_or(-1);
    if total_articles == 0 {
        log::info!("[ctr_audit] No articles with CTR issues detected. Skipping agent analysis.");
        return StepResult {
            success: true,
            message: "All articles look healthy — no CTR issues detected.".to_string(),
            output: Some("{\"recommendations\":[],\"summary\":\"All clear – every article passes the current health checks.\"}".to_string()),
        };
    }

    let repo_root = Path::new(project_path);

    let skill = match load_skill_with_fallback(repo_root, "ctr-optimization") {
        Some(s) => s,
        None => {
            return StepResult {
                success: false,
                message: "Skill 'ctr-optimization' not found in .github/skills/ or app defaults".to_string(),
                output: None,
            };
        }
    };

    // Use string concatenation to avoid format! panics if skill content contains { or }
    let prompt = skill.content
        + "\n\n---\n\n## CTR Audit Context\n\n"
        + context_json
        + "\n\nPlease analyze the above context and provide actionable CTR optimization recommendations."
        + "\n\nCRITICAL: Return ONLY a single JSON object matching the Output Contract above."
        + " Do not include markdown prose, summaries, tables, or explanations outside the JSON."
        + " Do not write files. Output the JSON directly in your response.";

    match agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(output) => StepResult {
            success: true,
            message: "CTR analysis completed".to_string(),
            output: Some(output),
        },
        Err(e) => StepResult {
            success: false,
            message: format!("Agent error during CTR analysis: {}", e),
            output: None,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Create Fix Tasks
// ═══════════════════════════════════════════════════════════════════════════════

/// Spawn up to 3 CTR fix tasks based on the recommendations artifact.
///
/// Looks for a `ctr_recommendations` artifact on the parent task; falls back
/// to reading `ctr_recommendations.json` from the automation directory.
pub(crate) fn create_ctr_fix_tasks(
    conn: &Connection,
    parent_task: &Task,
    project_path: &str,
) -> Vec<String> {
    let paths = ProjectPaths::from_path(project_path);

    // Try to find the artifact on the parent task first
    let recommendation_json = parent_task
        .artifacts
        .iter()
        .find(|a| a.key == "ctr_recommendations")
        .and_then(|a| a.content.clone())
        .or_else(|| {
            // Fallback: read from automation dir
            let fallback_path = paths.automation_dir.join("ctr_recommendations.json");
            std::fs::read_to_string(&fallback_path).ok()
        })
        .unwrap_or_default();

    if recommendation_json.is_empty() {
        log::warn!(
            "[ctr_audit] No ctr_recommendations artifact found for task {}",
            parent_task.id
        );
        return Vec::new();
    }

    // Parse recommendations so we can filter per fix task type.
    // Each fix task only receives recommendations relevant to its specialty,
    // preventing the agent from hitting step limits by trying to fix everything.
    let full_recommendations: serde_json::Value = match serde_json::from_str(&recommendation_json) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[ctr_audit] Failed to parse recommendations JSON: {}", e);
            return Vec::new();
        }
    };

    let fix_task_configs = [
        (
            "fix_title_meta",
            format!("ctr_fix:title_meta:{}:{}", parent_task.project_id, parent_task.id),
            vec!["title_rewrite", "meta_description"],
        ),
        (
            "fix_faq_schema",
            format!("ctr_fix:faq:{}:{}", parent_task.project_id, parent_task.id),
            vec!["faq_schema"],
        ),
        (
            "fix_snippet_bait",
            format!("ctr_fix:snippet:{}:{}", parent_task.project_id, parent_task.id),
            vec!["snippet_bait"],
        ),
    ];

    let mut created_ids = Vec::new();

    for (task_type, idempotency_key, allowed_fix_types) in &fix_task_configs {
        let filtered = filter_recommendations_by_fix_type(&full_recommendations, allowed_fix_types);
        let filtered_json = match serde_json::to_string(&filtered) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[ctr_audit] Failed to serialize filtered recommendations: {}", e);
                continue;
            }
        };

        // Skip creating this fix task if there are no relevant recommendations
        let rec_count = filtered["recommendations"].as_array().map(|r| r.len()).unwrap_or(0);
        if rec_count == 0 {
            log::info!(
                "[ctr_audit] No {} recommendations found — skipping fix task",
                task_type
            );
            continue;
        }

        let artifact = TaskArtifact {
            key: "ctr_recommendations".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("ctr_audit".to_string()),
            content: Some(filtered_json),
        };

        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: task_type.to_string(),
            title: Some(format!("CTR fix: {} ({} articles)", task_type, rec_count)),
            description: Some(format!(
                "Follow-up CTR fix task from {} (parent: {}) — {} articles to fix",
                task_type, parent_task.id, rec_count
            )),
            priority: crate::models::task::Priority::Medium,
            execution_mode: Some(ExecutionMode::Automatic),
            agent_policy: crate::models::task::AgentPolicy::Optional,
            depends_on: vec![parent_task.id.clone()],
            artifacts: vec![artifact],
            idempotency_key: Some(idempotency_key.clone()),
            ..Default::default()
        };

        match TaskSpawner::spawn(conn, spec) {
            Ok(task) => {
                log::info!("[ctr_audit] Created fix task {} (type: {}, {} articles)", task.id, task_type, rec_count);
                created_ids.push(task.id);
            }
            Err(e) => {
                log::warn!("[ctr_audit] Failed to create fix task {}: {}", task_type, e);
            }
        }
    }

    created_ids
}

/// Filter recommendations to only include fixes of the specified types.
/// Returns a new JSON object with only matching recommendations.
fn filter_recommendations_by_fix_type(
    full: &serde_json::Value,
    allowed_types: &[&str],
) -> serde_json::Value {
    let empty_arr = vec![];
    let recommendations = full["recommendations"].as_array().unwrap_or(&empty_arr);

    let filtered: Vec<serde_json::Value> = recommendations
        .iter()
        .filter_map(|rec| {
            let fixes = rec["fixes"].as_array()?;
            let matching_fixes: Vec<serde_json::Value> = fixes
                .iter()
                .filter(|f| {
                    f["type"]
                        .as_str()
                        .map(|t| allowed_types.contains(&t))
                        .unwrap_or(false)
                })
                .cloned()
                .collect();

            if matching_fixes.is_empty() {
                return None;
            }

            let mut new_rec = rec.clone();
            new_rec["fixes"] = serde_json::Value::Array(matching_fixes);
            Some(new_rec)
        })
        .collect();

    serde_json::json!({
        "recommendations": filtered,
        "summary": format!("Filtered for fix types: {:?}", allowed_types),
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_dir() -> String {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir()
            .join(format!("ctr_audit_test_{}_{}", std::process::id(), n))
            .to_string_lossy()
            .to_string()
    }

    fn setup_project(path: &str) {
        let _ = std::fs::remove_dir_all(path);
        let auto_dir = Path::new(path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();
        let content_dir = Path::new(path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        let articles = serde_json::json!({
            "articles": [
                {
                    "id": 1,
                    "url_slug": "test-article",
                    "title": "Test Article | Brand | Brand -- Tagline",
                    "target_keyword": "test article",
                    "file": "content/001_test_article.mdx",
                    "gsc": { "impressions": 10000.0, "clicks": 10.0, "ctr": 0.001, "avg_position": 8.5 }
                },
                {
                    "id": 2,
                    "url_slug": "another-article",
                    "title": "Another Article",
                    "target_keyword": "another article",
                    "file": "content/002_another_article.mdx",
                    "gsc": { "impressions": 5000.0, "clicks": 5.0, "ctr": 0.001, "avg_position": 12.0 }
                }
            ]
        });
        std::fs::write(auto_dir.join("articles.json"), serde_json::to_string_pretty(&articles).unwrap()).unwrap();

        let mdx1 = r#"---
title: "Test Article | Brand | Brand -- Tagline"
description: "A short desc"
date: "2024-01-01"
---

# Test Article | Brand | Brand -- Tagline

This is the first paragraph of the test article. It contains some content.

## Section 1

More content here.
"#;
        std::fs::write(content_dir.join("001_test_article.mdx"), mdx1).unwrap();

        let mdx2 = r#"---
title: "Another Article"
description: ""
date: "2024-01-02"
---

# Another Article

This is another article with different content.
"#;
        std::fs::write(content_dir.join("002_another_article.mdx"), mdx2).unwrap();
    }

    fn cleanup(path: &str) {
        let _ = std::fs::remove_dir_all(path);
    }

    fn test_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    #[test]
    fn test_read_article_excerpt() {
        let path = test_dir();
        setup_project(&path);
        let (title, meta, first, h1, has_faq, file_found) =
            crate::engine::exec::audit_health::read_article_excerpt(&path, "content/001_test_article.mdx");
        assert_eq!(title, "Test Article | Brand | Brand -- Tagline");
        assert_eq!(meta, "A short desc");
        assert_eq!(h1, "Test Article | Brand | Brand -- Tagline");
        assert!(first.contains("This is the first paragraph"));
        assert!(!has_faq, "Should not detect FAQ schema in this article");
        assert!(file_found, "File should exist");
        cleanup(&path);
    }

    #[test]
    fn test_exec_ctr_build_context() {
        let path = test_dir();
        setup_project(&path);
        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "ctr_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test CTR Audit".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let conn = test_db();
        let result = exec_ctr_build_context(&task, &path, &conn);
        assert!(result.success, "build_context failed: {}", result.message);

        let output: serde_json::Value = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(output["total_articles"].as_i64().unwrap(), 2);

        let articles = output["top_20_by_clicks_lost"].as_array().unwrap();
        let first = &articles[0];
        assert!(first["clicks_lost"].as_f64().unwrap() > 0.0);
        assert_eq!(first["title"].as_str().unwrap(), "Test Article | Brand | Brand -- Tagline");
        assert_eq!(first["meta_description"].as_str().unwrap(), "A short desc");
        assert!(!first["first_paragraph"].as_str().unwrap().is_empty());
        cleanup(&path);
    }

    #[test]
    fn test_clicks_lost_computation() {
        let path = test_dir();
        setup_project(&path);
        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "ctr_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let conn = test_db();
        let result = exec_ctr_build_context(&task, &path, &conn);
        let output: serde_json::Value = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        let articles = output["top_20_by_clicks_lost"].as_array().unwrap();

        let a1 = articles.iter().find(|a| a["id"].as_i64().unwrap() == 1).unwrap();
        let cl1 = a1["clicks_lost"].as_f64().unwrap();
        assert!((cl1 - 40.0).abs() < 0.1, "Expected ~40 clicks_lost, got {}", cl1);

        let a2 = articles.iter().find(|a| a["id"].as_i64().unwrap() == 2).unwrap();
        let cl2 = a2["clicks_lost"].as_f64().unwrap();
        assert!((cl2 - 20.0).abs() < 0.1, "Expected ~20 clicks_lost, got {}", cl2);
        cleanup(&path);
    }

    #[test]
    fn test_faq_schema_detection() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        let content_dir = Path::new(&path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        // MDX with JSON-LD FAQPage schema
        let mdx_with_faq = r#"---
title: "FAQ Article"
description: "An article with FAQ"
date: "2024-01-01"
---

# FAQ Article

Some content here.

<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@type": "FAQPage",
  "mainEntity": [
    {
      "@type": "Question",
      "name": "What is this?",
      "acceptedAnswer": {
        "@type": "Answer",
        "text": "This is a test."
      }
    }
  ]
}
</script>
"#;
        std::fs::write(content_dir.join("with_faq.mdx"), mdx_with_faq).unwrap();

        let (title, meta, first, h1, has_faq, file_found) =
            crate::engine::exec::audit_health::read_article_excerpt(&path, "content/with_faq.mdx");
        assert_eq!(title, "FAQ Article");
        assert!(has_faq, "Should detect JSON-LD FAQPage schema");
        assert!(file_found);

        // MDX with markdown FAQ heading but no schema
        let mdx_no_faq = r#"---
title: "No FAQ Article"
description: "An article without FAQ"
date: "2024-01-01"
---

# No FAQ Article

Some content here.
"#;
        std::fs::write(content_dir.join("no_faq.mdx"), mdx_no_faq).unwrap();

        let (_, _, _, _, has_faq2, file_found2) =
            crate::engine::exec::audit_health::read_article_excerpt(&path, "content/no_faq.mdx");
        assert!(!has_faq2, "Should not detect FAQ schema when absent");
        assert!(file_found2);

        // MDX with markdown FAQ heading
        let mdx_md_faq = r#"---
title: "Markdown FAQ"
description: "An article with markdown FAQ"
date: "2024-01-01"
---

# Markdown FAQ

## FAQ

Q: What?\nA: This.
"#;
        std::fs::write(content_dir.join("md_faq.mdx"), mdx_md_faq).unwrap();

        let (_, _, _, _, has_faq3, file_found3) =
            crate::engine::exec::audit_health::read_article_excerpt(&path, "content/md_faq.mdx");
        assert!(has_faq3, "Should detect markdown FAQ heading");
        assert!(file_found3);

        cleanup(&path);
    }

    #[test]
    fn test_missing_file_detected() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        let auto_dir = Path::new(&path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();

        let articles = serde_json::json!({
            "articles": [
                {
                    "id": 1,
                    "url_slug": "missing-article",
                    "title": "Missing Article",
                    "target_keyword": "missing article",
                    "file": "content/does_not_exist.mdx",
                    "gsc": { "impressions": 1000.0, "clicks": 5.0, "ctr": 0.005, "avg_position": 10.0 }
                }
            ]
        });
        std::fs::write(auto_dir.join("articles.json"), serde_json::to_string_pretty(&articles).unwrap()).unwrap();

        // File does not exist — read_article_excerpt should return file_found=false
        let (title, meta, first, _h1, _has_faq, file_found) =
            crate::engine::exec::audit_health::read_article_excerpt(&path, "content/does_not_exist.mdx");
        assert!(!file_found, "Should report file not found");
        assert_eq!(title, "");
        assert_eq!(meta, "");
        assert_eq!(first, "");

        // Health check should flag file_not_found
        let health = crate::engine::exec::audit_health::check_article_health(
            &title, &meta, &first, "missing article", false, file_found,
        );
        assert!(!health.all_ok(), "Missing file should not be healthy");
        assert!(health.issues.contains(&"file_not_found".to_string()), "Should flag file_not_found");

        // Build context should include the article with file_not_found issue
        let conn = test_db();
        let task = Task {
            id: "task-missing".to_string(),
            project_id: "proj-missing".to_string(),
            task_type: "ctr_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Missing File Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_build_context(&task, &path, &conn);
        assert!(result.success);
        let output: serde_json::Value = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(output["total_articles"].as_i64().unwrap(), 1);

        let articles = output["top_20_by_clicks_lost"].as_array().unwrap();
        let first = &articles[0];
        assert_eq!(first["issues_detected"]["file_not_found"].as_bool().unwrap(), true);

        cleanup(&path);
    }

    /// When all articles already have good titles, meta, snippets, and FAQ schema,
    /// the audit should return 0 articles and the analyze step should skip the agent.
    #[test]
    fn test_all_healthy_skips_agent() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        let auto_dir = Path::new(&path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();
        let content_dir = Path::new(&path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        let articles = serde_json::json!({
            "articles": [
                {
                    "id": 1,
                    "url_slug": "healthy-article",
                    "title": "Healthy Article",
                    "target_keyword": "healthy article",
                    "file": "content/001_healthy.mdx",
                    "gsc": { "impressions": 10000.0, "clicks": 10.0, "ctr": 0.001, "avg_position": 8.5 }
                }
            ]
        });
        std::fs::write(auto_dir.join("articles.json"), serde_json::to_string_pretty(&articles).unwrap()).unwrap();

        // Good title (<=60), good meta (>=50 chars), good snippet (>=30 words + contains keyword), has FAQ schema
        let mdx = r#"---
title: "Healthy Article"
description: "This is a very good meta description that is definitely longer than fifty characters for sure."
date: "2024-01-01"
---

# Healthy Article

One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight twentynine thirty healthy article.

## FAQ

**Q: What is this?**\nA: A test article.
"#;
        std::fs::write(content_dir.join("001_healthy.mdx"), mdx).unwrap();

        let task = Task {
            id: "task-healthy".to_string(),
            project_id: "proj-healthy".to_string(),
            task_type: "ctr_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Healthy Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        // Build context should find 0 articles with issues
        let conn = test_db();
        let result = exec_ctr_build_context(&task, &path, &conn);
        assert!(result.success);
        let output: serde_json::Value = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(output["total_articles"].as_i64().unwrap(), 0, "Expected 0 articles with issues");

        // Analyze step should skip the agent and return "all clear"
        let context_json = result.output.unwrap();
        let analyze_result = exec_ctr_analyze(&task, &path, "kimi", &context_json);
        assert!(analyze_result.success);
        assert!(analyze_result.message.contains("All articles look healthy"), "Expected early-exit message, got: {}", analyze_result.message);

        cleanup(&path);
    }
}
