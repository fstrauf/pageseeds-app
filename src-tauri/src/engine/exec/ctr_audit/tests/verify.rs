    use super::*;

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
            meta_check.detail.as_ref().unwrap().contains("120–155"),
            "Should mention 120–155 char range: {:?}",
            meta_check.detail
        );

        cleanup(&path);
    }

