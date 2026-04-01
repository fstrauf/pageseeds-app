/// Tool system for agentic workflows
///
/// Tools provide capabilities that agents can invoke to perform actions
/// like calling external APIs (Ahrefs, etc.).
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// Result of executing a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub data: Value,
    pub error: Option<String>,
}

impl ToolResult {
    pub fn success(data: Value) -> Self {
        Self {
            success: true,
            data,
            error: None,
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            success: false,
            data: Value::Null,
            error: Some(message.into()),
        }
    }
}

/// Tool trait - implement this for each tool
/// Uses Pin<Box<dyn Future>> instead of async_trait for dyn compatibility
pub trait Tool: Send + Sync {
    /// Tool name (used by agent to reference it)
    fn name(&self) -> &str;

    /// Tool description (shown to agent)
    fn description(&self) -> &str;

    /// JSON Schema for tool parameters
    fn parameters_schema(&self) -> Value;

    /// Execute the tool with given parameters
    fn execute(&self, params: Value) -> Pin<Box<dyn Future<Output = ToolResult> + Send + '_>>;
}

/// Registry of available tools
#[derive(Default)]
pub struct ToolRegistry {
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool
    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        let name = tool.name().to_string();
        self.tools.insert(name, Arc::new(tool));
    }

    /// Get a tool by name
    pub fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools.get(name).cloned()
    }

    /// List all available tool names
    pub fn list(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }

    /// Convert tools to OpenAI function schema format
    pub fn to_openai_schema(&self) -> Vec<Value> {
        self.tools
            .values()
            .map(|tool| {
                json!({
                    "type": "function",
                    "function": {
                        "name": tool.name(),
                        "description": tool.description(),
                        "parameters": tool.parameters_schema(),
                    }
                })
            })
            .collect()
    }
}

// Re-export tool implementations
mod keywords;
pub use keywords::{KeywordGeneratorTool, KeywordDifficultyTool};
