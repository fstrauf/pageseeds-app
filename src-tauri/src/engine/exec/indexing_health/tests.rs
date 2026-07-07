use super::*;
use crate::engine::project_paths::ProjectPaths;
use crate::models::indexing_health::{
    DistinctivenessVerdict, IndexingCampaignPlan, IndexingCampaignSummary,
    IndexingTargetContext, IndexingTargetPlan, PrerequisiteCheck, PrerequisiteReport,
    TargetArticleSummary, TargetDiagnosis,
};
use crate::models::task::{AgentPolicy, Priority, Task, TaskRunPolicy};
use crate::engine::spawner::DeduplicationPolicy;
use std::time::{SystemTime, UNIX_EPOCH};

    fn dummy_task() -> Task {
        Task {
            id: "task-123".to_string(),
            task_type: "indexing_health_campaign".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: Priority::High,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: crate::models::task::TaskReviewSurface::None,
            follow_up_policy: crate::models::task::FollowUpPolicy::None,
            agent_policy: AgentPolicy::Required,
            title: Some("Test Campaign".to_string()),
            description: Some("Test description".to_string()),
            project_id: "proj-abc".to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            not_before: None,
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        }
    }

    fn dummy_target_ctx(health: &str, links: usize, is_long: bool, reason: &str) -> IndexingTargetContext {
        IndexingTargetContext {
            target: TargetArticleSummary {
                url: "https://example.com/blog/test-article".to_string(),
                slug: "test-article".to_string(),
                reason_code: reason.to_string(),
                title: "Test Article".to_string(),
                h1: "Test H1".to_string(),
                target_keyword: "test keyword".to_string(),
                word_count: 800,
                incoming_links: links,
                content_audit_health: health.to_string(),
                article_id: 42,
                file: "content/test-article.mdx".to_string(),
            },
            cluster: None,
            diagnosis: TargetDiagnosis {
                has_links: links > 0,
                is_long,
                has_cluster_siblings: false,
                suspected_root_cause: "test".to_string(),
            },
            source_candidates: vec![],
        }
    }

    fn overlap_verdict(confidence: &str) -> DistinctivenessVerdict {
        DistinctivenessVerdict {
            target_url: "https://example.com/blog/test-article".to_string(),
            verdict: "OVERLAP".to_string(),
            confidence: confidence.to_string(),
            recommendation: "REWRITE".to_string(),
            keep_url: None,
            redirect_url: None,
            reason: "Shares H2s with sibling".to_string(),
            suggested_title: Some("Better Title".to_string()),
            suggested_h1: Some("Better H1".to_string()),
        }
    }

    fn distinct_verdict() -> DistinctivenessVerdict {
        DistinctivenessVerdict {
            target_url: "https://example.com/blog/test-article".to_string(),
            verdict: "DISTINCT".to_string(),
            confidence: "high".to_string(),
            recommendation: "NO_ACTION".to_string(),
            keep_url: None,
            redirect_url: None,
            reason: "Unique angle".to_string(),
            suggested_title: None,
            suggested_h1: None,
        }
    }

    // ─── determine_action tests ─────────────────────────────────────────────────

    #[test]
    fn determine_action_poor_health_returns_fix_content() {
        let ctx = dummy_target_ctx("poor", 5, true, "not_indexed_crawled");
        let action = determine_action(&ctx, None, &[]);
        assert_eq!(action, "fix_content");
    }

    #[test]
    fn determine_action_zero_links_returns_add_links() {
        let ctx = dummy_target_ctx("good", 0, true, "not_indexed_crawled");
        let action = determine_action(&ctx, None, &[]);
        assert_eq!(action, "add_links");
    }

    #[test]
    fn determine_action_high_overlap_returns_merge() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_crawled");
        let v = overlap_verdict("high");
        let action = determine_action(&ctx, Some(&v), &[]);
        assert_eq!(action, "merge");
    }

    #[test]
    fn determine_action_medium_overlap_returns_rewrite() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_crawled");
        let v = overlap_verdict("medium");
        let action = determine_action(&ctx, Some(&v), &[]);
        assert_eq!(action, "rewrite_title_h1");
    }

    #[test]
    fn determine_action_low_overlap_returns_rewrite() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_crawled");
        let v = overlap_verdict("low");
        let action = determine_action(&ctx, Some(&v), &[]);
        assert_eq!(action, "rewrite_title_h1");
    }

    #[test]
    fn determine_action_not_indexed_crawled_long_with_links_no_action() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_crawled");
        let v = distinct_verdict();
        let action = determine_action(&ctx, Some(&v), &[]);
        assert_eq!(action, "no_action");
    }

    #[test]
    fn determine_action_not_indexed_other_with_links_fix_indexing() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_other");
        let v = distinct_verdict();
        let action = determine_action(&ctx, Some(&v), &[]);
        assert_eq!(action, "fix_indexing");
    }

    #[test]
    fn determine_action_distinct_short_fix_indexing() {
        let ctx = dummy_target_ctx("good", 5, false, "not_indexed_crawled");
        let v = distinct_verdict();
        let action = determine_action(&ctx, Some(&v), &[]);
        assert_eq!(action, "fix_indexing");
    }

    #[test]
    fn determine_action_no_verdict_not_indexed_crawled_long_no_action() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_crawled");
        let action = determine_action(&ctx, None, &[]);
        assert_eq!(action, "no_action");
    }

    #[test]
    fn determine_action_no_verdict_not_indexed_other_fix_indexing() {
        let ctx = dummy_target_ctx("good", 5, true, "not_indexed_other");
        let action = determine_action(&ctx, None, &[]);
        assert_eq!(action, "fix_indexing");
    }

    // ─── slugify_url tests ──────────────────────────────────────────────────────

    #[test]
    fn slugify_url_strips_protocol() {
        assert_eq!(
            slugify_url("https://example.com/blog/my-post"),
            "example_com_blog_my-post"
        );
    }

    #[test]
    fn slugify_url_strips_www() {
        assert_eq!(
            slugify_url("https://www.example.com/page"),
            "example_com_page"
        );
    }

    #[test]
    fn slugify_url_http() {
        assert_eq!(
            slugify_url("http://example.com/path/to/page"),
            "example_com_path_to_page"
        );
    }

    #[test]
    fn slugify_url_lowercases() {
        assert_eq!(
            slugify_url("https://Example.COM/Blog/Page"),
            "example_com_blog_page"
        );
    }

    // ─── check_artifact tests ───────────────────────────────────────────────────

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
    }

    fn paths_from_dir(dir: &std::path::Path) -> ProjectPaths {
        ProjectPaths::from_path(dir.to_str().unwrap())
    }

    #[test]
    fn check_artifact_missing_file_not_fresh() {
        let dir = unique_temp_dir("ihc_test");
        let paths = paths_from_dir(&dir);
        let check = check_artifact(&paths, "missing.json", chrono::Duration::days(7));
        assert!(!check.fresh);
        assert_eq!(check.age_hours, None);
        assert_eq!(check.action, Some("auto_enqueue_missing".to_string()));
    }

    #[test]
    fn check_artifact_fresh_file_is_fresh() {
        let dir = unique_temp_dir("ihc_test");
        let paths = paths_from_dir(&dir);
        std::fs::create_dir_all(&paths.automation_dir).unwrap();
        std::fs::write(paths.automation_dir.join("fresh.json"), "{}").unwrap();

        let check = check_artifact(&paths, "fresh.json", chrono::Duration::days(7));
        assert!(check.fresh);
        assert!(check.age_hours.unwrap() < 1);
        assert_eq!(check.action, None);
    }

    #[test]
    fn check_artifact_cannibalization_auto_enqueues() {
        let dir = unique_temp_dir("ihc_test");
        let paths = paths_from_dir(&dir);
        std::fs::create_dir_all(&paths.automation_dir).unwrap();
        // Fresh file → no action needed
        std::fs::write(paths.automation_dir.join("cannibalization_strategy.json"), "{}").unwrap();
        let check = check_artifact(
            &paths,
            "cannibalization_strategy.json",
            chrono::Duration::days(7),
        );
        assert!(check.fresh);
        assert_eq!(check.action, None);
        // The stale action mapping is verified by the prerequisite_report test below.
    }

    // ─── build_rewrite_spec tests ───────────────────────────────────────────────

    fn dummy_target_plan(action: &str) -> IndexingTargetPlan {
        IndexingTargetPlan {
            url: "https://example.com/blog/test-article".to_string(),
            reason_code: "not_indexed_crawled".to_string(),
            recommended_action: action.to_string(),
            context_artifact_key: None,
            distinctiveness_verdict: Some(overlap_verdict("medium")),
            content_audit_summary: None,
            word_count: Some(800),
            incoming_links: Some(3),
            file: Some("content/test-article.mdx".to_string()),
        }
    }

    #[test]
    fn build_rewrite_spec_sets_correct_task_type() {
        let parent = dummy_task();
        let target = dummy_target_plan("rewrite_title_h1");
        let spec = build_rewrite_spec(&parent, &target, None);
        assert_eq!(spec.task_type, "fix_indexing");
        assert_eq!(spec.project_id, "proj-abc");
    }

    #[test]
    fn build_rewrite_spec_includes_suggested_title() {
        let parent = dummy_task();
        let target = dummy_target_plan("rewrite_title_h1");
        let spec = build_rewrite_spec(&parent, &target, None);
        let desc = spec.description.unwrap();
        assert!(desc.contains("Suggested title: Better Title"));
        assert!(desc.contains("Suggested H1: Better H1"));
        assert!(desc.contains("test-article"));
    }

    #[test]
    fn build_rewrite_spec_includes_context_artifact() {
        let parent = dummy_task();
        let target = dummy_target_plan("rewrite_title_h1");
        let ctx = dummy_target_ctx("good", 3, true, "not_indexed_crawled");
        let spec = build_rewrite_spec(&parent, &target, Some(&ctx));

        assert_eq!(spec.artifacts.len(), 1);
        assert_eq!(spec.artifacts[0].key, "indexing_target_context");
        assert_eq!(spec.artifacts[0].artifact_type, Some("json".to_string()));
        assert!(spec.artifacts[0].content.as_ref().unwrap().contains("test-article"));
    }

    #[test]
    fn build_rewrite_spec_has_idempotency_key() {
        let parent = dummy_task();
        let target = dummy_target_plan("rewrite_title_h1");
        let ctx = dummy_target_ctx("good", 0, true, "not_indexed_other");
        let spec = build_rewrite_spec(&parent, &target, Some(&ctx));
        let key = spec.idempotency_key.unwrap();
        assert!(key.starts_with("ihc-rewrite:"));
        assert!(key.contains("proj-abc"));
        // Key uses article_id (42), not parent.id, for cross-run dedup
        assert!(key.contains("42"));
        assert!(!key.contains("task-123"));
    }

    #[test]
    fn build_rewrite_spec_has_cooldown_dedup() {
        let parent = dummy_task();
        let target = dummy_target_plan("rewrite_title_h1");
        let spec = build_rewrite_spec(&parent, &target, None);
        match spec.dedup_policy {
            Some(DeduplicationPolicy::Cooldown { days }) => assert_eq!(days, 30),
            other => panic!("Expected Cooldown dedup policy, got {:?}", other),
        }
    }

    // ─── build_fix_content_spec tests ───────────────────────────────────────────

    #[test]
    fn build_fix_content_spec_sets_correct_type() {
        let parent = dummy_task();
        let target = dummy_target_plan("fix_content");
        let ctx = dummy_target_ctx("poor", 3, true, "not_indexed_other");
        let spec = build_fix_content_spec(&parent, &target, Some(&ctx), None);
        assert_eq!(spec.task_type, "fix_content_article");
        assert_eq!(spec.project_id, "proj-abc");
    }

    #[test]
    fn build_fix_content_spec_description_includes_url() {
        let parent = dummy_task();
        let target = dummy_target_plan("fix_content");
        let ctx = dummy_target_ctx("poor", 3, true, "not_indexed_other");
        let spec = build_fix_content_spec(&parent, &target, Some(&ctx), None);
        let desc = spec.description.unwrap();
        assert!(desc.contains("test-article"));
        assert!(desc.contains("fix_content"));
    }

    #[test]
    fn build_fix_content_spec_includes_recommendation_artifact() {
        let parent = dummy_task();
        let target = dummy_target_plan("fix_content");
        let ctx = dummy_target_ctx("poor", 3, true, "not_indexed_other");
        let spec = build_fix_content_spec(&parent, &target, Some(&ctx), None);
        assert_eq!(spec.artifacts.len(), 1);
        assert!(spec.artifacts[0].key.starts_with("recommendations_"));
        let content = spec.artifacts[0].content.as_ref().unwrap();
        assert!(content.contains("article_id"));
        assert!(content.contains("suggestions"));
        assert!(content.contains("content_depth"));
    }

    // ─── build_add_links_spec tests ─────────────────────────────────────────────

    #[test]
    fn build_add_links_spec_sets_correct_type() {
        let parent = dummy_task();
        let target = dummy_target_plan("add_links");
        let ctx = dummy_target_ctx("good", 0, true, "not_indexed_other");
        let spec = build_add_links_spec(&parent, &target, Some(&ctx));
        assert_eq!(spec.task_type, "fix_indexing_internal_links");
        assert_eq!(spec.project_id, "proj-abc");
    }

    #[test]
    fn build_add_links_spec_description_includes_url() {
        let parent = dummy_task();
        let target = dummy_target_plan("add_links");
        let ctx = dummy_target_ctx("good", 0, true, "not_indexed_other");
        let spec = build_add_links_spec(&parent, &target, Some(&ctx));
        let desc = spec.description.unwrap();
        assert!(desc.contains("test-article"));
        assert!(desc.contains("add_links"));
    }

    #[test]
    fn build_add_links_spec_includes_target_artifact() {
        let parent = dummy_task();
        let target = dummy_target_plan("add_links");
        let ctx = dummy_target_ctx("good", 0, true, "not_indexed_other");
        let spec = build_add_links_spec(&parent, &target, Some(&ctx));
        assert_eq!(spec.artifacts.len(), 1);
        assert_eq!(spec.artifacts[0].key, "indexing_link_target");
        let content = spec.artifacts[0].content.as_ref().unwrap();
        assert!(content.contains("campaign_task_id"));
        assert!(content.contains("test-article"));
        assert!(content.contains("article_id"));
    }

    // ─── PrerequisiteReport serialization tests ─────────────────────────────────

    #[test]
    fn prerequisite_report_serializes_correctly() {
        let report = PrerequisiteReport {
            all_fresh: false,
            checks: vec![
                PrerequisiteCheck {
                    artifact: "gsc_collection.json".to_string(),
                    fresh: true,
                    age_hours: Some(12),
                    action: None,
                },
                PrerequisiteCheck {
                    artifact: "cannibalization_strategy.json".to_string(),
                    fresh: false,
                    age_hours: Some(500),
                    action: Some("auto_enqueue_cannibalization_audit".to_string()),
                },
            ],
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("gsc_collection.json"));
        assert!(json.contains("auto_enqueue_cannibalization_audit"));
        assert!(json.contains("false"));
    }

    // ─── IndexingCampaignPlan serialization tests ───────────────────────────────

    #[test]
    fn campaign_plan_roundtrips_json() {
        let plan = IndexingCampaignPlan {
            generated_at: "2024-01-01".to_string(),
            targets: vec![
                IndexingTargetPlan {
                    url: "https://example.com/a".to_string(),
                    reason_code: "not_indexed_crawled".to_string(),
                    recommended_action: "rewrite_title_h1".to_string(),
                    context_artifact_key: None,
                    distinctiveness_verdict: Some(overlap_verdict("medium")),
                    content_audit_summary: None,
                    word_count: Some(500),
                    incoming_links: Some(2),
                    file: Some("content/a.mdx".to_string()),
                },
            ],
            summary: IndexingCampaignSummary {
                total_targets: 1,
                fix_content: 0,
                add_links: 0,
                merge: 0,
                rewrite_title_h1: 1,
                no_action: 0,
            },
        };
        let json = serde_json::to_string_pretty(&plan).unwrap();
        let parsed: IndexingCampaignPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.targets.len(), 1);
        assert_eq!(parsed.summary.rewrite_title_h1, 1);
    }
