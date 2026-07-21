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
