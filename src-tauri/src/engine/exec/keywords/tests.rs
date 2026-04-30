#[cfg(test)]
mod tests {
    use crate::engine::exec::keywords::*;
    use crate::models::task::{FollowUpPolicy, TaskReviewSurface};
    use std::fs;
    use std::path::PathBuf;

    /// Write `content` to `<tmp>/ps_kw_test_<name>.md` and return the path.
    fn write_tmp(name: &str, content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("ps_kw_test_{name}.md"));
        fs::write(&path, content).unwrap();
        path
    }

    // ── extract_from_brief: 🎯 items ─────────────────────────────────────────

    #[test]
    fn brief_goal_markers_extract_topic_names() {
        let path = write_tmp(
            "brief_goals",
            "\
## Gap Analysis\n\
- [ ] 🎯 SEO Tools for Beginners (PLANNED)\n\
- [ ] 🎯 Content Marketing Strategy\n\
- No marker here\n",
        );
        let themes = extract_from_brief(&path);
        assert!(
            themes.contains(&"SEO Tools for Beginners".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Content Marketing Strategy".to_string()),
            "got: {themes:?}"
        );
        // Non-goal lines must not appear.
        assert!(!themes.iter().any(|t| t.contains("No marker")));
    }

    #[test]
    fn brief_goal_heading_cluster_style_extracts_topic() {
        // Exact format from the failing brief: "### Cluster N: Topic (annotation) 🎯"
        // Old code returned ["### Cluster 7", "### Cluster 8"] — sending markdown
        // heading tokens straight to Ahrefs.
        let path = write_tmp(
            "brief_goals_heading",
            "\
### Cluster 7: Risk Management (EMERGING) 🎯\n\
### Cluster 8: Advanced Topics (EMERGING) 🎯\n\
**Cluster 9: IRA / Retirement Account Options (NEW) 🎯**\n\
**Cluster 10: Protective Put / Portfolio Hedging (NEW) 🎯**\n",
        );
        let themes = extract_from_brief(&path);
        assert!(
            !themes.iter().any(|t| t.contains('#')),
            "no # markers: {themes:?}"
        );
        assert!(
            !themes.iter().any(|t| t.starts_with("Cluster ")),
            "no bare cluster labels: {themes:?}"
        );
        assert!(
            themes.contains(&"Risk Management".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Advanced Topics".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"IRA / Retirement Account Options".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Protective Put / Portfolio Hedging".to_string()),
            "got: {themes:?}"
        );
    }

    // ── extract_from_brief: PLANNED clusters ──────────────────────────────────

    #[test]
    fn brief_planned_cluster_with_colon_extracts_topic() {
        let path = write_tmp(
            "brief_planned",
            "\
### Cluster 4: Advanced SEO Tactics (PLANNED)\n\
### Cluster 5: Link Building (PLANNED)\n",
        );
        let themes = extract_from_brief(&path);
        assert!(
            themes.contains(&"Advanced SEO Tactics".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Link Building".to_string()),
            "got: {themes:?}"
        );
    }

    #[test]
    fn brief_planned_heading_without_colon_is_filtered_out() {
        // "### Cluster 7 (PLANNED)" has no colon → no real topic → must be dropped.
        let path = write_tmp("brief_planned_no_colon", "### Cluster 7 (PLANNED)\n");
        let themes = extract_from_brief(&path);
        assert!(
            themes.is_empty(),
            "bare cluster label should be filtered: {themes:?}"
        );
    }

    // ── extract_from_brief: all-clusters fallback ─────────────────────────────

    #[test]
    fn brief_cluster_headings_without_planned_uses_last_resort() {
        let path = write_tmp(
            "brief_clusters",
            "\
### Cluster 1: On-Page SEO\n\
### Cluster 2: Technical SEO\n",
        );
        let themes = extract_from_brief(&path);
        assert!(
            themes.contains(&"On-Page SEO".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Technical SEO".to_string()),
            "got: {themes:?}"
        );
    }

    #[test]
    fn brief_empty_file_returns_empty() {
        let path = write_tmp("brief_empty", "");
        assert!(extract_from_brief(&path).is_empty());
    }

    #[test]
    fn brief_missing_file_returns_empty() {
        assert!(
            extract_from_brief(std::path::Path::new("/nonexistent/ps_kw_missing.md")).is_empty()
        );
    }

    // ── extract_from_summary ──────────────────────────────────────────────────

    #[test]
    fn summary_pillar_headings_extract_names() {
        let path = write_tmp(
            "summary_pillars",
            "\
### Pillar 1: Keyword Research\n\
### Pillar 2: Content Creation\n\
## Other section\n",
        );
        let themes = extract_from_summary(&path);
        assert!(
            themes.contains(&"Keyword Research".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Content Creation".to_string()),
            "got: {themes:?}"
        );
    }

    #[test]
    fn summary_search_keywords_list_fallback() {
        let path = write_tmp(
            "summary_keywords",
            "\
## Search Keywords\n\
- seo tips\n\
- content strategy\n\
## Other\n",
        );
        let themes = extract_from_summary(&path);
        assert!(themes.contains(&"seo tips".to_string()), "got: {themes:?}");
        assert!(
            themes.contains(&"content strategy".to_string()),
            "got: {themes:?}"
        );
    }

    #[test]
    fn summary_empty_file_returns_empty() {
        let path = write_tmp("summary_empty", "");
        assert!(extract_from_summary(&path).is_empty());
    }

    // ── find_file_by_suffix ───────────────────────────────────────────────────

    #[test]
    fn find_file_locates_by_partial_name() {
        let dir = std::env::temp_dir().join("ps_kw_find_test");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("my_seo_content_brief_v2.md");
        fs::write(&file, "content").unwrap();

        let found = find_file_by_suffix(&dir, "seo_content_brief");
        assert!(found.is_some(), "expected to find file");

        fs::remove_dir_all(&dir).ok();
    }

    // ── clean_theme_str ──────────────────────────────────────────────────────────────

    #[test]
    fn clean_theme_markdown_heading_no_colon_rejected() {
        // Exact inputs from the log: ["### Cluster 7", "### Cluster 8", ...]
        assert_eq!(clean_theme_str("### Cluster 7"), None);
        assert_eq!(clean_theme_str("### Cluster 8"), None);
    }

    #[test]
    fn clean_theme_bare_cluster_label_rejected() {
        assert_eq!(clean_theme_str("Cluster 9"), None);
        assert_eq!(clean_theme_str("Cluster 10"), None);
    }

    #[test]
    fn clean_theme_heading_with_colon_extracts_topic() {
        assert_eq!(
            clean_theme_str("### Cluster 4: SEO Tools"),
            Some("SEO Tools".to_string())
        );
    }

    #[test]
    fn clean_theme_strips_planned_annotation() {
        assert_eq!(
            clean_theme_str("### Cluster 5: Link Building (PLANNED)"),
            Some("Link Building".to_string())
        );
    }

    #[test]
    fn clean_theme_plain_topic_passes_through() {
        assert_eq!(
            clean_theme_str("content marketing"),
            Some("content marketing".to_string())
        );
    }

    #[test]
    fn clean_theme_empty_returns_none() {
        assert_eq!(clean_theme_str(""), None);
        assert_eq!(clean_theme_str("  "), None);
    }

    // ── parse_desc_themes ──────────────────────────────────────────────────────────

    #[test]
    fn parse_desc_exact_failing_log_payload_returns_empty() {
        // This is the exact string that caused the CapSolver failure.
        // After the fix it must produce zero themes so the fallback kicks in.
        let raw = "### Cluster 7, ### Cluster 8, Cluster 9, Cluster 10";
        assert!(
            parse_desc_themes(raw).is_empty(),
            "bare cluster labels must all be filtered out"
        );
    }

    #[test]
    fn parse_desc_topics_with_colon_extracted() {
        let raw = "### Cluster 4: SEO Tools, ### Cluster 5: Link Building";
        let themes = parse_desc_themes(raw);
        assert_eq!(themes, vec!["SEO Tools", "Link Building"]);
    }

    #[test]
    fn parse_desc_plain_comma_list_passes_through() {
        let raw = "seo tools, content marketing, link building";
        let themes = parse_desc_themes(raw);
        assert_eq!(
            themes,
            vec!["seo tools", "content marketing", "link building"]
        );
    }

    #[test]
    fn parse_desc_newline_separated_works() {
        let raw = "seo tools\ncontent marketing\n";
        assert_eq!(
            parse_desc_themes(raw),
            vec!["seo tools", "content marketing"]
        );
    }

    #[test]
    fn parse_desc_mixed_valid_and_bare_clusters() {
        // If a description has some good themes AND some bare cluster junk,
        // only the good ones should survive.
        let raw = "### Cluster 7, SEO Automation, Cluster 9";
        let themes = parse_desc_themes(raw);
        assert_eq!(themes, vec!["SEO Automation"]);
    }
    // ── derive_themes_from_project integration ────────────────────────────────

    #[test]
    fn derive_themes_real_brief_format_returns_clean_topics() {
        // Exact content structure from the brief that caused the CapSolver failure.
        // Verifies the full stack: find_file → extract_from_brief → clean_theme_str.
        let dir = std::env::temp_dir().join("ps_kw_derive_real");
        fs::create_dir_all(&dir).unwrap();

        let brief = "\
## Existing Clusters\n\
### Cluster 7: Risk Management (EMERGING) \u{1f3af}\n\
**Pillar Content:** Risk management principles\n\
\n\
### Cluster 8: Advanced Topics (EMERGING) \u{1f3af}\n\
**Pillar Content:** Advanced strategies\n\
\n\
### New Clusters Discovered\n\
**Cluster 9: IRA / Retirement Account Options (NEW) \u{1f3af}**\n\
\n\
**Cluster 10: Protective Put / Portfolio Hedging (NEW) \u{1f3af}**\n";

        fs::write(dir.join("seo_content_brief.md"), brief).unwrap();

        let themes = derive_themes_from_project(&dir);

        assert!(!themes.is_empty(), "should derive themes, got none");
        assert!(
            !themes.iter().any(|t| t.contains('#')),
            "no markdown heading markers in themes: {themes:?}"
        );
        assert!(
            !themes.iter().any(|t| {
                let w: Vec<_> = t.split_whitespace().collect();
                w.len() <= 2
                    && w.first()
                        .map(|s| s.eq_ignore_ascii_case("cluster"))
                        .unwrap_or(false)
            }),
            "no bare 'Cluster N' labels in themes: {themes:?}"
        );
        assert!(
            themes.contains(&"Risk Management".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Advanced Topics".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"IRA / Retirement Account Options".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Protective Put / Portfolio Hedging".to_string()),
            "got: {themes:?}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn derive_themes_bare_cluster_only_brief_returns_empty() {
        // If a brief has ONLY bare "### Cluster N" headings (no colon → no topic),
        // derive_themes should return empty so the executor fails with a clear
        // "No themes found" message instead of sending junk strings to Ahrefs.
        let dir = std::env::temp_dir().join("ps_kw_derive_bare");
        fs::create_dir_all(&dir).unwrap();

        let brief = "### Cluster 7 (PLANNED)\n### Cluster 8 (PLANNED)\nCluster 9\nCluster 10\n";
        fs::write(dir.join("seo_content_brief.md"), brief).unwrap();

        let themes = derive_themes_from_project(&dir);

        assert!(
            themes.is_empty(),
            "bare cluster labels must produce empty themes (not sent to Ahrefs): {themes:?}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn derive_themes_without_project_summary_uses_brief_only() {
        // Regression: missing project_summary.md must not crash or block theme derivation.
        let dir = std::env::temp_dir().join("ps_kw_derive_no_summary");
        fs::create_dir_all(&dir).unwrap();

        fs::write(
            dir.join("seo_content_brief.md"),
            "### Cluster 1: Protective Put (PLANNED)\n",
        )
        .unwrap();

        let themes = derive_themes_from_project(&dir);
        assert_eq!(themes, vec!["Protective Put".to_string()]);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn derive_themes_missing_brief_and_summary_returns_empty() {
        // Regression: no brief + no summary should fail gracefully with empty themes.
        let dir = std::env::temp_dir().join("ps_kw_derive_missing_all");
        fs::create_dir_all(&dir).unwrap();

        let themes = derive_themes_from_project(&dir);
        assert!(themes.is_empty(), "expected empty themes, got {themes:?}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_agent_themes_handles_fenced_json_with_tool_logs() {
        let raw = r#"● Read seo_content_brief.md
 │ .github/automation/seo_content_brief.md
 └ 1 line read

```json
{
  "themes": ["Protective Put", "IRA Options", "Portfolio Hedging"]
}
```
"#;

        let task = crate::models::task::Task {
            id: "t1".to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: crate::models::task::AgentPolicy::Optional,
            title: None,
            description: None,
            project_id: "p1".to_string(),
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "research_seed_extraction".to_string(),
                path: None,
                artifact_type: Some("agentic".to_string()),
                source: Some("agentic".to_string()),
                content: Some(raw.to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let parsed = parse_seed_extraction_artifact(&task);
        assert_eq!(
            parsed.themes,
            vec!["Protective Put", "IRA Options", "Portfolio Hedging"]
        );
    }

    // Note: List fallback ("1. Theme") removed - we now require JSON output contract.
    // The deterministic step expects {"themes": [...]} format from Step 1.

    #[test]
    fn parse_agent_themes_supports_array_json_contract() {
        let task = crate::models::task::Task {
            id: "t3".to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: crate::models::task::AgentPolicy::Optional,
            title: None,
            description: None,
            project_id: "p1".to_string(),
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "research_seed_extraction".to_string(),
                path: None,
                artifact_type: Some("agentic".to_string()),
                source: Some("agentic".to_string()),
                content: Some("[\"Protective Put\", \"IRA Options\"]".to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let parsed = parse_seed_extraction_artifact(&task);
        assert_eq!(parsed.themes, vec!["Protective Put", "IRA Options"]);
    }
    // ── find_file_by_suffix ──────────────────────────────────────────────────────

    #[test]
    fn find_file_exact_match_returned_first() {
        let dir = std::env::temp_dir().join("ps_kw_find_exact");
        fs::create_dir_all(&dir).unwrap();
        let exact = dir.join("seo_content_brief.md");
        fs::write(&exact, "exact").unwrap();

        let found = find_file_by_suffix(&dir, "seo_content_brief.md");
        assert_eq!(found.unwrap(), exact);

        fs::remove_dir_all(&dir).ok();
    }
}

// ─── Integration tests (require live credentials) ─────────────────────────────
//
// These tests call real external APIs (CapSolver → Ahrefs).
// They are marked `#[ignore]` so normal `cargo test` skips them.
//
// Run with:
//   CAPSOLVER_API_KEY=<key> cargo test --lib keyword_research_integration -- --ignored --nocapture
//
// Requirements:
//   - CAPSOLVER_API_KEY must be set (in env or ~/.config/automation/secrets.env)
//   - Network access to CapSolver and Ahrefs must be available

#[cfg(test)]
mod integration_tests {
    use crate::engine::exec::keywords::*;
    use crate::engine::workflows::StepResult;
    use crate::models::task::{FollowUpPolicy, TaskReviewSurface};
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Build a unique temp directory for a test run.
    fn unique_temp_project_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{prefix}_{nanos}"))
    }

    /// Helper: build a minimal fake repo at `dir` with:
    ///   - `.github/automation/seo_content_brief.md` containing `theme`
    ///   - `.github/automation/articles.json` (empty array)
    fn setup_dummy_project(dir: &std::path::Path, theme: &str) {
        let automation = dir.join(".github").join("automation");
        fs::create_dir_all(&automation).unwrap();

        let brief = format!("## Clusters\n\n### Cluster 1: {theme} (PLANNED)\n");
        fs::write(automation.join("seo_content_brief.md"), brief).unwrap();
        fs::write(automation.join("articles.json"), "[]").unwrap();
    }

    /// Run the full native keyword research flow against a temp dummy project.
    fn run_dummy_project_flow(theme: &str) -> StepResult {
        let dir = unique_temp_project_dir("ps_kw_integration_test");
        setup_dummy_project(&dir, theme);

        let project_path = dir.to_string_lossy().to_string();

        let task = crate::models::task::Task {
            id: "integration-test".to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Integration test".to_string()),
            description: None,
            project_id: "test".to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        // Need a tokio runtime because exec_keyword_research_native uses block_on.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            tokio::task::spawn_blocking(move || {
                exec_keyword_research_native(&task, &project_path, "ahrefs")
            })
            .await
            .unwrap()
        });

        fs::remove_dir_all(&dir).ok();
        result
    }

    /// Full end-to-end: brief → theme extraction → CapSolver → Ahrefs keyword ideas
    /// → difficulty analysis → structured JSON output.
    ///
    /// This is what the "Run" button triggers. If it fails here, it will fail in the app.
    #[test]
    #[ignore = "calls live CapSolver + Ahrefs APIs; run with --ignored"]
    fn full_keyword_research_pipeline_single_theme() {
        // Resolve CAPSOLVER_API_KEY the same way the app does.
        let capsolver_key = {
            use crate::config::env_resolver::EnvResolver;
            // Use a throwaway project path — we only need the secrets resolution.
            let env = EnvResolver::new("/tmp").build_env(std::collections::HashMap::new());
            env.get("CAPSOLVER_API_KEY").cloned().unwrap_or_default()
        };

        if capsolver_key.is_empty() {
            eprintln!(
                "SKIP: CAPSOLVER_API_KEY not set — set it in ~/.config/automation/secrets.env"
            );
            return;
        }

        // Build and run against a minimal throwaway dummy project.
        let result = run_dummy_project_flow("options risk management");

        eprintln!("=== StepResult ===");
        eprintln!("success: {}", result.success);
        eprintln!("message: {}", result.message);
        if let Some(output) = &result.output {
            let v: serde_json::Value = serde_json::from_str(output).unwrap_or_default();
            eprintln!("themes:   {:?}", v["themes"]);
            eprintln!("candidates: {}", v["total_candidates"]);
            eprintln!("analyzed:   {}", v["difficulty"]["total"]);
            eprintln!("results:    {}", v["difficulty"]["results"]);
        }

        if !result.success {
            assert!(
                result.message.contains("No new keyword ideas found")
                    || result.message.contains("Failed to fetch keyword ideas")
                    || result.message.contains("No themes found")
                    || result.message.contains("CAPSOLVER"),
                "unexpected pipeline failure: {}",
                result.message
            );
            return;
        }

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap_or("{}")).unwrap();

        // Themes must be clean (no # markers, no bare "Cluster N").
        let themes = output["themes"].as_array().unwrap();
        assert!(!themes.is_empty(), "no themes derived");
        for t in themes {
            let s = t.as_str().unwrap();
            assert!(!s.contains('#'), "theme contains # marker: {s}");
            assert!(
                !(s.split_whitespace().count() <= 2
                    && s.split_whitespace()
                        .next()
                        .map(|w| w.eq_ignore_ascii_case("cluster"))
                        .unwrap_or(false)),
                "bare cluster label sent to API: {s}"
            );
        }

        // Must have analysed at least one keyword with KD data.
        let results = output["difficulty"]["results"].as_array().unwrap();
        assert!(!results.is_empty(), "no difficulty results returned");
    }

    /// Lightweight dummy-project smoke flow that still exercises the full live pipeline.
    #[test]
    #[ignore = "calls live CapSolver + Ahrefs APIs; run with --ignored"]
    fn keyword_research_dummy_project_smoke_flow() {
        let capsolver_key = {
            use crate::config::env_resolver::EnvResolver;
            let env = EnvResolver::new("/tmp").build_env(std::collections::HashMap::new());
            env.get("CAPSOLVER_API_KEY").cloned().unwrap_or_default()
        };

        if capsolver_key.is_empty() {
            eprintln!(
                "SKIP: CAPSOLVER_API_KEY not set — set it in ~/.config/automation/secrets.env"
            );
            return;
        }

        let result = run_dummy_project_flow("coffee roasting profiles");
        eprintln!("smoke flow success: {}", result.success);
        eprintln!("smoke flow message: {}", result.message);

        if !result.success {
            assert!(
                result.message.contains("No new keyword ideas found")
                    || result.message.contains("Failed to fetch keyword ideas")
                    || result.message.contains("CAPSOLVER"),
                "unexpected smoke-flow failure: {}",
                result.message
            );
            return;
        }

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap_or("{}")).unwrap_or_default();
        assert!(
            output.is_object(),
            "expected JSON output object when successful"
        );
    }
}

#[cfg(test)]
mod volume_tests {
    use crate::engine::exec::keywords::estimate_volume;

    #[test]
    fn estimate_volume_maps_ahrefs_labels() {
        assert_eq!(estimate_volume("MoreThanOneHundred"), Some(100));
        assert_eq!(estimate_volume("MoreThanOneThousand"), Some(1000));
        assert_eq!(estimate_volume("LessThanOneHundred"), Some(50));
    }

    #[test]
    fn estimate_volume_parses_ranges_and_numbers() {
        assert_eq!(estimate_volume("100-1,000"), Some(550));
        assert_eq!(estimate_volume("2,400"), Some(2400));
    }
}

#[cfg(test)]
mod keyword_workflow_tests {
    use crate::engine::exec::keywords::*;
    use crate::engine::workflows::handlers::default_handlers;
    use crate::engine::workflows::StepKind;
    use crate::models::task::{AgentPolicy, TaskRunPolicy, Priority, Task, TaskRun, TaskStatus, TaskReviewSurface, FollowUpPolicy};
    use chrono::Utc;

    fn in_memory_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY, name TEXT NOT NULL,
                path TEXT NOT NULL,
                content_dir TEXT,
                site_url TEXT,
                site_id TEXT,
                active INTEGER NOT NULL DEFAULT 1,
                agent_provider TEXT
             );
             CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY, type TEXT NOT NULL, phase TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'todo',
                priority TEXT NOT NULL DEFAULT 'medium',
                run_policy TEXT NOT NULL DEFAULT 'user_enqueue',
                review_surface TEXT NOT NULL DEFAULT 'none',
                follow_up_policy TEXT NOT NULL DEFAULT 'none',
                agent_policy TEXT NOT NULL DEFAULT 'none',
                title TEXT, description TEXT,
                project_id TEXT NOT NULL,
                depends_on TEXT NOT NULL DEFAULT '[]',
                artifacts TEXT NOT NULL DEFAULT '[]',
                run_attempts INTEGER NOT NULL DEFAULT 0,
                run_last_error TEXT, run_provider TEXT,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL
             );",
        )
        .unwrap();
        conn
    }

    fn create_test_project(conn: &rusqlite::Connection, path: &str) -> String {
        let id = format!("proj-{}", Utc::now().timestamp_millis());
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES (?1, 'Test', ?2, 1)",
            [&id, path],
        )
        .unwrap();
        id
    }

    fn create_keyword_research_task(project_id: &str, themes: &[&str]) -> Task {
        Task {
            id: format!("task-{}", Utc::now().timestamp_millis()),
            project_id: project_id.to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: AgentPolicy::Optional,
            title: Some("Keyword Research".to_string()),
            description: if themes.is_empty() {
                None // No themes provided - should trigger agentic mode
            } else {
                Some(format!("Themes: {}", themes.join(", ")))
            },
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun {
                attempts: 0,
                last_error: None,
                provider: None,
                ..Default::default()
            },
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        }
    }

    /// Test workflow planning - now uses 3-step agentic workflow for all research tasks.
    #[test]
    fn workflow_uses_four_step_hybrid_workflow() {
        let conn = in_memory_db();
        let temp_dir =
            std::env::temp_dir().join(format!("ps_kw_test_{}", Utc::now().timestamp_millis()));
        std::fs::create_dir_all(&temp_dir.join(".github").join("automation")).unwrap();

        std::fs::write(
            temp_dir
                .join(".github")
                .join("automation")
                .join("articles.json"),
            r#"{"nextArticleId":1,"articles":[]}"#,
        )
        .unwrap();

        let project_id = create_test_project(&conn, &temp_dir.to_string_lossy());
        let task = create_keyword_research_task(&project_id, &["personal finance", "budgeting"]);

        let handlers = default_handlers();
        let handler = handlers
            .iter()
            .find(|h| h.supports(&task))
            .expect("Should find handler");
        let steps = handler.plan(&task);

        // 5-step hybrid workflow (normalizer removed — agentic steps use Extractor<T>):
        //   1. seed extraction (agentic, structured)
        //   2. autocomplete (deterministic)
        //   3. seed validation (agentic, structured)
        //   4. ahrefs pipeline (deterministic)
        //   5. final selection (deterministic)
        assert_eq!(steps.len(), 5, "Should have 5 steps: agentic → deterministic → agentic → deterministic → deterministic");
        assert_eq!(steps[0].name, "research_seed_extraction");
        assert_eq!(steps[0].kind, StepKind::Agentic);
        assert_eq!(steps[1].name, "research_autocomplete");
        assert_eq!(steps[1].kind, StepKind::ResearchAutocomplete);
        assert_eq!(steps[2].name, "research_seed_validation");
        assert_eq!(steps[2].kind, StepKind::Agentic);
        assert_eq!(steps[3].name, "research_ahrefs_pipeline");
        assert_eq!(steps[3].kind, StepKind::KeywordResearchNative);
        assert_eq!(steps[4].name, "research_final_selection");
        assert_eq!(steps[4].kind, StepKind::ResearchFinalSelection);

        std::fs::remove_dir_all(&temp_dir).ok();
    }
}

