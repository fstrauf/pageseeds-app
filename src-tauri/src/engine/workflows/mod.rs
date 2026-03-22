pub mod handlers;

use serde::{Deserialize, Serialize};

/// A single step in a workflow plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub name: String,
    /// "deterministic" | "agentic" | "normalizer" | "manual"
    pub kind: String,
    pub params: std::collections::HashMap<String, String>,
    pub optional: bool,
}

impl WorkflowStep {
    pub fn new(name: &str, kind: &str) -> Self {
        Self {
            name: name.to_string(),
            kind: kind.to_string(),
            params: Default::default(),
            optional: false,
        }
    }

    pub fn with_param(mut self, key: &str, val: &str) -> Self {
        self.params.insert(key.to_string(), val.to_string());
        self
    }

    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }
}

/// Result returned by a workflow handler's execute call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub success: bool,
    pub message: String,
    /// Raw stdout/stderr captured from CLI invocations.
    pub output: Option<String>,
}
