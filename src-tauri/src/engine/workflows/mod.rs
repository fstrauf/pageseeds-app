pub mod handlers;

use serde::{Deserialize, Serialize};

/// Param keys consumed by the executor's step dispatch logic.
/// Use these constants instead of inline string literals in handler `plan()` implementations.
///
/// - Keys used by `executor::run_step()` directly: `NORMALIZER_ID`, `CMD`, `ARTIFACT`
/// - Keys forwarded to `exec_agentic()`: `SKILL`
/// - Keys used by normalizer post-processing: `ARTIFACT_NAME`
pub mod step_params {
    /// Names the SKILL.md file to load for an `"agentic"` step.
    pub const SKILL: &str = "skill";
    /// Selects which normalizer to apply in a `"normalizer"` step.
    pub const NORMALIZER_ID: &str = "normalizer_id";
    /// Names the output artifact written by a `"normalizer"` step.
    pub const ARTIFACT_NAME: &str = "artifact_name";
    /// CLI command string for a `"deterministic"` step. Supports `{project_path}` and `{automation_dir}` tokens.
    pub const CMD: &str = "cmd";
    /// Artifact file name passed as context to an agentic step (e.g. `gsc_collection.json`).
    pub const ARTIFACT: &str = "artifact";
}

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
