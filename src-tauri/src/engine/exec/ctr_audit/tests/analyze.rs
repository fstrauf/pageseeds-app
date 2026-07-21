    use super::*;

    /// When all articles already have good titles, meta, snippets, and CTR at or
    /// above the position-expected level, the audit should return 0 articles and
    /// the analyze step should skip the agent.
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
                    "gsc": { "impressions": 10000.0, "clicks": 60.0, "ctr": 0.006, "avg_position": 8.5 }
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

