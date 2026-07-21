    use super::*;

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

    fn article_json(
        id: i64,
        slug: &str,
        clicks_lost: f64,
        issues: serde_json::Value,
        top_queries: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "url_slug": slug,
            "file": format!("content/{:03}_{}.mdx", id, slug),
            "content_hash": format!("hash-{}", id),
            "target_keyword": "test",
            "clicks_lost": clicks_lost,
            "has_frontmatter_faq": false,
            "issues_detected": issues,
            "top_queries": top_queries,
        })
    }

    fn title_issue() -> serde_json::Value {
        serde_json::json!({
            "file_not_found": false,
            "title_too_long": true,
            "meta_too_short": false,
            "snippet_suboptimal": false,
            "missing_faq_schema": false
        })
    }

    fn faq_only_issue() -> serde_json::Value {
        serde_json::json!({
            "file_not_found": false,
            "title_too_long": false,
            "meta_too_short": false,
            "snippet_suboptimal": false,
            "missing_faq_schema": true
        })
    }

    #[test]
    fn priority_for_rank_maps_clicks_lost_deciles() {
        use crate::models::task::Priority;

        // 20 articles → top decile = 2 slots
        assert_eq!(priority_for_rank(0, 20, 100.0), Priority::High);
        assert_eq!(priority_for_rank(1, 20, 90.0), Priority::High);
        assert_eq!(priority_for_rank(2, 20, 80.0), Priority::Medium);
        assert_eq!(priority_for_rank(19, 20, 0.5), Priority::Medium);

        // Small sets always promote the single highest-impact article
        assert_eq!(priority_for_rank(0, 3, 10.0), Priority::High);
        assert_eq!(priority_for_rank(1, 3, 5.0), Priority::Medium);

        // Zero clicks_lost (pure format violations) → Low
        assert_eq!(priority_for_rank(0, 3, 0.0), Priority::Low);
        assert_eq!(priority_for_rank(5, 20, 0.0), Priority::Low);
    }

    #[test]
    fn create_ctr_fix_tasks_assigns_priority_from_clicks_lost() {
        use crate::models::task::Priority;

        let path = test_dir();
        setup_project(&path);
        let conn = test_db();
        insert_test_project(&conn, &path);

        let context = serde_json::json!({
            "total_articles": 3,
            "articles": [
                article_json(1, "high-impact", 100.0, title_issue(), serde_json::Value::Null),
                article_json(2, "mid-impact", 50.0, title_issue(), serde_json::Value::Null),
                article_json(3, "format-only", 0.0, title_issue(), serde_json::Value::Null),
            ]
        });
        let parent = ctr_parent_task("parent-priority", context);
        crate::engine::task_store::create_task(&conn, &parent).unwrap();

        let ids = create_ctr_fix_tasks(&conn, &parent, &path);
        assert_eq!(ids.len(), 3);

        let priority_of = |slug: &str| {
            crate::engine::task_store::list_tasks(&conn, "proj-test")
                .unwrap()
                .into_iter()
                .find(|t| t.task_type == "fix_ctr_article" && t.title == Some(format!("CTR fix: {}", slug)))
                .map(|t| t.priority)
                .unwrap_or_else(|| panic!("no fix task for {}", slug))
        };

        assert_eq!(priority_of("high-impact"), Priority::High);
        assert_eq!(priority_of("mid-impact"), Priority::Medium);
        assert_eq!(priority_of("format-only"), Priority::Low);

        cleanup(&path);
    }

    #[test]
    fn create_ctr_fix_tasks_faq_requires_question_intent() {
        let path = test_dir();
        setup_project(&path);
        let conn = test_db();
        insert_test_project(&conn, &path);

        let context = serde_json::json!({
            "total_articles": 3,
            "articles": [
                // Only issue is missing FAQ, no query data → NOT spawn-worthy
                article_json(1, "faq-no-queries", 10.0, faq_only_issue(), serde_json::Value::Null),
                // Only issue is missing FAQ, non-question queries → NOT spawn-worthy
                article_json(2, "faq-generic-queries", 10.0, faq_only_issue(), serde_json::json!([
                    { "query": "option selling", "intent": "generic" }
                ])),
                // Only issue is missing FAQ, question-intent query present → spawn-worthy
                article_json(3, "faq-question-queries", 10.0, faq_only_issue(), serde_json::json!([
                    { "query": "what is a cash secured put", "intent": "question" }
                ])),
            ]
        });
        let parent = ctr_parent_task("parent-faq-gate", context);
        crate::engine::task_store::create_task(&conn, &parent).unwrap();

        let ids = create_ctr_fix_tasks(&conn, &parent, &path);
        assert_eq!(
            ids.len(),
            1,
            "only the question-intent article should get an FAQ-driven fix task"
        );

        let task = crate::engine::task_store::get_task(&conn, &ids[0]).unwrap();
        assert_eq!(task.title.as_deref(), Some("CTR fix: faq-question-queries"));

        cleanup(&path);
    }

