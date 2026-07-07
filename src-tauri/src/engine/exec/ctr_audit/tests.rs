    use super::*;
    use crate::models::ctr::CtrFixVerificationReport;
    use crate::models::task::{FollowUpPolicy, TaskReviewSurface};
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
        let auto_dir = std::path::Path::new(path)
            .join(".github")
            .join("automation");
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
        std::fs::write(
            auto_dir.join("articles.json"),
            serde_json::to_string_pretty(&articles).unwrap(),
        )
        .unwrap();

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

    fn insert_test_project(conn: &rusqlite::Connection, path: &str) {
        let project = crate::models::project::Project {
            id: "proj-test".to_string(),
            name: "Test Project".to_string(),
            path: path.to_string(),
            content_dir: None,
            site_url: None,
            site_id: None,
            sitemap_url: None,
            project_mode: crate::models::project::ProjectMode::Workspace,
            active: true,
            agent_provider: None,
            seo_provider: Some("ahrefs".to_string()),
            clarity_project_id: None,
        };
        crate::engine::task_store::create_project(conn, &project).unwrap();
    }

    fn ctr_parent_task(id: &str, content: serde_json::Value) -> crate::models::task::Task {
        crate::models::task::Task {
            id: id.to_string(),
            project_id: "proj-test".to_string(),
            task_type: "ctr_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Parent Audit".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_build_context".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_audit".to_string()),
                content: Some(content.to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        }
    }

    fn ctr_context_for_article(content_hash: &str) -> serde_json::Value {
        serde_json::json!({
            "total_articles": 1,
            "articles": [
                {
                    "id": 1,
                    "url_slug": "test-article",
                    "file": "content/001_test_article.mdx",
                    "content_hash": content_hash,
                    "target_keyword": "test article",
                    "issues_detected": {
                        "file_not_found": false,
                        "title_too_long": true,
                        "meta_too_short": false,
                        "snippet_suboptimal": false,
                        "missing_faq_schema": false
                    }
                }
            ]
        })
    }

    #[test]
    fn test_read_article_excerpt() {
        let path = test_dir();
        setup_project(&path);
        let (title, meta, first, h1, has_faq, file_found) =
            crate::engine::exec::audit_health::read_article_excerpt(
                &path,
                "content/001_test_article.mdx",
            );
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
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test CTR Audit".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let conn = test_db();
        let result = exec_ctr_build_context(&task, &path, None, &conn);
        assert!(result.success, "build_context failed: {}", result.message);

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(output["total_articles"].as_i64().unwrap(), 2);

        let articles = output["top_20_by_clicks_lost"].as_array().unwrap();
        let first = &articles[0];
        assert!(first["clicks_lost"].as_f64().unwrap() > 0.0);
        assert_eq!(
            first["title"].as_str().unwrap(),
            "Test Article | Brand | Brand -- Tagline"
        );
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
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let conn = test_db();
        let result = exec_ctr_build_context(&task, &path, None, &conn);
        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        let articles = output["top_20_by_clicks_lost"].as_array().unwrap();

        let a1 = articles
            .iter()
            .find(|a| a["id"].as_i64().unwrap() == 1)
            .unwrap();
        let cl1 = a1["clicks_lost"].as_f64().unwrap();
        // Article 1: pos 8.5 → target 0.8% → 10000 * (0.008 - 0.001) = 70
        assert!(
            (cl1 - 70.0).abs() < 0.1,
            "Expected ~70 clicks_lost, got {}",
            cl1
        );
        assert_eq!(a1["target_ctr"].as_f64().unwrap(), 0.008);

        let a2 = articles
            .iter()
            .find(|a| a["id"].as_i64().unwrap() == 2)
            .unwrap();
        let cl2 = a2["clicks_lost"].as_f64().unwrap();
        // Article 2: pos 12.0 → target 0.3% → 5000 * (0.003 - 0.001) = 10
        assert!(
            (cl2 - 10.0).abs() < 0.1,
            "Expected ~10 clicks_lost, got {}",
            cl2
        );
        assert_eq!(a2["target_ctr"].as_f64().unwrap(), 0.003);
        cleanup(&path);
    }

    #[test]
    fn test_target_ctr_for_position() {
        assert_eq!(target_ctr_for_position(1.0), 0.08);
        assert_eq!(target_ctr_for_position(2.0), 0.08);
        assert_eq!(target_ctr_for_position(3.0), 0.04);
        assert_eq!(target_ctr_for_position(4.0), 0.04);
        assert_eq!(target_ctr_for_position(5.0), 0.015);
        assert_eq!(target_ctr_for_position(7.0), 0.015);
        assert_eq!(target_ctr_for_position(8.0), 0.008);
        assert_eq!(target_ctr_for_position(10.0), 0.008);
        assert_eq!(target_ctr_for_position(11.0), 0.003);
        assert_eq!(target_ctr_for_position(20.0), 0.003);
        assert_eq!(target_ctr_for_position(21.0), 0.0);
        assert_eq!(target_ctr_for_position(0.0), 0.0);
    }

    #[test]
    fn test_classify_query_intent() {
        assert_eq!(
            classify_query_intent("what is a cash secured put"),
            "question"
        );
        assert_eq!(
            classify_query_intent("cash secured put vs naked put"),
            "comparison"
        );
        assert_eq!(
            classify_query_intent("best stocks for covered calls"),
            "best_list"
        );
        assert_eq!(classify_query_intent("option tax calculator"), "tax_legal");
        assert_eq!(classify_query_intent("theta decay tool"), "calculator_tool");
        assert_eq!(
            classify_query_intent("option selling strategies"),
            "generic"
        );
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
        let auto_dir = std::path::Path::new(&path)
            .join(".github")
            .join("automation");
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
        std::fs::write(
            auto_dir.join("articles.json"),
            serde_json::to_string_pretty(&articles).unwrap(),
        )
        .unwrap();

        // File does not exist — read_article_excerpt should return file_found=false
        let (title, meta, first, _h1, _has_faq, file_found) =
            crate::engine::exec::audit_health::read_article_excerpt(
                &path,
                "content/does_not_exist.mdx",
            );
        assert!(!file_found, "Should report file not found");
        assert_eq!(title, "");
        assert_eq!(meta, "");
        assert_eq!(first, "");

        // Health check should flag file_not_found
        let health = crate::engine::exec::audit_health::check_article_health(
            &title,
            &meta,
            &first,
            "missing article",
            false,
            file_found,
        );
        assert!(!health.all_ok(), "Missing file should not be healthy");
        assert!(
            health.issues.contains(&"file_not_found".to_string()),
            "Should flag file_not_found"
        );

        // Build context should include the article with file_not_found issue
        let conn = test_db();
        let task = crate::models::task::Task {
            id: "task-missing".to_string(),
            project_id: "proj-missing".to_string(),
            task_type: "ctr_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Missing File Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_build_context(&task, &path, None, &conn);
        assert!(result.success);
        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(output["total_articles"].as_i64().unwrap(), 1);

        let articles = output["top_20_by_clicks_lost"].as_array().unwrap();
        let first = &articles[0];
        assert_eq!(
            first["issues_detected"]["file_not_found"]
                .as_bool()
                .unwrap(),
            true
        );

        cleanup(&path);
    }

    /// When all articles already have good titles, meta, snippets, and FAQ schema,
    /// the audit should return 0 articles and the analyze step should skip the agent.
    #[test]
    fn test_all_healthy_skips_agent() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        let auto_dir = std::path::Path::new(&path)
            .join(".github")
            .join("automation");
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
        std::fs::write(
            auto_dir.join("articles.json"),
            serde_json::to_string_pretty(&articles).unwrap(),
        )
        .unwrap();

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
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Healthy Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        // Build context should find 0 articles with issues
        let conn = test_db();
        let result = exec_ctr_build_context(&task, &path, None, &conn);
        assert!(result.success);
        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(
            output["total_articles"].as_i64().unwrap(),
            0,
            "Expected 0 articles with issues"
        );

        // Analyze step should skip the agent and return "all clear"
        let context_json = result.output.unwrap();
        let analyze_result = exec_ctr_analyze(&task, &path, "kimi", &context_json);
        assert!(analyze_result.success);
        assert!(
            analyze_result.message.contains("All articles look healthy"),
            "Expected early-exit message, got: {}",
            analyze_result.message
        );

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
                "first_paragraph": "This is the replaced first paragraph for the test article, written to satisfy the snippet requirement with enough useful context for readers. It gives a direct answer, keeps the wording concise, and stays within the allowed word-count range for CTR verification."
            }
        });

        let task = crate::models::task::Task {
            id: "task-fix".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_apply(&task, &path, Some(&patch.to_string()));
        assert!(result.success, "Apply failed: {}", result.message);

        // File should have new content
        let content = std::fs::read_to_string(
            std::path::Path::new(&path)
                .join("content")
                .join("001_test_article.mdx"),
        )
        .unwrap();
        assert!(content.contains("New Title"));
        assert!(content.contains("This is a new meta description"));
        assert!(content.contains("This is the replaced first paragraph"));

        cleanup(&path);
    }

    #[test]
    fn exec_ctr_fix_apply_repairs_near_miss_patch_values() {
        let path = test_dir();
        setup_project(&path);

        let patch = serde_json::json!({
            "article_id": 1,
            "file": "content/001_test_article.mdx",
            "changes": {
                "title": "Coffee Grind Size Chart: Complete Guide with Micron Ranges",
                "description": "Coffee grind size chart with exact micron ranges for espresso, pour over, French press, AeroPress, moka pot, and cold brew so you can dial in flavor fast every morning without guessing.",
                "first_paragraph": "Fresh brewing starts with consistent particles and practical timing. This guide shows the exact texture, common mistakes, and simple adjustment cues that help home brewers match each method with a repeatable grind size for better cups every day at home."
            }
        });

        let artifact_json = serde_json::json!({
            "article_id": 1,
            "url_slug": "test-article",
            "file": "content/001_test_article.mdx",
            "target_keyword": "test article",
            "fixes": [
                {"type": "title_rewrite", "recommended": "Coffee Grind Size Chart: Complete Guide with Micron Ranges"},
                {"type": "meta_description", "recommended": "Coffee grind size chart with exact micron ranges for espresso, pour over, French press, AeroPress, moka pot, and cold brew so you can dial in flavor fast every morning without guessing."},
                {"type": "snippet_bait", "recommended": "Fresh brewing starts with consistent particles..."}
            ]
        });

        let task = crate::models::task::Task {
            id: "task-fix-near-miss".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix near miss test".to_string()),
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
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_apply(&task, &path, Some(&patch.to_string()));
        assert!(result.success, "Apply failed: {}", result.message);
        assert!(
            result.message.contains("normalized"),
            "Expected normalization message, got: {}",
            result.message
        );

        let content = std::fs::read_to_string(
            std::path::Path::new(&path)
                .join("content")
                .join("001_test_article.mdx"),
        )
        .unwrap();
        let (frontmatter, body) = crate::content::frontmatter::split_mdx(&content).unwrap();
        let scalars = crate::content::frontmatter::top_level_scalars(frontmatter);
        let title = scalars
            .iter()
            .find(|field| field.key == "title")
            .map(|field| field.raw_value.trim_matches('"').trim_matches('\''))
            .unwrap();
        let description = scalars
            .iter()
            .find(|field| field.key == "description")
            .map(|field| field.raw_value.trim_matches('"').trim_matches('\''))
            .unwrap();
        let first_paragraph = crate::content::cleaner::find_first_paragraph_range(body)
            .map(|(start, end)| body[start..end].trim().to_string())
            .unwrap();

        assert!(title.chars().count() <= crate::engine::exec::audit_health::TITLE_MAX_LEN);
        assert!(
            description.chars().count() >= crate::engine::exec::audit_health::META_MIN_LEN
                && description.chars().count() <= crate::engine::exec::audit_health::META_MAX_LEN
        );
        assert!(first_paragraph.contains("test article?") || first_paragraph.contains('?'));
        assert!(
            crate::content::ops::count_words(&first_paragraph)
                <= crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
        );

        cleanup(&path);
    }

    #[test]
    fn exec_ctr_fix_apply_rejects_severely_overlong_first_paragraph_before_write() {
        let path = test_dir();
        setup_project(&path);

        let file_path = std::path::Path::new(&path)
            .join("content")
            .join("001_test_article.mdx");
        let original = std::fs::read_to_string(&file_path).unwrap();
        let overlong = (1..=66)
            .map(|i| format!("word{}", i))
            .collect::<Vec<_>>()
            .join(" ");

        let patch = serde_json::json!({
            "article_id": 1,
            "file": "content/001_test_article.mdx",
            "changes": {
                "first_paragraph": overlong
            }
        });

        let task = crate::models::task::Task {
            id: "task-fix-overlong".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix overlong snippet test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_apply(&task, &path, Some(&patch.to_string()));
        assert!(!result.success, "Overlong snippet should be rejected");
        assert!(
            result.message.contains("first_paragraph is 66 words"),
            "Expected word-count validation failure, got: {}",
            result.message
        );

        let unchanged = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(
            original, unchanged,
            "File should not be modified when patch validation fails"
        );

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
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix corrupt test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let original = std::fs::read_to_string(content_dir.join("minimal.mdx")).unwrap();

        let result = exec_ctr_fix_apply(&task, &path, Some(&patch.to_string()));
        assert!(
            !result.success,
            "Should reject invalid patch before writing"
        );
        assert!(
            result.message.contains("invalid CtrFixPatch"),
            "Expected patch validation failure, got: {}",
            result.message
        );

        // Original should remain untouched because validation failed before writing.
        let restored = std::fs::read_to_string(content_dir.join("minimal.mdx")).unwrap();
        assert_eq!(original, restored, "Original should remain unchanged");

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
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix missing test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_apply(&task, &path, Some(&patch.to_string()));
        assert!(!result.success, "Should fail for missing file");
        assert!(
            result.message.contains("File not found"),
            "Expected file not found, got: {}",
            result.message
        );

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
        std::fs::write(
            std::path::Path::new(&path)
                .join("content")
                .join("001_test_article.mdx"),
            mdx,
        )
        .unwrap();

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
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
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
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_verify_fix(&task, &path);
        assert!(
            result.success,
            "Verification should pass: {}",
            result.message
        );
        let report: CtrFixVerificationReport =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
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
        std::fs::write(
            std::path::Path::new(&path)
                .join("content")
                .join("001_test_article.mdx"),
            mdx,
        )
        .unwrap();

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
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
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
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_verify_fix(&task, &path);
        assert!(!result.success, "Verification should find issues");
        let report: CtrFixVerificationReport =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(report.overall_status, "partial");
        let meta_check = report
            .checks
            .iter()
            .find(|c| c.check_type == "description")
            .unwrap();
        assert_eq!(meta_check.status, "fail");
        assert!(
            meta_check.detail.as_ref().unwrap().contains("130"),
            "Should mention 130 char minimum: {:?}",
            meta_check.detail
        );

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
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Parent Audit".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_build_context".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_audit".to_string()),
                content: Some(
                    serde_json::json!({
                        "total_articles": 2,
                        "articles": [
                            {
                                "id": 1,
                                "url_slug": "test-article",
                                "file": "content/001_test_article.mdx",
                                "target_keyword": "test article",
                                "issues_detected": {
                                    "file_not_found": false,
                                    "title_too_long": true,
                                    "meta_too_short": false,
                                    "snippet_suboptimal": false,
                                    "missing_faq_schema": false
                                }
                            },
                            {
                                "id": 2,
                                "url_slug": "another-article",
                                "file": "content/002_another_article.mdx",
                                "target_keyword": "another article",
                                "issues_detected": {
                                    "file_not_found": false,
                                    "title_too_long": false,
                                    "meta_too_short": true,
                                    "snippet_suboptimal": false,
                                    "missing_faq_schema": false
                                }
                            }
                        ]
                    })
                    .to_string(),
                ),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
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
            clarity_project_id: None,
        };
        crate::engine::task_store::create_project(&conn, &project).unwrap();
        crate::engine::task_store::create_task(&conn, &parent_task).unwrap();

        let ids = create_ctr_fix_tasks(&conn, &parent_task, &path);
        assert_eq!(ids.len(), 2, "Should create 2 fix tasks, got {}", ids.len());

        // Verify tasks are correct type and have ctr_context artifact
        for id in &ids {
            let task = crate::engine::task_store::get_task(&conn, id).unwrap();
            assert_eq!(task.task_type, "fix_ctr_article");
            let has_context = task.artifacts.iter().any(|a| a.key == "ctr_context");
            assert!(
                has_context,
                "fix_ctr_article task should have ctr_context artifact"
            );
        }

        cleanup(&path);
    }

    #[test]
    fn create_ctr_fix_tasks_reuses_existing_task_for_same_article_state() {
        let path = test_dir();
        setup_project(&path);
        let conn = test_db();
        insert_test_project(&conn, &path);

        let parent_one = ctr_parent_task("parent-one", ctr_context_for_article("hash-one"));
        crate::engine::task_store::create_task(&conn, &parent_one).unwrap();

        let first_ids = create_ctr_fix_tasks(&conn, &parent_one, &path);
        assert_eq!(first_ids.len(), 1);

        let parent_two = ctr_parent_task("parent-two", ctr_context_for_article("hash-one"));
        crate::engine::task_store::create_task(&conn, &parent_two).unwrap();

        let second_ids = create_ctr_fix_tasks(&conn, &parent_two, &path);
        assert_eq!(second_ids, first_ids);

        let fix_task_count = crate::engine::task_store::list_tasks(&conn, "proj-test")
            .unwrap()
            .into_iter()
            .filter(|task| task.task_type == "fix_ctr_article")
            .count();
        assert_eq!(
            fix_task_count, 1,
            "repeat audit should not create duplicate fix task"
        );

        cleanup(&path);
    }

    #[test]
    fn create_ctr_fix_tasks_allows_new_task_after_article_state_changes() {
        let path = test_dir();
        setup_project(&path);
        let conn = test_db();
        insert_test_project(&conn, &path);

        let parent_one = ctr_parent_task("parent-one", ctr_context_for_article("hash-one"));
        crate::engine::task_store::create_task(&conn, &parent_one).unwrap();

        let first_ids = create_ctr_fix_tasks(&conn, &parent_one, &path);
        assert_eq!(first_ids.len(), 1);
        crate::engine::task_store::update_task_status(
            &conn,
            &first_ids[0],
            crate::models::task::TaskStatus::Done,
        )
        .unwrap();

        let parent_two = ctr_parent_task("parent-two", ctr_context_for_article("hash-two"));
        crate::engine::task_store::create_task(&conn, &parent_two).unwrap();

        let second_ids = create_ctr_fix_tasks(&conn, &parent_two, &path);
        assert_eq!(second_ids.len(), 1);
        assert_ne!(second_ids[0], first_ids[0]);

        let fix_task_count = crate::engine::task_store::list_tasks(&conn, "proj-test")
            .unwrap()
            .into_iter()
            .filter(|task| task.task_type == "fix_ctr_article")
            .count();
        assert_eq!(
            fix_task_count, 2,
            "changed article state should be eligible for new fix work"
        );

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
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix complex frontmatter test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_recommendations".to_string(),
                path: None,
                artifact_type: None,
                source: None,
                content: Some(
                    serde_json::json!({
                        "article_id": 1,
                        "file": "content/test_article.mdx",
                        "target_keyword": "test article",
                        "fixes": [
                            { "type": "TitleRewrite", "recommended": "New Title" },
                            { "type": "MetaDescription", "recommended": "New desc" },
                            { "type": "SnippetBait", "recommended": "New paragraph" }
                        ]
                    })
                    .to_string(),
                ),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_apply(&task, &path, Some(&patch.to_string()));
        assert!(result.success, "CTR fix apply failed: {}", result.message);

        // Read the modified file
        let modified = std::fs::read_to_string(&file_path).unwrap();

        // Title and description should be updated
        assert!(
            modified.contains("title: \"New Title\""),
            "Title was not updated"
        );
        assert!(
            modified.contains("description: \"This is a very good meta description"),
            "Description was not updated"
        );

        // Alias should be removed
        assert!(
            !modified.contains("metaDescription:"),
            "metaDescription alias was not removed"
        );

        // Complex YAML must be preserved
        assert!(modified.contains("faq:"), "FAQ list was destroyed");
        assert!(
            modified.contains("  - question: \"What is this?\""),
            "FAQ question 1 was destroyed"
        );
        assert!(
            modified.contains("  - question: \"Why?\""),
            "FAQ question 2 was destroyed"
        );
        assert!(
            modified.contains("# AI SEO: FAQ Schema"),
            "Comment was destroyed"
        );
        assert!(
            modified.contains("citations:"),
            "Citations list was destroyed"
        );
        assert!(
            modified.contains("  - source: \"Example\""),
            "Citation source was destroyed"
        );

        // First paragraph should be replaced
        assert!(
            !modified.contains("This is the first paragraph that should be replaced"),
            "First paragraph was not replaced"
        );
        assert!(
            modified.contains("One two three four five six seven"),
            "New first paragraph is missing"
        );

        cleanup(&path);
    }

    /// Articles without detected issues are skipped; articles with issues get individual tasks.
    #[test]
    fn create_ctr_fix_tasks_skips_healthy_articles() {
        let path = test_dir();
        setup_project(&path);
        let conn = test_db();

        let parent_task = crate::models::task::Task {
            id: "parent-skip".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "ctr_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Skip Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_build_context".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_audit".to_string()),
                content: Some(
                    serde_json::json!({
                        "total_articles": 3,
                        "articles": [
                            {
                                "id": 1,
                                "url_slug": "test-article",
                                "file": "content/001_test_article.mdx",
                                "target_keyword": "test article",
                                "issues_detected": {
                                    "file_not_found": false,
                                    "title_too_long": true,
                                    "meta_too_short": false,
                                    "snippet_suboptimal": false,
                                    "missing_faq_schema": false
                                }
                            },
                            {
                                "id": 2,
                                "url_slug": "another-article",
                                "file": "content/002_another_article.mdx",
                                "target_keyword": "another article",
                                "issues_detected": {
                                    "file_not_found": false,
                                    "title_too_long": false,
                                    "meta_too_short": false,
                                    "snippet_suboptimal": false,
                                    "missing_faq_schema": false
                                }
                            },
                            {
                                "id": 999,
                                "url_slug": "missing-article",
                                "file": "",
                                "target_keyword": "",
                                "issues_detected": {
                                    "file_not_found": true,
                                    "title_too_long": false,
                                    "meta_too_short": false,
                                    "snippet_suboptimal": false,
                                    "missing_faq_schema": false
                                }
                            }
                        ]
                    })
                    .to_string(),
                ),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
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
            clarity_project_id: None,
        };
        crate::engine::task_store::create_project(&conn, &project).unwrap();
        crate::engine::task_store::create_task(&conn, &parent_task).unwrap();

        let ids = create_ctr_fix_tasks(&conn, &parent_task, &path);
        // Article 1 has title issue, article 2 is healthy, article 999 has file_not_found issue.
        assert_eq!(
            ids.len(),
            2,
            "Should create exactly 2 fix tasks (skip healthy article 2), got {}",
            ids.len()
        );

        cleanup(&path);
    }

    #[test]
    fn apply_prefers_ctr_fix_patch_artifact() {
        let path = test_dir();
        setup_project(&path);

        // Artifact contains a valid patch; legacy raw contains garbage
        let artifact_patch = serde_json::json!({
            "article_id": 1,
            "file": "content/001_test_article.mdx",
            "changes": {
                "title": "Artifact Title"
            }
        });

        let task = crate::models::task::Task {
            id: "task-artifact".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Artifact test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_fix_patch".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_fix_generate".to_string()),
                content: Some(artifact_patch.to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_apply(&task, &path, Some("this is garbage not json"));
        assert!(result.success, "Apply failed: {}", result.message);

        let content = std::fs::read_to_string(
            std::path::Path::new(&path)
                .join("content")
                .join("001_test_article.mdx"),
        )
        .unwrap();
        assert!(
            content.contains("Artifact Title"),
            "Should use artifact patch, not legacy raw"
        );

        cleanup(&path);
    }

    #[test]
    fn apply_falls_back_to_legacy_raw_output() {
        let path = test_dir();
        setup_project(&path);

        let legacy_patch = serde_json::json!({
            "article_id": 1,
            "file": "content/001_test_article.mdx",
            "changes": {
                "title": "Legacy Title"
            }
        });

        let task = crate::models::task::Task {
            id: "task-legacy".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Legacy test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_apply(&task, &path, Some(&legacy_patch.to_string()));
        assert!(result.success, "Apply failed: {}", result.message);

        let content = std::fs::read_to_string(
            std::path::Path::new(&path)
                .join("content")
                .join("001_test_article.mdx"),
        )
        .unwrap();
        assert!(
            content.contains("Legacy Title"),
            "Should fall back to legacy raw output"
        );

        cleanup(&path);
    }

    #[test]
    fn validate_patch_against_recommendation_mismatch() {
        let rec = crate::models::ctr::CtrRecommendation {
            article_id: 1,
            url_slug: "test".to_string(),
            file: "content/test.mdx".to_string(),
            priority: None,
            expected_ctr_improvement: None,
            target_keyword: "test keyword".to_string(),
            fixes: vec![crate::models::ctr::CtrFix {
                fix_type: crate::models::ctr::CtrFixType::TitleRewrite,
                current: Some("Old Title".to_string()),
                recommended: serde_json::json!("New Title"),
                reason: None,
            }],
        };

        let mdx = r#"---
title: "Old Title That Is Way Way Way Way Way Way Way Way Way Way Way Way Way Way Way Way Too Long"
description: "desc"
date: "2024-01-01"
---

# Old Title That Is Way Way Way Way Way Way Way Way Way Way Way Way Way Way Way Way Too Long

This is the first paragraph of the test article. It contains some content.
"#;

        // Wrong article_id
        let patch_bad_id = crate::models::ctr::CtrFixPatch {
            article_id: 99,
            file: "content/test.mdx".to_string(),
            error: None,
            changes: crate::models::ctr::CtrFixPatchChanges {
                title: Some("New Title".to_string()),
                ..Default::default()
            },
        };
        let errors = super::patch::validate_patch_against_recommendation(&patch_bad_id, &rec, mdx);
        assert!(
            errors.iter().any(|e| e.contains("article_id")),
            "Should error on article_id mismatch: {:?}",
            errors
        );

        // Missing requested title fix
        let patch_missing_title = crate::models::ctr::CtrFixPatch {
            article_id: 1,
            file: "content/test.mdx".to_string(),
            error: None,
            changes: crate::models::ctr::CtrFixPatchChanges::default(),
        };
        let errors =
            super::patch::validate_patch_against_recommendation(&patch_missing_title, &rec, mdx);
        assert!(
            errors.iter().any(|e| e.contains("title_rewrite")),
            "Should error when requested title fix is missing: {:?}",
            errors
        );

        // Unrequested description change
        let patch_unrequested = crate::models::ctr::CtrFixPatch {
            article_id: 1,
            file: "content/test.mdx".to_string(),
            error: None,
            changes: crate::models::ctr::CtrFixPatchChanges {
                title: Some("New Title".to_string()),
                description: Some("New desc".to_string()),
                ..Default::default()
            },
        };
        let errors =
            super::patch::validate_patch_against_recommendation(&patch_unrequested, &rec, mdx);
        assert!(
            errors
                .iter()
                .any(|e| e.contains("meta_description was not requested")),
            "Should error on unrequested description change: {:?}",
            errors
        );
    }
