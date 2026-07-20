use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

use super::*;
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_related_section_link_creates_new_section() {
        let content = "# Hello\n\nThis is a paragraph.\n";
        let result = apply_related_section_link(content, "Example Page", "example-page");
        assert!(result.contains("## Related Articles"));
        assert!(result.contains("[Example Page](/blog/example-page)"));
    }

    #[test]
    fn apply_related_section_link_appends_to_existing_section() {
        let content = "# Hello\n\nThis is a paragraph.\n\n## Related Articles\n\n- [Another](/blog/another)\n";
        let result = apply_related_section_link(content, "Example Page", "example-page");
        assert!(result.contains("[Another](/blog/another)"));
        assert!(result.contains("[Example Page](/blog/example-page)"));
        // Should not create a second Related Articles section
        let section_count = result.matches("## Related Articles").count();
        assert_eq!(section_count, 1);
    }

    #[test]
    fn insert_contextual_link_finds_relevant_paragraph() {
        let content = "# Machine Learning Guide\n\nMachine learning is a subset of artificial intelligence.\n\nBaking cakes is a fun hobby.\n";
        let result = insert_contextual_link(content, "machine learning tutorial", "ml-tutorial");
        assert!(result.is_some());
        let result = result.unwrap();
        assert!(result.contains("[machine learning tutorial](/blog/ml-tutorial)"));
        // The link should be in the ML paragraph, not the baking paragraph
        let lines: Vec<&str> = result.lines().collect();
        let ml_line = lines
            .iter()
            .position(|l| l.contains("Machine learning"))
            .unwrap();
        let baking_line = lines
            .iter()
            .position(|l| l.contains("Baking cakes"))
            .unwrap();
        assert!(lines[ml_line].contains("ml-tutorial"));
        assert!(!lines[baking_line].contains("ml-tutorial"));
    }

    #[test]
    fn insert_contextual_link_falls_back_to_longest_paragraph() {
        let content = "# Baking Guide\n\nBaking cakes is a fun hobby that many people enjoy on weekends with their families and friends.\n\nChocolate is delicious.\n";
        let result = insert_contextual_link(content, "machine learning tutorial", "ml-tutorial");
        // No keyword match, but falls back to the longest substantial paragraph (>80 chars)
        assert!(result.is_some(), "should fall back to longest paragraph");
        let result = result.unwrap();
        // The longest paragraph gets the link
        assert!(result.contains("Baking cakes"));
        assert!(result.contains("ml-tutorial"));
    }

    #[test]
    fn parse_target_artifact_extracts_target() {
        let task = Task {
            id: "task-1".to_string(),
            project_id: "proj-1".to_string(),
            task_type: "fix_indexing_internal_links".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: crate::models::task::TaskReviewSurface::None,
            follow_up_policy: crate::models::task::FollowUpPolicy::BackendAuto,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "indexing_link_target".to_string(),
                path: None,
                artifact_type: None,
                source: None,
                content: Some(r#"{"target": {"slug": "test-page", "article_id": 42}}"#.to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        };

        let target = parse_target_artifact(&task);
        assert!(target.is_some());
        let target = target.unwrap();
        assert_eq!(target["slug"].as_str(), Some("test-page"));
        assert_eq!(target["article_id"].as_i64(), Some(42));
    }

    #[test]
    fn parse_target_artifact_returns_none_for_missing_key() {
        let task = Task {
            id: "task-1".to_string(),
            project_id: "proj-1".to_string(),
            task_type: "fix_indexing_internal_links".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: crate::models::task::TaskReviewSurface::None,
            follow_up_policy: crate::models::task::FollowUpPolicy::BackendAuto,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        };

        assert!(parse_target_artifact(&task).is_none());
    }

    #[test]
    fn normalize_link_slug_strips_prefixes() {
        use crate::content::slug::normalize_url_slug;
        assert_eq!(normalize_url_slug("my-post"), "my-post");
        assert_eq!(normalize_url_slug("blog/my-post"), "my-post");
        assert_eq!(normalize_url_slug("/blog/my-post"), "my-post");
        assert_eq!(normalize_url_slug("tools/blog/my-post"), "my-post");
        // Double numeric prefix (date + sequence) — must be fully stripped
        assert_eq!(
            normalize_url_slug("2025-08-01-the-good-enough-mindset"),
            "the-good-enough-mindset"
        );
        assert_eq!(
            normalize_url_slug("01-the-good-enough-mindset"),
            "the-good-enough-mindset"
        );
    }

    /// Build a minimal `fix_indexing_internal_links` task carrying the given artifacts.
    fn indexing_link_task(artifacts: Vec<crate::models::task::TaskArtifact>) -> Task {
        Task {
            id: "task-1".to_string(),
            project_id: "proj-1".to_string(),
            task_type: "fix_indexing_internal_links".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: crate::models::task::TaskReviewSurface::None,
            follow_up_policy: crate::models::task::FollowUpPolicy::BackendAuto,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts,
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        }
    }

    fn target_artifact_with_candidates() -> crate::models::task::TaskArtifact {
        crate::models::task::TaskArtifact {
            key: "indexing_link_target".to_string(),
            path: None,
            artifact_type: None,
            source: None,
            content: Some(
                r#"{"target": {
                    "article_id": 42,
                    "slug": "target-page",
                    "title": "Target Page",
                    "url": "https://example.com/blog/target-page",
                    "target_keyword": "target keyword",
                    "reason_code": "no_internal_links",
                    "source_candidates": [
                        {"article_id": 7, "slug": "source-a", "file": "source-a.mdx", "title": "Source A", "gsc_impressions": 120, "score": 0.9},
                        {"article_id": 8, "slug": "source-b", "file": "source-b.mdx", "title": "Source B", "gsc_impressions": 45, "score": 0.5}
                    ]
                }}"#
                .to_string(),
            ),
        }
    }

    /// Regression test for issue #34: the context step used to early-return
    /// "Nothing to verify" when no `indexing_link_plan` artifact existed yet
    /// (the plan is only written by step 2), emitting an output with no
    /// `sources` array. That made the plan step short-circuit with empty
    /// `links_to_add` and the verify step fail every task.
    ///
    /// The context step must instead build a non-empty `sources` array from
    /// the target artifact's `source_candidates`, and that context must flow
    /// into the plan step without hitting the empty-sources short-circuit.
    #[test]
    fn context_step_builds_sources_and_hands_off_to_plan() {
        // Temp project dir with a freshly written link_scan.json so the
        // context step never falls back to opening the real app DB + rescan
        // (fresh mtime is never stale).
        let temp_dir = std::env::temp_dir().join(format!(
            "indexing_link_ctx_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        let automation_dir = temp_dir.join(".github").join("automation");
        std::fs::create_dir_all(&automation_dir).expect("create automation dir");
        std::fs::write(automation_dir.join("link_scan.json"), r#"{"profiles": []}"#)
            .expect("write link_scan.json");
        let project_path = temp_dir.to_string_lossy().to_string();

        let task = indexing_link_task(vec![target_artifact_with_candidates()]);

        // Step 1: context must succeed and emit a non-empty sources array.
        let ctx = exec_indexing_link_context(&task, &project_path);
        assert!(ctx.success, "context step failed: {}", ctx.message);
        let ctx_json: serde_json::Value =
            serde_json::from_str(ctx.output.as_deref().unwrap_or("{}")).expect("context output JSON");
        let sources = ctx_json["sources"].as_array().cloned().unwrap_or_default();
        assert_eq!(
            sources.len(),
            2,
            "context sources must be built from source_candidates, got: {}",
            ctx.output.as_deref().unwrap_or("<none>")
        );

        // Handoff: attach the context output as the artifact the plan step
        // reads, then run plan. With non-empty sources, plan proceeds past the
        // short-circuit to the agent call; the unknown provider makes that call
        // fail fast with a clean error (no subprocess), which proves the
        // sources flowed through.
        let mut task_with_ctx = task.clone();
        task_with_ctx.artifacts.push(crate::models::task::TaskArtifact {
            key: "indexing_link_context".to_string(),
            path: None,
            artifact_type: None,
            source: None,
            content: ctx.output.clone(),
        });
        let plan = exec_indexing_link_plan(&task_with_ctx, &project_path, "__test_unknown_provider__");
        assert!(
            !(plan.success && plan.message.contains("Nothing to do")),
            "plan must not short-circuit on empty sources; got: success={} message={}",
            plan.success,
            plan.message
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    /// Inverse of the handoff test: a context with empty `sources` must still
    /// make the plan step short-circuit successfully (no behavior change there).
    #[test]
    fn plan_still_short_circuits_on_empty_sources() {
        let mut task = indexing_link_task(vec![target_artifact_with_candidates()]);
        task.artifacts.push(crate::models::task::TaskArtifact {
            key: "indexing_link_context".to_string(),
            path: None,
            artifact_type: None,
            source: None,
            content: Some(r#"{"target": {}, "sources": []}"#.to_string()),
        });

        let plan = exec_indexing_link_plan(&task, "/nonexistent", "__test_unknown_provider__");
        assert!(plan.success, "plan should short-circuit successfully: {}", plan.message);
        assert!(
            plan.message.contains("Nothing to do"),
            "expected empty-sources short-circuit, got: {}",
            plan.message
        );
    }

    /// End-to-end test: apply a link to the real learnedlate repo file and verify
    /// scan_links detects it. This exercises the full apply → verify path.
    #[test]
    #[ignore = "requires filesystem + DB"] // run with: cargo test -- --ignored
    fn apply_and_verify_on_real_file() {
        let project_path = "/Users/fstrauf/01_code/learnedlate";
        let content_dir = std::path::Path::new(project_path).join("src/blog/posts");
        let source_file = content_dir.join("070_product_management_for_non_technical_founders_a_practical_guide.mdx");

        // Read original content
        let original = std::fs::read_to_string(&source_file).expect("read source");

        // Apply link using the fixed function
        let modified = apply_related_section_link(&original, "the good enough mindset", "the-good-enough-mindset");

        // Sanity: the link line must contain proper markdown ()
        assert!(
            modified.contains("[the good enough mindset](/blog/the-good-enough-mindset)"),
            "link must be properly formatted markdown"
        );

        // Write to a temp copy so we don't mutate the repo
        let temp_dir = std::path::PathBuf::from(format!("/tmp/test_link_fix_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis()));
        std::fs::create_dir_all(&temp_dir).expect("create temp dir");
        let temp_file = temp_dir.join("070_product_management_for_non_technical_founders_a_practical_guide.mdx");
        std::fs::write(&temp_file, &modified).expect("write temp");

        // Also copy the target file so scan_links can build profiles for it
        let target_file = content_dir.join("2025-08-01-the-good-enough-mindset.mdx");
        let temp_target = temp_dir.join("2025-08-01-the-good-enough-mindset.mdx");
        std::fs::copy(&target_file, &temp_target).expect("copy target");

        // Load articles from DB (need at least source + target)
        let db_path = crate::db::default_db_path();
        let db = rusqlite::Connection::open(&db_path).expect("open db");
        let articles: Vec<crate::models::article::Article> = crate::content::article_index::list_articles(&db, "learnedlate")
            .expect("list articles")
            .into_iter()
            .filter(|a| {
                a.file.contains("070_product_management")
                    || a.file.contains("2025-08-01-the-good-enough-mindset")
            })
            .collect();

        assert!(
            articles.iter().any(|a| a.id == 70),
            "source article 70 must be in DB"
        );
        assert!(
            articles.iter().any(|a| a.id == 19),
            "target article 19 must be in DB"
        );

        // Scan links in the temp dir
        let scan_result = crate::content::linking::scan_links(&temp_dir, &articles)
            .expect("scan_links");

        // Find target profile
        let target_profile = scan_result.profiles.iter().find(|p| p.id == 19);
        let incoming_after = target_profile.map(|p| p.incoming_ids.len()).unwrap_or(0);

        assert!(
            incoming_after > 0,
            "target article 19 must have >0 inbound links after apply; found {}. Profiles: {:?}",
            incoming_after,
            scan_result.profiles
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
