    use super::*;

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
        // Beyond normalize auto-trim window (max+8) so field is pruned, not applied.
        let overlong = (1..=80)
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
        // Invalid first_paragraph is pruned; with no other fields the apply fails
        // as empty rather than writing a bad intro.
        assert!(
            result.message.contains("first_paragraph is 80 words")
                || result.message.contains("no valid change fields"),
            "Expected word-count or empty-after-prune failure, got: {}",
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
            result.message.contains("invalid CtrFixPatch")
                || result.message.contains("no valid change fields"),
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

