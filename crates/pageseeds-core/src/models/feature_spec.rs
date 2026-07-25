//! Structured output schema for agentic feature spec generation.
//!
//! The LLM agent returns a JSON array of `FeatureSpecFinding` objects.
//! Each finding is then verified deterministically before being rendered
//! into the final markdown spec.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A single finding discovered by the agentic investigation.
/// The agent proposes these; the verification layer confirms or rejects them.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeatureSpecFinding {
    /// Priority: P0 (code), P1 (content), or P2 (structural)
    pub priority: String,

    /// Machine-readable issue type
    pub issue_type: String,

    /// Human-readable description of the problem
    pub description: String,

    /// URL slugs of affected pages
    pub affected_slugs: Vec<String>,

    /// Which tool calls support this finding
    pub evidence_tool_calls: Vec<String>,

    /// Suggested fix or migration plan
    pub suggested_fix: String,

    /// Agent confidence 0.0–1.0
    pub confidence: f32,
}

/// The agent's complete structured response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FeatureSpecAgentOutput {
    /// Executive summary (2–3 sentences)
    pub executive_summary: String,

    /// Discovered findings
    pub findings: Vec<FeatureSpecFinding>,

    /// Tech stack detected from observations (not source files).
    /// Not returned by agent — set deterministically after parsing.
    #[serde(default)]
    pub tech_stack: String,
}

/// A verified finding that has passed deterministic cross-checks.
#[derive(Debug, Clone)]
pub struct VerifiedFinding {
    pub priority: String,
    pub issue_type: String,
    pub description: String,
    pub affected_slugs: Vec<String>,
    pub evidence: Vec<VerifiedEvidence>,
    /// Original agent evidence (file paths, line numbers, exact samples)
    pub evidence_tool_calls: Vec<String>,
    pub suggested_fix: String,
}

/// Ground-truth evidence confirming one aspect of a finding.
#[derive(Debug, Clone)]
pub struct VerifiedEvidence {
    pub slug: String,
    pub metric: String,
    pub value: String,
}
