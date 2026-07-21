    use super::*;

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

    fn test_task(id: &str) -> crate::models::task::Task {
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
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Write a project with a single article + MDX file; returns the temp path.
    fn setup_single_article_project(
        slug: &str,
        title: &str,
        meta: &str,
        first_paragraph: &str,
        gsc: serde_json::Value,
    ) -> String {
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
                    "url_slug": slug,
                    "title": title,
                    "target_keyword": "test keyword",
                    "file": "content/001_article.mdx",
                    "gsc": gsc
                }
            ]
        });
        std::fs::write(
            auto_dir.join("articles.json"),
            serde_json::to_string_pretty(&articles).unwrap(),
        )
        .unwrap();

        let mdx = format!(
            "---\ntitle: \"{}\"\ndescription: \"{}\"\ndate: \"2024-01-01\"\n---\n\n# {}\n\n{}\n\n## Section\n\nMore content.\n",
            title, meta, title, first_paragraph
        );
        std::fs::write(content_dir.join("001_article.mdx"), mdx).unwrap();
        path
    }

    /// A snippet that passes the linter: 45 words, contains the target keyword.
    fn good_snippet() -> String {
        format!("test keyword {}", "filler ".repeat(43))
    }

    /// A meta description that passes the linter: 130-155 chars.
    fn good_meta() -> String {
        "m".repeat(140)
    }

    #[test]
    fn test_ctr_underperforms_helper() {
        // position 8.5 → target 0.008; half of target is 0.004
        assert!(ctr_underperforms(0.001, 0.008));
        assert!(!ctr_underperforms(0.004, 0.008));
        assert!(!ctr_underperforms(0.006, 0.008));
        // No position expectation (target 0) → never underperforms
        assert!(!ctr_underperforms(0.0, 0.0));
    }

    #[test]
    fn test_well_formatted_article_admitted_on_ctr_underperformance() {
        let path = setup_single_article_project(
            "well-formatted",
            "Good Title",
            &good_meta(),
            &good_snippet(),
            // pos 8.5 → target 0.008; ctr 0.001 << 0.004 → underperforms
            serde_json::json!({ "impressions": 10000.0, "clicks": 10.0, "ctr": 0.001, "avg_position": 8.5 }),
        );

        let conn = test_db();
        let result = exec_ctr_build_context(&test_task("task-ctr"), &path, None, &conn);
        assert!(result.success, "build_context failed: {}", result.message);

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(
            output["total_articles"].as_i64().unwrap(),
            1,
            "well-formatted but underperforming article should be admitted"
        );

        let article = &output["articles"][0];
        let reasons: Vec<&str> = article["detection_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap())
            .collect();
        assert_eq!(
            reasons,
            vec!["ctr_underperformance"],
            "only CTR underperformance should be detected, got {:?}",
            reasons
        );
        assert!(article["clicks_lost"].as_f64().unwrap() > 0.0);
        cleanup(&path);
    }

    #[test]
    fn test_low_impressions_skipped_even_when_linter_fails() {
        let path = setup_single_article_project(
            "low-impressions",
            "Good Title",
            "short", // meta linter violation
            "too short", // snippet linter violation
            serde_json::json!({ "impressions": 100.0, "clicks": 0.0, "ctr": 0.0, "avg_position": 5.0 }),
        );

        let conn = test_db();
        let result = exec_ctr_build_context(&test_task("task-low"), &path, None, &conn);
        assert!(result.success);

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(
            output["total_articles"].as_i64().unwrap(),
            0,
            "article below the impressions floor must never enter the funnel"
        );
        assert!(
            result.message.contains("1 low-impressions"),
            "skip counter should be reported, got: {}",
            result.message
        );
        cleanup(&path);
    }

    #[test]
    fn test_check_article_health_faq_advisory() {
        let health = crate::engine::exec::audit_health::check_article_health(
            "Good Title",
            &good_meta(),
            &good_snippet(),
            "test keyword",
            false, // no FAQ schema
            true,
        );
        assert!(
            health.all_ok(),
            "FAQ-less article with passing core checks should be healthy"
        );
        assert!(
            !health.issues.contains(&"missing_faq_schema".to_string()),
            "missing FAQ must not appear in issues: {:?}",
            health.issues
        );
        assert!(health.issues.is_empty());
        assert!(!health.faq_ok, "faq_ok field stays computed (advisory)");
    }

