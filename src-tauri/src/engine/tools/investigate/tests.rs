use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::engine::project_paths::ProjectPaths;
use super::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_context_open_db_invalid_path() {
        let ctx = InvestigationContext {
            project_id: "test".into(),
            project_path: ".".into(),
            db_path: "/nonexistent/test.db".into(),
        };
        assert!(ctx.open_db().is_err());
    }

    async fn tool_names(tools: &[Box<dyn rig::tool::ToolDyn>]) -> Vec<String> {
        let mut names = Vec::with_capacity(tools.len());
        for tool in tools {
            let def = rig::tool::ToolDyn::definition(tool.as_ref(), "test".to_string()).await;
            names.push(def.name);
        }
        names
    }

    fn test_ctx() -> InvestigationContext {
        InvestigationContext {
            project_id: "test".into(),
            project_path: ".".into(),
            db_path: ":memory:".into(),
        }
    }

    #[test]
    fn test_tool_definitions_smoke() {
        let ctx = test_ctx();
        let rt = tokio::runtime::Runtime::new().unwrap();

        let tools = investigation_tools(ctx);
        assert_eq!(tools.len(), 19);

        // Verify each tool's definition compiles
        for tool in &tools {
            let def = rt.block_on(async {
                rig::tool::ToolDyn::definition(tool.as_ref(), "test".to_string()).await
            });
            assert!(!def.name.is_empty(), "Tool name must not be empty");
            assert!(!def.description.is_empty(), "Tool description must not be empty for {}", def.name);
        }
    }

    #[test]
    fn test_investigation_read_only_tools_excludes_mutators() {
        let ctx = test_ctx();
        let rt = tokio::runtime::Runtime::new().unwrap();

        let full = investigation_tools(ctx.clone());
        assert_eq!(full.len(), 19, "full set must remain 19 tools");

        let ro = investigation_read_only_tools(ctx);
        assert_eq!(ro.len(), 15, "read-only set must be 15 tools");

        let names = rt.block_on(tool_names(&ro));
        for mutator in [
            "create_task",
            "enqueue_task",
            "run_content_audit",
            "write_feature_spec",
        ] {
            assert!(
                !names.iter().any(|n| n == mutator),
                "RO set must not include mutator {mutator}; got {names:?}"
            );
        }
        assert!(
            names.iter().any(|n| n == "get_task_status"),
            "RO set must include get_task_status; got {names:?}"
        );
    }

    #[test]
    fn test_kit_catalog_and_tools_aligned_per_mode() {
        let ctx = test_ctx();
        let rt = tokio::runtime::Runtime::new().unwrap();

        for access in [InvestigationAccess::Full, InvestigationAccess::ReadOnly] {
            let kit = investigation_kit(ctx.clone(), access);
            let expected = inventory_names(access);
            let actual = rt.block_on(tool_names(&kit.tools));

            assert_eq!(
                actual.len(),
                expected.len(),
                "{access:?}: tool count mismatch"
            );
            assert_eq!(
                actual, expected,
                "{access:?}: registered tool names must match inventory order"
            );

            for name in &expected {
                assert!(
                    kit.catalog.contains(&format!("[tools.{name}]")),
                    "{access:?}: catalog missing section for tool {name}"
                );
            }

            // No extra mutator sections in RO catalog
            if access == InvestigationAccess::ReadOnly {
                for mutator in [
                    "create_task",
                    "enqueue_task",
                    "run_content_audit",
                    "write_feature_spec",
                ] {
                    assert!(
                        !kit.catalog.contains(&format!("[tools.{mutator}]")),
                        "RO catalog must not advertise mutator {mutator}"
                    );
                }
                assert!(!kit.catalog.contains("mutates = true"));
            } else {
                assert!(kit.catalog.contains("mutates = true"));
                assert!(kit.catalog.contains("[tools.get_task_status]"));
            }
        }

        assert_eq!(inventory_names(InvestigationAccess::Full).len(), 19);
        assert_eq!(inventory_names(InvestigationAccess::ReadOnly).len(), 15);
    }

    #[test]
    fn test_wrappers_match_kit() {
        let ctx = test_ctx();
        let rt = tokio::runtime::Runtime::new().unwrap();

        let full_tools = rt.block_on(tool_names(&investigation_tools(ctx.clone())));
        let full_kit = rt.block_on(tool_names(
            &investigation_kit(ctx.clone(), InvestigationAccess::Full).tools,
        ));
        assert_eq!(full_tools, full_kit);

        let ro_tools = rt.block_on(tool_names(&investigation_read_only_tools(ctx.clone())));
        let ro_kit = rt.block_on(tool_names(
            &investigation_kit(ctx, InvestigationAccess::ReadOnly).tools,
        ));
        assert_eq!(ro_tools, ro_kit);
    }

    fn temp_db_path(suffix: &str) -> String {
        let dir = std::env::current_dir().unwrap();
        let name = format!("pageseeds_test_{}_{}.db", std::process::id(), suffix);
        dir.join(name).to_string_lossy().to_string()
    }

    #[test]
    fn test_list_research_shortlist_empty() {
        let db_path = temp_db_path("shortlist");
        let _ = std::fs::remove_file(&db_path);
        let ctx = InvestigationContext {
            project_id: "proj1".into(),
            project_path: ".".into(),
            db_path: db_path.clone(),
        };
        let db = ctx.open_db().unwrap();
        db.execute_batch(
            "CREATE TABLE research_shortlist (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT NOT NULL,
                theme TEXT NOT NULL,
                seeds TEXT NOT NULL DEFAULT '[]',
                source TEXT NOT NULL,
                status TEXT NOT NULL,
                priority TEXT NOT NULL,
                article_count INTEGER,
                total_impressions REAL,
                signal_score REAL,
                health_status TEXT NOT NULL,
                last_reviewed_at TEXT,
                added_at TEXT NOT NULL,
                researched_at TEXT,
                covered_at TEXT
            );"
        ).unwrap();
        drop(db);

        let result = list_research_shortlist(&ctx, None, None).unwrap();
        assert_eq!(result["count"].as_i64(), Some(0));
        assert_eq!(result["summary"]["pending"].as_i64(), Some(0));
        assert_eq!(result["summary"]["depleted"].as_i64(), Some(0));

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn test_list_article_quality_reviews_empty() {
        let db_path = temp_db_path("reviews");
        let _ = std::fs::remove_file(&db_path);
        let ctx = InvestigationContext {
            project_id: "proj1".into(),
            project_path: ".".into(),
            db_path: db_path.clone(),
        };
        let db = ctx.open_db().unwrap();
        db.execute_batch(
            "CREATE TABLE article_quality_reviews (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT NOT NULL,
                task_id TEXT NOT NULL,
                article_file TEXT NOT NULL,
                overall_pass INTEGER NOT NULL,
                scores_json TEXT NOT NULL,
                checks_json TEXT NOT NULL,
                reviewed_at TEXT NOT NULL
            );"
        ).unwrap();
        drop(db);

        let result = list_article_quality_reviews(&ctx, 10).unwrap();
        assert_eq!(result["count"].as_i64(), Some(0));
        assert_eq!(result["passed"].as_i64(), Some(0));
        assert_eq!(result["failed"].as_i64(), Some(0));

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn create_task_tool_rejects_fix_content_article_without_slug() {
        let db_path = temp_db_path("create_task_no_slug");
        let _ = std::fs::remove_file(&db_path);
        let ctx = InvestigationContext {
            project_id: "proj1".into(),
            project_path: ".".into(),
            db_path: db_path.clone(),
        };
        // Open once so the file exists; CreateTaskTool opens its own connection.
        let _ = ctx.open_db().unwrap();

        let tool = CreateTaskTool { ctx };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(tool.call(CreateTaskArgs {
                task_type: "fix_content_article".into(),
                title: "Fix something".into(),
                reason: "because".into(),
                slug: None,
            }))
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("requires slug"),
            "expected slug requirement error, got: {msg}"
        );

        let _ = std::fs::remove_file(&db_path);
    }

    #[test]
    fn create_task_tool_rejects_fix_content_article_with_empty_slug() {
        let db_path = temp_db_path("create_task_empty_slug");
        let _ = std::fs::remove_file(&db_path);
        let ctx = InvestigationContext {
            project_id: "proj1".into(),
            project_path: ".".into(),
            db_path: db_path.clone(),
        };
        let _ = ctx.open_db().unwrap();

        let tool = CreateTaskTool { ctx };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(tool.call(CreateTaskArgs {
                task_type: "fix_content_article".into(),
                title: "Fix something".into(),
                reason: "because".into(),
                slug: Some("   ".into()),
            }))
            .unwrap_err();
        assert!(err.to_string().contains("requires slug"));

        let _ = std::fs::remove_file(&db_path);
    }
}
