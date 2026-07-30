use super::topic_health::classify_topic_health;
    use super::*;

    #[test]
    fn should_spawn_cluster_link_follow_up_residual_and_focus_rules() {
        // Cap
        assert!(!should_spawn_cluster_link_follow_up(5, 5, 3, true, 3));
        // Classic: orphans + progress
        assert!(should_spawn_cluster_link_follow_up(2, 0, 1, false, 1));
        // Zero-incoming residual + progress (origin C)
        assert!(should_spawn_cluster_link_follow_up(0, 3, 2, false, 1));
        // Pure zero-incoming without progress — no re-round
        assert!(!should_spawn_cluster_link_follow_up(0, 3, 0, false, 1));
        // Focus still zero forces re-round even with links_added == 0
        assert!(should_spawn_cluster_link_follow_up(0, 1, 0, true, 1));
        // Focus still zero with progress
        assert!(should_spawn_cluster_link_follow_up(0, 0, 1, true, 2));
        // Healthy
        assert!(!should_spawn_cluster_link_follow_up(0, 0, 0, false, 1));
        assert!(!should_spawn_cluster_link_follow_up(0, 0, 5, false, 1));
    }

    #[test]
    fn cluster_link_post_action_spawns_on_zero_incoming_residual() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('proj1', 'Test', '/tmp/pa_cl', 1, 'workspace')",
            [],
        )
        .unwrap();

        let mut task = make_task();
        task.id = "cluster-round-1".to_string();
        task.task_type = "cluster_and_link".to_string();
        task.project_id = "proj1".to_string();
        task.status = TaskStatus::Done;
        task.title = Some("Cluster and link: fresh article".to_string());
        task.artifacts = vec![
            crate::models::task::TaskArtifact {
                key: "focus_slug".to_string(),
                path: None,
                artifact_type: Some("text".to_string()),
                source: Some("write_spawn".to_string()),
                content: Some("fresh-article".to_string()),
            },
            crate::models::task::TaskArtifact {
                key: "cluster_link_apply".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: None,
                content: Some(
                    serde_json::json!({
                        "files_modified": 1,
                        "links_added": 2,
                        "orphans_remaining": 0,
                        "zero_incoming_remaining": 1,
                        "focus_still_zero_incoming": true,
                    })
                    .to_string(),
                ),
            },
        ];
        crate::engine::task_store::create_task(&conn, &task).unwrap();

        let follow_ups = after_task_success(&PostTaskContext {
            conn: &conn,
            task: &task,
            project_path: "/tmp/pa_cl",
            progress: &[],
        });
        assert_eq!(follow_ups.len(), 1, "expected residual follow-up: {:?}", follow_ups);
        let child = crate::engine::task_store::get_task(&conn, &follow_ups[0]).unwrap();
        assert_eq!(child.task_type, "cluster_and_link");
        assert!(
            child
                .title
                .as_deref()
                .unwrap_or("")
                .contains("zero-incoming"),
            "title should mention residual debt: {:?}",
            child.title
        );
        let focus = child
            .artifacts
            .iter()
            .find(|a| a.key == "focus_slug")
            .and_then(|a| a.content.as_deref());
        assert_eq!(focus, Some("fresh-article"), "focus_slug must propagate");
    }

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

    /// Nested after_task_success still spawns content_outcome_review (issue #23 / #203).
    #[test]
    fn write_article_after_success_spawns_content_outcome_review() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        let project_path = std::env::temp_dir().join(format!(
            "pageseeds-pa-outcome-{}",
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_dir_all(&project_path);
        std::fs::create_dir_all(project_path.join("content/blog")).unwrap();
        std::fs::write(
            project_path.join("content/blog/written-post.mdx"),
            "---\ntitle: Written\nslug: written-post\n---\n\n# Written\n\nbody\n",
        )
        .unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('proj1', 'Test', ?1, 1, 'workspace')",
            [project_path.to_str().unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles (
                id, project_id, title, url_slug, file, status, target_keyword,
                content_gaps_addressed, target_volume, word_count, review_count, content_hash
             ) VALUES (1, 'proj1', 'Written', 'written-post',
                       'content/blog/written-post.mdx', 'draft', 'kw', '[]', 0, 10, 0, 'h')",
            [],
        )
        .unwrap();

        let mut task = make_task();
        task.id = "write-nested-1".to_string();
        task.task_type = "write_article".to_string();
        task.project_id = "proj1".to_string();
        task.status = TaskStatus::Done;
        task.description = Some(
            "File: content/blog/written-post.mdx | Target keyword: written post".to_string(),
        );
        crate::engine::task_store::create_task(&conn, &task).unwrap();

        let follow_ups = after_task_success(&PostTaskContext {
            conn: &conn,
            task: &task,
            project_path: project_path.to_str().unwrap(),
            progress: &[],
        });

        let reviews: Vec<_> = follow_ups
            .iter()
            .filter_map(|id| crate::engine::task_store::get_task(&conn, id).ok())
            .filter(|t| t.task_type == "content_outcome_review")
            .collect();
        assert_eq!(
            reviews.len(),
            1,
            "nested write must spawn content_outcome_review; follow_ups={follow_ups:?}"
        );
        let review = &reviews[0];
        assert!(review.not_before.is_some());
        let art = review
            .artifacts
            .iter()
            .find(|a| a.key == "content_outcome_target")
            .expect("content_outcome_target");
        let v: serde_json::Value =
            serde_json::from_str(art.content.as_deref().unwrap_or("{}")).unwrap();
        assert_eq!(v["slug"].as_str(), Some("written-post"));
        assert_eq!(v["parent_task_id"].as_str(), Some("write-nested-1"));

        // Core helper is idempotent for same parent+slug.
        let again = spawn_content_outcome_review_for_slug(&conn, &task, "written-post");
        assert_eq!(again.as_deref(), Some(review.id.as_str()));

        let _ = std::fs::remove_dir_all(&project_path);
    }

    /// Empty slug → None (shared helper contract).
    #[test]
    fn spawn_content_outcome_review_for_slug_rejects_empty() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('proj1', 'Test', '/tmp/pa_empty', 1, 'workspace')",
            [],
        )
        .unwrap();
        let task = make_task();
        assert!(spawn_content_outcome_review_for_slug(&conn, &task, "").is_none());
    }

    // ─── Issue #272 nested write registration ────────────────────────────────

    fn nested_write_fixture() -> (rusqlite::Connection, std::path::PathBuf) {
        let project_path = std::env::temp_dir().join(format!(
            "pageseeds-pa-nested-kw-{}",
            uuid::Uuid::new_v4()
        ));
        let _ = std::fs::remove_dir_all(&project_path);
        std::fs::create_dir_all(project_path.join(".github/automation")).unwrap();
        std::fs::create_dir_all(project_path.join("content/blog")).unwrap();
        std::fs::write(
            project_path.join(".github/automation/seo_workspace.json"),
            r#"{"content_dir":"content/blog"}"#,
        )
        .unwrap();
        // Locator seed orphan (must not inherit write-task keyword).
        std::fs::write(
            project_path.join("content/blog/000_seed.mdx"),
            "---\ntitle: Seed\ndescription: Seed body for nested keyword isolation tests only.\nslug: seed\ndate: \"2024-01-01\"\n---\n\n# Seed\n\nseed body.\n",
        )
        .unwrap();

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, content_dir, active, project_mode)
             VALUES ('proj1', 'Test', ?1, 'content/blog', 1, 'workspace')",
            [project_path.to_str().unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles_meta (project_id, next_article_id) VALUES ('proj1', 1)",
            [],
        )
        .unwrap();
        (conn, project_path)
    }

    /// Nested path mirrors Path B: only the written basename gets K; co-ingested
    /// seed/orphans demote to draft without inheriting the write-task keyword.
    #[test]
    fn ingest_content_write_files_stamps_keyword_only_on_written_basename() {
        let (conn, project_path) = nested_write_fixture();
        std::fs::write(
            project_path.join("content/blog/seo_tools_new.mdx"),
            "---\ntitle: SEO Tools New\ndescription: A new article body for nested keyword stamp isolation.\nslug: seo-tools-new\ndate: \"2024-06-01\"\n---\n\n# SEO Tools New\n\nseo tools body.\n",
        )
        .unwrap();

        let mut task = make_task();
        task.id = "write-kw-only".to_string();
        task.task_type = "write_article".to_string();
        task.project_id = "proj1".to_string();
        task.description = Some(
            "File: content/blog/seo_tools_new.mdx\nTarget keyword: seo tools\nKD: 28\nVolume: 900"
                .to_string(),
        );

        let summary =
            ingest_content_write_files(&conn, &task, &project_path).expect("ingest should succeed");
        assert!(
            summary.ingested >= 2,
            "seed + written should ingest: {:?}",
            summary.files
        );
        assert!(
            summary.files.iter().any(|f| f == "seo_tools_new.mdx"),
            "files={:?}",
            summary.files
        );
        assert!(
            summary.files.iter().any(|f| f == "000_seed.mdx"),
            "files={:?}",
            summary.files
        );

        let written_kw: Option<String> = conn
            .query_row(
                "SELECT target_keyword FROM articles WHERE project_id='proj1' AND file LIKE '%seo_tools_new.mdx'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(written_kw.as_deref(), Some("seo tools"));

        let seed_kw: Option<String> = conn
            .query_row(
                "SELECT target_keyword FROM articles WHERE project_id='proj1' AND file LIKE '%000_seed.mdx'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            seed_kw.as_deref().unwrap_or("").is_empty(),
            "seed must not inherit write-task keyword: {seed_kw:?}"
        );
        let seed_status: String = conn
            .query_row(
                "SELECT status FROM articles WHERE project_id='proj1' AND file LIKE '%000_seed.mdx'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(seed_status, "draft");

        let _ = std::fs::remove_dir_all(&project_path);
    }

    /// Nested collision gate runs before keyword stamp: twin may exist as draft
    /// without K, but never as a second live catalog owner of the keyword.
    #[test]
    fn ingest_content_write_files_collision_skips_keyword_stamp() {
        let (conn, project_path) = nested_write_fixture();

        conn.execute(
            "INSERT INTO articles (
                id, project_id, title, url_slug, file, status, target_keyword,
                content_gaps_addressed, target_volume, word_count, review_count, content_hash,
                page_type
             ) VALUES (1, 'proj1', 'SEO Tools Hub', 'hub-seo-tools',
                       './content/blog/hub_seo_tools.mdx', 'published', 'seo tools',
                       '[]', 0, 100, 0, 'h', 'hub')",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE articles_meta SET next_article_id = 2 WHERE project_id = 'proj1'",
            [],
        )
        .unwrap();

        std::fs::write(
            project_path.join("content/blog/seo_tools_twin.mdx"),
            "---\ntitle: SEO Tools Twin\ndescription: Twin body for nested collision gate before keyword stamp.\nslug: seo-tools-twin\ndate: \"2024-06-01\"\n---\n\n# SEO Tools Twin\n\nseo tools body.\n",
        )
        .unwrap();

        let mut task = make_task();
        task.id = "write-kw-collide".to_string();
        task.task_type = "write_article".to_string();
        task.project_id = "proj1".to_string();
        task.description = Some(
            "File: content/blog/seo_tools_twin.mdx\nTarget keyword: seo tools\nKD: 28\nVolume: 900"
                .to_string(),
        );

        let err = ingest_content_write_files(&conn, &task, &project_path)
            .expect_err("collision must fail closed");
        assert!(
            is_keyword_collision_error(&err),
            "expected collision message, got: {err}"
        );
        assert!(err.contains("hub-seo-tools"), "must name collider: {err}");
        assert!(
            err.contains("Retarget") || err.contains("retarget") || err.contains("Consolidate"),
            "resolution guidance: {err}"
        );

        let twin_kw: Option<String> = conn
            .query_row(
                "SELECT target_keyword FROM articles WHERE project_id='proj1' AND file LIKE '%seo_tools_twin.mdx'",
                [],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        assert!(
            twin_kw.as_deref().unwrap_or("").is_empty(),
            "twin must not own colliding keyword: {twin_kw:?}"
        );

        // Twin may be present as draft without K (acceptable fail-closed state).
        let twin_status: Option<String> = conn
            .query_row(
                "SELECT status FROM articles WHERE project_id='proj1' AND file LIKE '%seo_tools_twin.mdx'",
                [],
                |row| row.get(0),
            )
            .ok();
        if let Some(status) = twin_status {
            assert_eq!(status, "draft");
        }

        // Owner hub still holds the keyword alone.
        let owners: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM articles
                 WHERE project_id='proj1' AND target_keyword IS NOT NULL
                   AND TRIM(target_keyword) != ''",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(owners, 1, "exactly one live owner of any keyword");

        let _ = std::fs::remove_dir_all(&project_path);
    }
