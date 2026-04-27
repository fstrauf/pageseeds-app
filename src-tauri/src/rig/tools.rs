//! Rig tool system wrappers.
//!
//! Re-exports rig's native tool types and provides helpers for building
//! `ToolSet` instances that can be attached to rig `Agent`s.

pub use rig::tool::ToolDyn;

use rig::tool::ToolSet;

/// Build a `ToolSet` from a collection of boxed tools.
///
/// This is a convenience helper so callers don't need to import `ToolSet`
/// directly from rig internals.
///
/// # Example
/// ```ignore
/// use crate::rig::tools::boxed_tool_set;
/// use rig::tool::ToolDyn;
///
/// let tools: Vec<Box<dyn ToolDyn>> = vec![
///     Box::new(MyTool),
/// ];
/// let tool_set = boxed_tool_set(tools);
/// let agent = client.agent(model)
///     .tools(tool_set)
///     .build();
/// ```
pub fn boxed_tool_set(tools: Vec<Box<dyn ToolDyn>>) -> ToolSet {
    ToolSet::from_tools_boxed(tools)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rig::completion::request::ToolDefinition;
    use rig::tool::Tool;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    /// A minimal test tool for verifying ToolSet construction.
    #[derive(Debug, Clone)]
    struct EchoTool;

    #[derive(Debug, Serialize, Deserialize, JsonSchema)]
    struct EchoArgs {
        message: String,
    }

    #[derive(Debug, Serialize, JsonSchema)]
    struct EchoOutput {
        echo: String,
    }

    impl Tool for EchoTool {
        const NAME: &'static str = "echo";
        type Error = std::convert::Infallible;
        type Args = EchoArgs;
        type Output = EchoOutput;

        async fn definition(&self, _prompt: String) -> ToolDefinition {
            ToolDefinition {
                name: Self::NAME.to_string(),
                description: "Echoes the input message".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" }
                    },
                    "required": ["message"]
                }),
            }
        }

        async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
            Ok(EchoOutput {
                echo: args.message,
            })
        }
    }

    #[test]
    fn test_boxed_tool_set_empty() {
        let tool_set = boxed_tool_set(vec![]);
        // ToolSet doesn't expose length, but we can verify it constructs without panic.
        let _ = tool_set;
    }

    #[test]
    fn test_boxed_tool_set_with_tools() {
        let tools: Vec<Box<dyn ToolDyn>> = vec![
            Box::new(EchoTool),
        ];
        let tool_set = boxed_tool_set(tools);
        let _ = tool_set;
    }

    #[test]
    fn test_echo_tool_definition() {
        let tool = EchoTool;
        let def = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(rig::tool::Tool::definition(&tool, "test".to_string()));
        assert_eq!(def.name, "echo");
        assert!(def.description.contains("Echoes"));
    }

    #[test]
    fn test_echo_tool_call() {
        let tool = EchoTool;
        let output = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(rig::tool::Tool::call(&tool, EchoArgs {
                message: "hello".to_string(),
            }))
            .unwrap();
        assert_eq!(output.echo, "hello");
    }
}
