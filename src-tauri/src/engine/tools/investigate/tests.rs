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

    #[test]
    fn test_tool_definitions_smoke() {
        let ctx = InvestigationContext {
            project_id: "test".into(),
            project_path: ".".into(),
            db_path: ":memory:".into(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();

        let tools = investigation_tools(ctx);
        assert_eq!(tools.len(), 18);

        // Verify each tool's definition compiles
        for tool in &tools {
            let def = rt.block_on(async {
                rig::tool::ToolDyn::definition(tool.as_ref(), "test".to_string()).await
            });
            assert!(!def.name.is_empty(), "Tool name must not be empty");
            assert!(!def.description.is_empty(), "Tool description must not be empty for {}", def.name);
        }
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
}
