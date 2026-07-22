    use super::*;

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

    fn task_with_rec(rec: &crate::models::ctr::CtrRecommendation) -> crate::models::task::Task {
        crate::models::task::Task {
            id: "task-try-patch".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("try patch".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_recommendations".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_audit".to_string()),
                content: Some(serde_json::to_string(rec).unwrap()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn base_mdx_needing_title_meta() -> &'static str {
        r#"---
title: "Test Article | Brand | Brand -- Tagline That Makes Title Too Long"
description: "A short desc"
date: "2024-01-01"
---

# Test Article

This is the first paragraph of the test article. It contains some content.
"#
    }

    #[test]
    fn try_patch_from_recommendation_concrete_title_and_meta() {
        let title = "Best CSP Stocks Guide";
        // 120–155 chars (META_MIN_LEN..META_MAX_LEN)
        let meta = "Discover the best cash-secured put stocks for consistent income. \
Compare risk, premiums, and entry strategies in this practical guide for option sellers.";
        assert!(
            (crate::engine::exec::audit_health::META_MIN_LEN
                ..=crate::engine::exec::audit_health::META_MAX_LEN)
                .contains(&meta.chars().count()),
            "test meta length {}",
            meta.chars().count()
        );
        assert!(title.chars().count() <= crate::engine::exec::audit_health::TITLE_MAX_LEN);

        let rec = crate::models::ctr::CtrRecommendation {
            article_id: 42,
            url_slug: "best-csp-stocks".to_string(),
            file: "content/best_csp_stocks.mdx".to_string(),
            priority: None,
            expected_ctr_improvement: None,
            target_keyword: "csp stocks".to_string(),
            fixes: vec![
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::TitleRewrite,
                    current: Some("Old long title".to_string()),
                    recommended: serde_json::json!(title),
                    reason: None,
                },
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::MetaDescription,
                    current: Some("A short desc".to_string()),
                    recommended: serde_json::json!(meta),
                    reason: None,
                },
            ],
        };
        let task = task_with_rec(&rec);
        let mdx = base_mdx_needing_title_meta();

        let patch = super::patch::try_patch_from_recommendation(&rec, mdx, &task)
            .expect("concrete title+meta should map deterministically");
        assert_eq!(patch.article_id, 42);
        assert_eq!(patch.file, "content/best_csp_stocks.mdx");
        assert_eq!(patch.changes.title.as_deref(), Some(title));
        assert_eq!(patch.changes.description.as_deref(), Some(meta));
        assert!(patch.changes.first_paragraph.is_none());
        assert!(patch.changes.faq_questions.is_none());
    }

    #[test]
    fn try_patch_from_recommendation_guidance_or_invalid_returns_none() {
        let mdx = base_mdx_needing_title_meta();

        // Non-string recommended for title
        let rec_non_string = crate::models::ctr::CtrRecommendation {
            article_id: 1,
            url_slug: "test".to_string(),
            file: "content/test.mdx".to_string(),
            priority: None,
            expected_ctr_improvement: None,
            target_keyword: "test".to_string(),
            fixes: vec![crate::models::ctr::CtrFix {
                fix_type: crate::models::ctr::CtrFixType::TitleRewrite,
                current: Some("Old".to_string()),
                recommended: serde_json::json!({"instruction": "rewrite for SERP"}),
                reason: None,
            }],
        };
        let task = task_with_rec(&rec_non_string);
        assert!(
            super::patch::try_patch_from_recommendation(&rec_non_string, mdx, &task).is_none(),
            "non-string recommended must not map"
        );

        // String that fails validation (meta far too short)
        let rec_short_meta = crate::models::ctr::CtrRecommendation {
            article_id: 1,
            url_slug: "test".to_string(),
            file: "content/test.mdx".to_string(),
            priority: None,
            expected_ctr_improvement: None,
            target_keyword: "test".to_string(),
            fixes: vec![
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::TitleRewrite,
                    current: Some("Old".to_string()),
                    recommended: serde_json::json!("Good Title"),
                    reason: None,
                },
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::MetaDescription,
                    current: Some("short".to_string()),
                    recommended: serde_json::json!("Rewrite meta to be more compelling and include the keyword."),
                    reason: None,
                },
            ],
        };
        let task = task_with_rec(&rec_short_meta);
        assert!(
            super::patch::try_patch_from_recommendation(&rec_short_meta, mdx, &task).is_none(),
            "guidance/short meta that fails validation must return None"
        );
    }

    #[test]
    fn try_patch_from_recommendation_faq_questions_only_returns_none() {
        let mdx = r#"---
title: "Good Enough Title"
description: "A short desc"
date: "2024-01-01"
---

# Heading

Body paragraph here.
"#;

        // Array of plain strings (questions only)
        let rec_strings = crate::models::ctr::CtrRecommendation {
            article_id: 1,
            url_slug: "test".to_string(),
            file: "content/test.mdx".to_string(),
            priority: None,
            expected_ctr_improvement: None,
            target_keyword: "test".to_string(),
            fixes: vec![crate::models::ctr::CtrFix {
                fix_type: crate::models::ctr::CtrFixType::FaqSchema,
                current: None,
                recommended: serde_json::json!([
                    "What is cash secured put?",
                    "How do CSPs work?",
                    "When should I sell puts?"
                ]),
                reason: None,
            }],
        };
        let task = task_with_rec(&rec_strings);
        assert!(
            super::patch::try_patch_from_recommendation(&rec_strings, mdx, &task).is_none(),
            "FAQ questions-only strings must return None"
        );

        // Objects missing answers
        let rec_missing_answers = crate::models::ctr::CtrRecommendation {
            article_id: 1,
            url_slug: "test".to_string(),
            file: "content/test.mdx".to_string(),
            priority: None,
            expected_ctr_improvement: None,
            target_keyword: "test".to_string(),
            fixes: vec![crate::models::ctr::CtrFix {
                fix_type: crate::models::ctr::CtrFixType::FaqSchema,
                current: None,
                recommended: serde_json::json!([
                    { "question": "What is cash secured put?" },
                    { "question": "How do CSPs work?" },
                    { "question": "When should I sell puts?" }
                ]),
                reason: None,
            }],
        };
        let task = task_with_rec(&rec_missing_answers);
        assert!(
            super::patch::try_patch_from_recommendation(&rec_missing_answers, mdx, &task).is_none(),
            "FAQ objects without answers must return None"
        );
    }
