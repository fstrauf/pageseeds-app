use super::topic_health::classify_topic_health;
    use super::*;

    #[test]
    fn classify_topic_health_promising_when_quality_and_traffic_signals_are_strong() {
        let (status, score) = classify_topic_health(75, 2, 5.0, 500.0);
        assert_eq!(status, "promising");
        assert!(score.is_some());
    }

    #[test]
    fn classify_topic_health_promising_with_high_impressions_even_without_clicks() {
        let (status, score) = classify_topic_health(80, 1, 0.0, 1200.0);
        assert_eq!(status, "promising");
        assert!(score.is_some());
    }

    #[test]
    fn classify_topic_health_depleted_when_quality_and_impressions_are_low() {
        let (status, _score) = classify_topic_health(40, 2, 0.0, 50.0);
        assert_eq!(status, "depleted");
        // Any clicks should prevent depleted classification.
        let (status_with_clicks, _) = classify_topic_health(40, 2, 1.0, 50.0);
        assert_eq!(status_with_clicks, "unproven");
        // Higher impressions should prevent depleted classification.
        let (status_with_impressions, _) = classify_topic_health(40, 2, 0.0, 150.0);
        assert_eq!(status_with_impressions, "unproven");
    }

    #[test]
    fn classify_topic_health_unproven_for_mixed_or_missing_signals() {
        let (status, score) = classify_topic_health(60, 2, 0.0, 500.0);
        assert_eq!(status, "unproven");
        assert!(score.is_some());

        // No quality data but some traffic → still unproven (not enough evidence either way).
        let (no_quality_status, no_quality_score) = classify_topic_health(0, 0, 0.0, 500.0);
        assert_eq!(no_quality_status, "unproven");
        assert!(no_quality_score.is_none());
    }

    #[test]
    fn classify_topic_health_signal_score_combines_quality_clicks_and_impressions() {
        let (_, score) = classify_topic_health(70, 1, 3.0, 500.0);
        // 70 + (3 * 10) + (500 / 100) = 70 + 30 + 5 = 105
        assert_eq!(score, Some(105.0));
    }

    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, TaskReviewSurface, TaskRun, TaskRunPolicy,
        TaskStatus,
    };

    fn make_task() -> Task {
        Task {
            id: "test-task".to_string(),
            task_type: "write_article".to_string(),
            phase: "implementation".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::Optional,
            title: None,
            description: None,
            project_id: "proj1".to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        }
    }

    #[test]
    fn content_task_target_keyword_reads_keyword_line() {
        let mut task = make_task();
        task.description =
            Some("Target keyword: gamma scalping strategy\nKD: 35\nVolume: 3000".to_string());
        assert_eq!(
            content_task_target_keyword(&task).as_deref(),
            Some("gamma scalping strategy")
        );
    }

    #[test]
    fn content_task_target_keyword_skips_empty_and_missing() {
        let mut task = make_task();
        assert!(content_task_target_keyword(&task).is_none());

        task.description = Some("KD: 35\nVolume: 3000".to_string());
        assert!(content_task_target_keyword(&task).is_none());

        task.description = Some("Target keyword:\nKD: 35".to_string());
        assert!(content_task_target_keyword(&task).is_none());
    }

    #[test]
    fn strip_content_task_title_prefix_strips_known_prefixes() {
        assert_eq!(
            strip_content_task_title_prefix("Write article: delta hedging"),
            "delta hedging"
        );
        assert_eq!(
            strip_content_task_title_prefix("Write territory article: theta decay"),
            "theta decay"
        );
        assert_eq!(
            strip_content_task_title_prefix("Create hub: options greeks"),
            "options greeks"
        );
        assert_eq!(
            strip_content_task_title_prefix("Refresh hub: options greeks"),
            "options greeks"
        );
        // No-space variant (hub titles are stripped with bare prefixes upstream).
        assert_eq!(
            strip_content_task_title_prefix("Create hub:options greeks"),
            "options greeks"
        );
        // Unknown prefixes and bare titles are returned trimmed but intact.
        assert_eq!(
            strip_content_task_title_prefix("Cluster and link: delta hedging"),
            "Cluster and link: delta hedging"
        );
        assert_eq!(strip_content_task_title_prefix("plain title"), "plain title");
    }

    /// Issue #152: successful `fix_ctr_article` records a change event and must
    /// not spawn `ctr_outcome_review`.
    #[test]
    fn fix_ctr_article_records_change_event_without_outcome_review_spawn() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('proj1', 'Test', '/tmp/pa_ctr', 1, 'workspace')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles (
                id, project_id, title, url_slug, file, status, target_keyword,
                content_gaps_addressed, target_volume, word_count, review_count, content_hash
             ) VALUES (9, 'proj1', 'CTR Article', 'ctr-article', 'content/ctr.mdx',
                       'published', 'kw', '[]', 0, 100, 0, 'h')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ctr_rendered_page_audits (project_id, article_id, url, file, checked_at)
             VALUES ('proj1', 9, 'https://example.com/blog/ctr-article', 'content/ctr.mdx',
                     '2026-07-01T00:00:00Z')",
            [],
        )
        .unwrap();

        let mut task = make_task();
        task.id = "fix-ctr-1".to_string();
        task.task_type = "fix_ctr_article".to_string();
        task.project_id = "proj1".to_string();
        task.status = TaskStatus::Done;
        task.artifacts = vec![crate::models::task::TaskArtifact {
            key: "ctr_context".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: None,
            content: Some(
                serde_json::json!({
                    "articles": [{ "id": 9, "url_slug": "ctr-article" }]
                })
                .to_string(),
            ),
        }];
        crate::engine::task_store::create_task(&conn, &task).unwrap();

        let follow_ups = after_task_success(&PostTaskContext {
            conn: &conn,
            task: &task,
            project_path: "/tmp/pa_ctr",
            progress: &[],
        });

        assert!(
            follow_ups.is_empty()
                || !follow_ups.iter().any(|id| {
                    crate::engine::task_store::get_task(&conn, id)
                        .map(|t| t.task_type == "ctr_outcome_review")
                        .unwrap_or(false)
                }),
            "must not spawn ctr_outcome_review follow-ups: {:?}",
            follow_ups
        );
        let tasks = crate::engine::task_store::list_tasks(&conn, "proj1").unwrap();
        assert!(
            !tasks.iter().any(|t| t.task_type == "ctr_outcome_review"),
            "no ctr_outcome_review tasks should exist"
        );

        let outcomes = crate::db::list_ctr_outcomes(&conn, "proj1").unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].article_id, 9);
        assert_eq!(outcomes[0].fix_task_id, "fix-ctr-1");
        assert_eq!(outcomes[0].outcome_status, "pending");
        assert!(
            outcomes[0].deployed_at.is_none(),
            "deployed_at null until verify"
        );
    }
