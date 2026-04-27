/// CTR (Click-Through Rate) audit execution module.
///
/// Covers:
///   - exec_ctr_build_context   (deterministic data collection + clicks_lost scoring)
///   - exec_ctr_analyze         (agentic analysis with ctr-optimization skill)
///   - create_ctr_fix_tasks     (spawn follow-up fix tasks)

mod analyze;
mod apply;
mod context;
mod task_spawner;

pub(crate) use analyze::*;
pub(crate) use apply::*;
pub(crate) use context::*;
pub(crate) use task_spawner::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ctr::CtrFixVerificationReport;
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
        let auto_dir = std::path::Path::new(path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();
        let content_dir = std::path::Path::new(path).join("content");
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
        let task = crate::models::task::Task {
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
        let task = crate::models::task::Task {
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
        let content_dir = std::path::Path::new(&path).join("content");
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
        let auto_dir = std::path::Path::new(&path).join(".github").join("automation");
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
        let task = crate::models::task::Task {
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
        let auto_dir = std::path::Path::new(&path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();
        let content_dir = std::path::Path::new(&path).join("content");
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

        // Good title (<=55), good meta (130-155 chars), good snippet (40-60 words + contains keyword), has FAQ schema
        let mdx = r#"---
title: "Healthy Article"
description: "This is a very good meta description that is definitely longer than one hundred and thirty characters so it passes the new strict health check thresholds."
date: "2024-01-01"
---

# Healthy Article

One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight twentynine thirty thirtyone thirtytwo thirtythree thirtyfour thirtyfive thirtysix thirtyseven thirtyeight thirtynine forty healthy article.

## FAQ

**Q: What is this?**\nA: A test article.
"#;
        std::fs::write(content_dir.join("001_healthy.mdx"), mdx).unwrap();

        let task = crate::models::task::Task {
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

    #[test]
    fn exec_ctr_fix_apply_success() {
        let path = test_dir();
        setup_project(&path);

        let patch = serde_json::json!({
            "article_id": 1,
            "file": "content/001_test_article.mdx",
            "changes": {
                "title": "New Title",
                "description": "This is a new meta description that is definitely longer than one hundred and thirty characters so it passes the strict health check.",
                "first_paragraph": "This is the replaced first paragraph. It contains enough words to satisfy the new snippet requirement of at least forty words for the test article."
            }
        });

        let task = crate::models::task::Task {
            id: "task-fix".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_apply(&task, &path, Some(&patch.to_string()));
        assert!(result.success, "Apply failed: {}", result.message);

        // File should have new content
        let content = std::fs::read_to_string(std::path::Path::new(&path).join("content").join("001_test_article.mdx")).unwrap();
        assert!(content.contains("New Title"));
        assert!(content.contains("This is a new meta description"));
        assert!(content.contains("This is the replaced first paragraph"));

        cleanup(&path);
    }

    #[test]
    fn exec_ctr_fix_apply_corrupt_restore() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        let content_dir = std::path::Path::new(&path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        // Create a minimal MDX where the body has ONLY a first paragraph (no H1).
        // Replacing it with empty string will make the body empty, failing validation.
        let mdx = r#"---
title: "Minimal"
description: "desc"
date: "2024-01-01"
---

This is the only paragraph.
"#;
        std::fs::write(content_dir.join("minimal.mdx"), mdx).unwrap();

        let patch = serde_json::json!({
            "article_id": 1,
            "file": "content/minimal.mdx",
            "changes": {
                "first_paragraph": ""
            }
        });

        let task = crate::models::task::Task {
            id: "task-fix-corrupt".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix corrupt test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let original = std::fs::read_to_string(content_dir.join("minimal.mdx")).unwrap();

        let result = exec_ctr_fix_apply(&task, &path, Some(&patch.to_string()));
        assert!(!result.success, "Should fail on corrupted file");
        assert!(result.message.contains("integrity failed"), "Expected integrity failure, got: {}", result.message);

        // Original should be restored
        let restored = std::fs::read_to_string(content_dir.join("minimal.mdx")).unwrap();
        assert_eq!(original, restored, "Original should be restored");

        cleanup(&path);
    }

    #[test]
    fn exec_ctr_fix_apply_missing_file() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();

        let patch = serde_json::json!({
            "article_id": 99,
            "file": "content/missing.mdx",
            "changes": {
                "title": "New Title"
            }
        });

        let task = crate::models::task::Task {
            id: "task-fix-missing".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix missing test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_apply(&task, &path, Some(&patch.to_string()));
        assert!(!result.success, "Should fail for missing file");
        assert!(result.message.contains("File not found"), "Expected file not found, got: {}", result.message);

        cleanup(&path);
    }

    #[test]
    fn exec_ctr_verify_fix_all_pass() {
        let path = test_dir();
        setup_project(&path);

        // Write a healthy file
        let mdx = r#"---
title: "Good Title"
description: "This is a very good meta description that is definitely longer than one hundred and thirty characters so it passes the strict health check."
date: "2024-01-01"
---

# Good Title

One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight twentynine thirty thirtyone thirtytwo thirtythree thirtyfour thirtyfive thirtysix thirtyseven thirtyeight thirtynine forty test article.

## FAQ

Q: What?\nA: This.
"#;
        std::fs::write(std::path::Path::new(&path).join("content").join("001_test_article.mdx"), mdx).unwrap();

        let artifact_json = serde_json::json!({
            "article_id": 1,
            "url_slug": "test-article",
            "file": "content/001_test_article.mdx",
            "target_keyword": "test article",
            "fixes": [
                {"type": "title_rewrite", "recommended": "Good Title"},
                {"type": "meta_description", "recommended": "This is a very good meta description that is definitely longer than one hundred and thirty characters so it passes the strict health check."},
                {"type": "snippet_bait", "recommended": "One two three..."},
                {"type": "faq_schema", "recommended": [{"question": "What?", "answer": "This."}]}
            ]
        });

        let task = crate::models::task::Task {
            id: "task-verify".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Verify test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_recommendations".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_audit".to_string()),
                content: Some(artifact_json.to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_verify_fix(&task, &path);
        assert!(result.success, "Verification should pass: {}", result.message);
        let report: CtrFixVerificationReport = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(report.overall_status, "verified");
        assert!(report.checks.iter().all(|c| c.status == "pass"));

        cleanup(&path);
    }

    #[test]
    fn exec_ctr_verify_fix_partial_fail() {
        let path = test_dir();
        setup_project(&path);

        // File has a meta description that is too short (116 chars)
        let mdx = r#"---
title: "Good Title"
description: "This is only one hundred sixteen chars long so it fails."
date: "2024-01-01"
---

# Good Title

One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight twentynine thirty thirtyone thirtytwo thirtythree thirtyfour thirtyfive thirtysix thirtyseven thirtyeight thirtynine forty test article.
"#;
        std::fs::write(std::path::Path::new(&path).join("content").join("001_test_article.mdx"), mdx).unwrap();

        let artifact_json = serde_json::json!({
            "article_id": 1,
            "url_slug": "test-article",
            "file": "content/001_test_article.mdx",
            "target_keyword": "test article",
            "fixes": [
                {"type": "meta_description", "recommended": "This is only one hundred sixteen chars long so it fails."},
                {"type": "snippet_bait", "recommended": "One two three..."}
            ]
        });

        let task = crate::models::task::Task {
            id: "task-verify-partial".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Verify partial test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_recommendations".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_audit".to_string()),
                content: Some(artifact_json.to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_verify_fix(&task, &path);
        assert!(!result.success, "Verification should find issues");
        let report: CtrFixVerificationReport = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(report.overall_status, "partial");
        let meta_check = report.checks.iter().find(|c| c.check_type == "description").unwrap();
        assert_eq!(meta_check.status, "fail");
        assert!(meta_check.detail.as_ref().unwrap().contains("130"), "Should mention 130 char minimum: {:?}", meta_check.detail);

        cleanup(&path);
    }

    #[test]
    fn create_ctr_fix_tasks_per_article() {
        let path = test_dir();
        setup_project(&path);
        let conn = test_db();

        let parent_task = crate::models::task::Task {
            id: "parent-task".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "ctr_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Parent Audit".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_recommendations".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_audit".to_string()),
                content: Some(serde_json::json!({
                    "recommendations": [
                        {
                            "article_id": 1,
                            "url_slug": "test-article",
                            "file": "content/001_test_article.mdx",
                            "target_keyword": "test article",
                            "fixes": [
                                {"type": "title_rewrite", "recommended": "New Title"}
                            ]
                        },
                        {
                            "article_id": 2,
                            "url_slug": "another-article",
                            "file": "content/002_another_article.mdx",
                            "target_keyword": "another article",
                            "fixes": [
                                {"type": "meta_description", "recommended": "New meta"}
                            ]
                        }
                    ]
                }).to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        // Project and parent task must exist in DB for spawner validation
        let project = crate::models::project::Project {
            id: "proj-test".to_string(),
            name: "Test Project".to_string(),
            path: path.clone(),
            content_dir: None,
            site_url: None,
            site_id: None,
            sitemap_url: None,
            project_mode: crate::models::project::ProjectMode::Workspace,
            active: true,
            agent_provider: None,
            seo_provider: Some("ahrefs".to_string()),
        };
        crate::engine::task_store::create_project(&conn, &project).unwrap();
        crate::engine::task_store::create_task(&conn, &parent_task).unwrap();

        let ids = create_ctr_fix_tasks(&conn, &parent_task, &path);
        assert_eq!(ids.len(), 2, "Should create 2 fix tasks, got {}", ids.len());

        // Verify tasks are correct type
        for id in &ids {
            let task = crate::engine::task_store::get_task(&conn, id).unwrap();
            assert_eq!(task.task_type, "fix_ctr_article");
        }

        cleanup(&path);
    }

    /// End-to-end regression test: CTR fix apply on complex frontmatter preserves
    /// YAML lists, comments, and nested objects. This was the original sanitizer
    /// root-cause — the old `replace_frontmatter_field` would match indented lines
    /// and alias lines, destroying structured data.
    #[test]
    fn exec_ctr_fix_apply_preserves_complex_frontmatter() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        let content_dir = std::path::Path::new(&path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        // File with complex frontmatter: comments, FAQ list, citations, alias
        let mdx = r#"---
title: "Old Title"
metaDescription: "Old alias desc"
description: "Old desc"
date: "2024-01-01"
# AI SEO: FAQ Schema
faq:
  - question: "What is this?"
    answer: "This is a test."
  - question: "Why?"
    answer: "For regression testing."
citations:
  - source: "Example"
    url: "https://example.com"
---

# Old Title

This is the first paragraph that should be replaced with something longer so it passes the health check. One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight twentynine thirty thirtyone thirtytwo thirtythree thirtyfour thirtyfive thirtysix thirtyseven thirtyeight thirtynine forty.

## FAQ

Q: What is this?
A: This is a test.
"#;

        let file_path = content_dir.join("test_article.mdx");
        std::fs::write(&file_path, mdx).unwrap();

        let patch = serde_json::json!({
            "article_id": 1,
            "file": "content/test_article.mdx",
            "changes": {
                "title": "New Title",
                "description": "This is a very good meta description that is definitely longer than one hundred and thirty characters so it passes the strict health check.",
                "first_paragraph": "One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight twentynine thirty thirtyone thirtytwo thirtythree thirtyfour thirtyfive thirtysix thirtyseven thirtyeight thirtynine forty test article."
            }
        });

        let task = crate::models::task::Task {
            id: "task-fix-complex".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix complex frontmatter test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![
                crate::models::task::TaskArtifact {
                    key: "ctr_recommendations".to_string(),
                    path: None,
                    artifact_type: None,
                    source: None,
                    content: Some(serde_json::json!({
                        "article_id": 1,
                        "file": "content/test_article.mdx",
                        "target_keyword": "test article",
                        "fixes": [
                            { "type": "TitleRewrite", "recommended": "New Title" },
                            { "type": "MetaDescription", "recommended": "New desc" },
                            { "type": "SnippetBait", "recommended": "New paragraph" }
                        ]
                    }).to_string()),
                }
            ],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_apply(&task, &path, Some(&patch.to_string()));
        assert!(result.success, "CTR fix apply failed: {}", result.message);

        // Read the modified file
        let modified = std::fs::read_to_string(&file_path).unwrap();

        // Title and description should be updated
        assert!(modified.contains("title: \"New Title\""), "Title was not updated");
        assert!(modified.contains("description: \"This is a very good meta description"), "Description was not updated");

        // Alias should be removed
        assert!(!modified.contains("metaDescription:"), "metaDescription alias was not removed");

        // Complex YAML must be preserved
        assert!(modified.contains("faq:"), "FAQ list was destroyed");
        assert!(modified.contains("  - question: \"What is this?\""), "FAQ question 1 was destroyed");
        assert!(modified.contains("  - question: \"Why?\""), "FAQ question 2 was destroyed");
        assert!(modified.contains("# AI SEO: FAQ Schema"), "Comment was destroyed");
        assert!(modified.contains("citations:"), "Citations list was destroyed");
        assert!(modified.contains("  - source: \"Example\""), "Citation source was destroyed");

        // First paragraph should be replaced
        assert!(!modified.contains("This is the first paragraph that should be replaced"), "First paragraph was not replaced");
        assert!(modified.contains("One two three four five six seven"), "New first paragraph is missing");

        cleanup(&path);
    }

    /// Phase 1 contract enforcement: recommendations missing required `file` or
    /// `target_keyword` fields must be rejected rather than spawning broken fix tasks.
    #[test]
    fn create_ctr_fix_tasks_rejects_incomplete_recommendations() {
        let path = test_dir();
        setup_project(&path);
        let conn = test_db();

        let parent_task = crate::models::task::Task {
            id: "parent-reject".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "ctr_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Reject Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_recommendations".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_audit".to_string()),
                content: Some(serde_json::json!({
                    "recommendations": [
                        {
                            "article_id": 1,
                            "url_slug": "test-article",
                            "file": "",
                            "target_keyword": "test article",
                            "fixes": [{"type": "title_rewrite", "recommended": "New Title"}]
                        },
                        {
                            "article_id": 2,
                            "url_slug": "another-article",
                            "file": "content/002_another_article.mdx",
                            "target_keyword": "",
                            "fixes": [{"type": "meta_description", "recommended": "New meta"}]
                        },
                        {
                            "article_id": 3,
                            "url_slug": "valid-article",
                            "file": "content/001_test_article.mdx",
                            "target_keyword": "test article",
                            "fixes": [{"type": "title_rewrite", "recommended": "Good Title"}]
                        }
                    ]
                }).to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let project = crate::models::project::Project {
            id: "proj-test".to_string(),
            name: "Test Project".to_string(),
            path: path.clone(),
            content_dir: None,
            site_url: None,
            site_id: None,
            sitemap_url: None,
            project_mode: crate::models::project::ProjectMode::Workspace,
            active: true,
            agent_provider: None,
            seo_provider: Some("ahrefs".to_string()),
        };
        crate::engine::task_store::create_project(&conn, &project).unwrap();
        crate::engine::task_store::create_task(&conn, &parent_task).unwrap();

        let ids = create_ctr_fix_tasks(&conn, &parent_task, &path);
        assert_eq!(ids.len(), 1, "Should create exactly 1 fix task (only the valid recommendation), got {}", ids.len());

        let task = crate::engine::task_store::get_task(&conn, &ids[0]).unwrap();
        assert!(task.description.as_ref().unwrap().contains("valid-article"));

        cleanup(&path);
    }
}
