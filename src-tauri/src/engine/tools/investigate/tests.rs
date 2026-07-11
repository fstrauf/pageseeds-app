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
}
