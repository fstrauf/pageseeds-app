pub mod handlers;
pub mod step_kind;

use serde::{Deserialize, Serialize};
use std::str::FromStr;
pub use step_kind::StepKind;

/// Param keys consumed by the executor's step dispatch logic.
/// Use these constants instead of inline string literals in handler `plan()` implementations.
///
/// - Keys forwarded to `exec_agentic()`: `SKILL`
/// - Keys used for artifact naming: `ARTIFACT_NAME`
/// - Artifact file name passed as context to an agentic step: `ARTIFACT`
pub mod step_params {
    /// Names the SKILL.md file to load for an `"agentic"` step.
    pub const SKILL: &str = "skill";
    /// Names the output artifact written by a step.
    pub const ARTIFACT_NAME: &str = "artifact_name";
    /// Artifact file name passed as context to an agentic step (e.g. `gsc_collection.json`).
    pub const ARTIFACT: &str = "artifact";
}

/// Policy for how a step affects the `latest_raw_output` pipeline variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LatestRawPolicy {
    /// Keep the existing `latest_raw_output` from a previous step (default).
    #[default]
    Preserve,
    /// Replace `latest_raw_output` with this step's `output`.
    ReplaceWithOutput,
    /// Clear `latest_raw_output` so downstream steps see `None`.
    Clear,
}

/// A declarative prompt section an agentic step wants appended to its prompt.
///
/// Issue #4 stage C: prompt-assembly policy lives in the step plan, not in
/// `exec_agentic`. Handlers declare which sections a step needs;
/// `exec_agentic` iterates this list and resolves each section's content from
/// runtime context (content dir, target path, hub briefs). Sections are
/// assembled in declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PromptSection {
    /// Publish-date, file-format, link-format, and target-file directives for
    /// content tasks. `new_article` selects the "required date + exact target
    /// path" variant (new articles) over the "preserve date + approximate
    /// filename hint" variant (optimize/fix tasks), and also drives the
    /// executor-write fallback for text-only providers.
    ContentDirectives { new_article: bool },
    /// Hub spoke context (briefs gathered from SQLite) plus the hub page
    /// requirements block. Also suppresses the bulky `cannibalization_strategy`
    /// artifact from the generic task-artifacts section.
    HubDirectives,
}

/// A single step in a workflow plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub name: String,
    pub kind: StepKind,
    pub params: std::collections::HashMap<String, String>,
    pub optional: bool,
    pub latest_raw_policy: LatestRawPolicy,
    /// Prompt sections this step declares for `exec_agentic` to assemble.
    /// Empty for steps whose prompt is just the skill body + artifacts.
    #[serde(default)]
    pub prompt_sections: Vec<PromptSection>,
}

impl WorkflowStep {
    pub fn new(name: &str, kind: StepKind) -> Self {
        // Agentic steps typically produce output that downstream steps consume.
        let latest_raw_policy = match kind {
            StepKind::Agentic | StepKind::CtrAnalyze | StepKind::CtrFixGenerate => {
                LatestRawPolicy::ReplaceWithOutput
            }
            _ => LatestRawPolicy::Preserve,
        };
        Self {
            name: name.to_string(),
            kind,
            params: Default::default(),
            optional: false,
            latest_raw_policy,
            prompt_sections: Vec::new(),
        }
    }

    pub fn from_kind_str(name: &str, kind: &str) -> Self {
        let parsed =
            StepKind::from_str(kind).unwrap_or_else(|_| panic!("Unknown step kind '{}'", kind));
        Self::new(name, parsed)
    }

    pub fn with_param(mut self, key: &str, val: &str) -> Self {
        self.params.insert(key.to_string(), val.to_string());
        self
    }

    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    pub fn with_latest_raw_policy(mut self, policy: LatestRawPolicy) -> Self {
        self.latest_raw_policy = policy;
        self
    }

    pub fn with_prompt_section(mut self, section: PromptSection) -> Self {
        self.prompt_sections.push(section);
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

impl StepResult {
    /// Create a failure result with no output.
    pub fn fail(message: impl Into<String>) -> Self {
        StepResult {
            success: false,
            message: message.into(),
            output: None,
        }
    }

    /// Create a success result with no output.
    pub fn ok(message: impl Into<String>) -> Self {
        StepResult {
            success: true,
            message: message.into(),
            output: None,
        }
    }

    /// Create a success result with output.
    pub fn ok_with_output(message: impl Into<String>, output: impl Into<String>) -> Self {
        StepResult {
            success: true,
            message: message.into(),
            output: Some(output.into()),
        }
    }
}
