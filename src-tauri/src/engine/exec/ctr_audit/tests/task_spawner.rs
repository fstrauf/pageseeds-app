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

