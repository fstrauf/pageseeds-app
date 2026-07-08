    use super::*;
    use rusqlite::Connection;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn make_task(id: &str, project_id: &str, task_type: &str, status: TaskStatus) -> Task {
        let now = chrono::Utc::now().to_rfc3339();
        Task {
            id: id.to_string(),
            project_id: project_id.to_string(),
            task_type: task_type.to_string(),
            phase: "investigation".to_string(),
            status,
            priority: Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::UserEnqueue,
            review_surface: crate::models::task::TaskReviewSurface::None,
            follow_up_policy: crate::models::task::FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test Task".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: now.clone(),
            updated_at: now,
            not_before: None,
        }
    }

    /// Creating new tasks increases done count in get_project_overview.
    #[test]
    fn project_overview_task_counts_update_after_creating_tasks() {
        let conn = in_memory_db();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('p1', 'Test', '/tmp/test', 1, 'workspace')",
            [],
        ).unwrap();

        // Insert 3 done tasks from a previous audit
        create_task(&conn, &make_task("t1", "p1", "content_review", TaskStatus::Done)).unwrap();
        create_task(&conn, &make_task("t2", "p1", "indexing_health_campaign", TaskStatus::Done)).unwrap();
        create_task(&conn, &make_task("t3", "p1", "fix_content_article", TaskStatus::Done)).unwrap();

        // Insert 2 todo tasks
        create_task(&conn, &make_task("t4", "p1", "ctr_audit", TaskStatus::Todo)).unwrap();
        create_task(&conn, &make_task("t5", "p1", "fix_content_article", TaskStatus::Todo)).unwrap();

        let before = get_project_overview(&conn, "p1").unwrap();
        assert_eq!(before.tasks.total, 5);
        assert_eq!(before.tasks.done, 3);
        assert_eq!(before.tasks.todo, 2);

        // Simulate a new "Run Full Audit" creating 2 new tasks
        create_task(&conn, &make_task("t6", "p1", "content_review", TaskStatus::Todo)).unwrap();
        create_task(&conn, &make_task("t7", "p1", "indexing_health_campaign", TaskStatus::Todo)).unwrap();

        let after = get_project_overview(&conn, "p1").unwrap();
        assert_eq!(after.tasks.total, 7, "total should increase by 2");
        assert_eq!(after.tasks.todo, 4, "todo should increase by 2");
        assert_eq!(after.tasks.done, 3, "done should remain 3");
    }

    /// load_project_slug_set normalizes prefixed/uppercase/raw url_slugs to a
    /// canonical lowercased form so callers can match without re-implementing
    /// slug cleanup.
    #[test]
    fn load_project_slug_set_normalizes_url_slugs() {
        let conn = in_memory_db();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('p1', 'Test', '/tmp/test', 1, 'workspace')",
            [],
        ).unwrap();

        for (id, slug) in [
            (1, "hub-coffee"),
            (2, "/blog/Best-Home-Coffee-Roaster/"),
            (3, "blog/ethiopia-coffee-regions"),
            (4, "001_my_post"),
        ] {
            conn.execute(
                "INSERT INTO articles (
                    id, project_id, title, url_slug, file, status,
                    content_gaps_addressed, target_volume, word_count, review_count
                 ) VALUES (?1, 'p1', ?2, ?3, 'article.mdx', 'draft', '[]', 0, 0, 0)",
                rusqlite::params![id, format!("Article {}", id), slug],
            ).unwrap();
        }

        let slugs = load_project_slug_set(&conn, "p1").unwrap();
        assert!(slugs.contains("hub-coffee"));
        assert!(slugs.contains("best-home-coffee-roaster"));
        assert!(slugs.contains("ethiopia-coffee-regions"));
        assert!(slugs.contains("my-post"));
        assert!(!slugs.contains("/blog/hub-coffee"));
    }

    /// Completing the new audit tasks increases done and reduces todo.
    #[test]
    fn project_overview_reflects_completed_tasks() {
        let conn = in_memory_db();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('p1', 'Test', '/tmp/test', 1, 'workspace')",
            [],
        ).unwrap();

        // Initial state: 3 done tasks
        for i in 1..=3 {
            create_task(&conn, &make_task(&format!("t{}", i), "p1", "content_review", TaskStatus::Done)).unwrap();
        }

        let before = get_project_overview(&conn, "p1").unwrap();
        assert_eq!(before.tasks.done, 3);

        // Create 2 new audit tasks as todo
        let t4 = make_task("t4", "p1", "content_review", TaskStatus::Todo);
        let t5 = make_task("t5", "p1", "indexing_health_campaign", TaskStatus::Todo);
        create_task(&conn, &t4).unwrap();
        create_task(&conn, &t5).unwrap();

        let mid = get_project_overview(&conn, "p1").unwrap();
        assert_eq!(mid.tasks.todo, 2, "todo should be 2 after creating new tasks");
        assert_eq!(mid.tasks.total, 5);

        // Simulate queue runner completing both tasks
        update_task_status(&conn, "t4", TaskStatus::Done).unwrap();
        update_task_status(&conn, "t5", TaskStatus::Done).unwrap();

        let after = get_project_overview(&conn, "p1").unwrap();
        assert_eq!(after.tasks.done, 5, "done should be 5 after completing new tasks");
        assert_eq!(after.tasks.todo, 0, "todo should be 0 after completing all");
    }
