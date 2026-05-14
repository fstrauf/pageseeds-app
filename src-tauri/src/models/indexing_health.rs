/// Types for the unified indexing health campaign workflow.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Verdict from the agentic distinctiveness review step.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct DistinctivenessVerdict {
    pub target_url: String,
    pub verdict: String, // "DISTINCT" | "OVERLAP"
    pub confidence: String, // "high" | "medium" | "low"
    pub recommendation: String, // "MERGE" | "REWRITE" | "NO_ACTION"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirect_url: Option<String>,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_h1: Option<String>,
}

/// Per-target plan produced by the reduce step.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct IndexingTargetPlan {
    pub url: String,
    pub reason_code: String,
    pub recommended_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_artifact_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distinctiveness_verdict: Option<DistinctivenessVerdict>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_audit_summary: Option<serde_json::Value>,
}

/// The full campaign plan written by `ihc_reduce_plan`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct IndexingCampaignPlan {
    pub generated_at: String,
    pub targets: Vec<IndexingTargetPlan>,
    pub summary: IndexingCampaignSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct IndexingCampaignSummary {
    pub total_targets: usize,
    pub fix_content: usize,
    pub add_links: usize,
    pub merge: usize,
    pub rewrite_title_h1: usize,
    pub no_action: usize,
}

/// Result of a single prerequisite freshness check.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PrerequisiteCheck {
    pub artifact: String,
    pub fresh: bool,
    pub age_hours: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

/// Output of the `IhcCheckPrerequisites` step.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct PrerequisiteReport {
    pub all_fresh: bool,
    pub checks: Vec<PrerequisiteCheck>,
}

/// A potential source article for adding internal links.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LinkSourceCandidate {
    pub article_id: i64,
    pub slug: String,
    pub title: String,
    pub file: String,
    pub reason: String,
}

/// Per-target context built by `IhcBuildTargetContext`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct IndexingTargetContext {
    pub target: TargetArticleSummary,
    pub cluster: Option<ClusterContext>,
    pub diagnosis: TargetDiagnosis,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub source_candidates: Vec<LinkSourceCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TargetArticleSummary {
    pub url: String,
    pub slug: String,
    pub reason_code: String,
    pub title: String,
    pub h1: String,
    pub word_count: usize,
    pub incoming_links: usize,
    pub content_audit_health: String,
    pub article_id: i64,
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ClusterContext {
    pub cluster_id: String,
    pub theme: String,
    pub sibling_count: usize,
    pub siblings: Vec<SiblingArticle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_headings: Option<Vec<String>>,
    pub exact_keyword_dupe: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SiblingArticle {
    pub url: String,
    pub title: String,
    pub h1: String,
    pub word_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impressions: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct TargetDiagnosis {
    pub has_links: bool,
    pub is_long: bool,
    pub has_cluster_siblings: bool,
    pub suspected_root_cause: String,
}