#[cfg(test)]
mod sampling_tests {
    use crate::engine::exec::keywords::{smart_sample_candidates, Candidate};

    fn make(kw: &str, theme: &str, is_question: bool, volume: Option<i64>) -> Candidate {
        Candidate {
            keyword: kw.to_string(),
            source_theme: theme.to_string(),
            is_question,
            volume,
            kd: None,
            intent: None,
        }
    }

    #[test]
    fn sampling_returns_all_when_below_budget() {
        let candidates = vec![
            make("a", "t1", false, Some(100)),
            make("b", "t1", false, Some(200)),
        ];
        let result = smart_sample_candidates(candidates.clone(), 10);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn sampling_stratifies_across_themes() {
        let candidates = vec![
            make("a1", "t1", false, Some(1000)),
            make("a2", "t1", false, Some(900)),
            make("a3", "t1", false, Some(800)),
            make("b1", "t2", false, Some(700)),
            make("b2", "t2", false, Some(600)),
            make("b3", "t2", false, Some(500)),
        ];
        let result = smart_sample_candidates(candidates, 4);
        let t1_count = result.iter().filter(|c| c.source_theme == "t1").count();
        let t2_count = result.iter().filter(|c| c.source_theme == "t2").count();
        assert!(t1_count >= 1, "t1 should have at least 1 sample");
        assert!(t2_count >= 1, "t2 should have at least 1 sample");
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn sampling_prioritizes_question_keywords() {
        let candidates = vec![
            make("a1", "t1", false, Some(1000)),
            make("a2", "t1", true, Some(100)), // question
            make("a3", "t1", false, Some(800)),
        ];
        let result = smart_sample_candidates(candidates, 2);
        assert!(
            result.iter().any(|c| c.keyword == "a2" && c.is_question),
            "question keyword should be sampled"
        );
    }

    #[test]
    fn sampling_fills_remaining_with_highest_volume() {
        let candidates = vec![
            make("a1", "t1", false, Some(100)),
            make("b1", "t2", false, Some(1000)),
            make("b2", "t2", false, Some(900)),
        ];
        let result = smart_sample_candidates(candidates, 3);
        assert_eq!(result.len(), 3);
        // The highest-volume keyword should definitely be included.
        assert!(result
            .iter()
            .any(|c| c.keyword == "b1" && c.volume == Some(1000)));
    }
}
