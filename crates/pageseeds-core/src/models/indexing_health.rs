/// Types for the unified indexing health campaign workflow.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Verdict from the agentic distinctiveness review step.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
    /// Content audit word count for this URL (0 = not in tracked content)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub word_count: Option<usize>,
    /// Internal incoming links from link scan
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incoming_links: Option<usize>,
    /// Source MDX file path if tracked in content audit; None if only in GSC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

/// The full campaign plan written by `ihc_reduce_plan`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IndexingCampaignPlan {
    pub generated_at: String,
    pub targets: Vec<IndexingTargetPlan>,
    pub summary: IndexingCampaignSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IndexingCampaignSummary {
    pub total_targets: usize,
    pub fix_content: usize,
    pub add_links: usize,
    pub merge: usize,
    pub rewrite_title_h1: usize,
    /// Fallback fix_indexing targets (mapped to concrete child tasks at spawn time)
    #[serde(default)]
    pub fix_indexing: usize,
    pub no_action: usize,
}

/// Result of a single prerequisite freshness check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrerequisiteCheck {
    pub artifact: String,
    pub fresh: bool,
    pub age_hours: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

/// Output of the `IhcCheckPrerequisites` step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrerequisiteReport {
    pub all_fresh: bool,
    pub checks: Vec<PrerequisiteCheck>,
}

/// A potential source article for adding internal links.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkSourceCandidate {
    pub article_id: i64,
    pub slug: String,
    pub title: String,
    pub file: String,
    pub reason: String,
}

/// Pipeline outcome for `fix_indexing_internal_links` plan/apply → verify.
///
/// Intentional no-ops (`NoCandidates`, `AlreadyLinked`) must pass verify without
/// requiring an inbound-link increase. Only `Applied` requires growth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexingLinkOutcome {
    /// At least one source file was modified to add a link.
    Applied,
    /// Planned sources already linked to the target — nothing to do.
    AlreadyLinked,
    /// No usable source candidates (empty shortlist or none remaining after filter).
    NoCandidates,
}

impl IndexingLinkOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Applied => "applied",
            Self::AlreadyLinked => "already_linked",
            Self::NoCandidates => "no_candidates",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "applied" => Some(Self::Applied),
            "already_linked" => Some(Self::AlreadyLinked),
            "no_candidates" => Some(Self::NoCandidates),
            _ => None,
        }
    }

    /// Non-error intentional no-ops — verify should pass without inbound growth.
    pub fn is_intentional_noop(self) -> bool {
        matches!(self, Self::AlreadyLinked | Self::NoCandidates)
    }
}

/// Target payload inside the `indexing_link_target` task artifact.
///
/// Shape fields are stable across IHC children, GSC recovery, and operator slug spawn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingLinkTarget {
    pub url: String,
    pub slug: String,
    pub article_id: i64,
    pub file: String,
    pub reason_code: String,
    pub incoming_link_count_before: usize,
    pub target_keyword: String,
    #[serde(default)]
    pub source_candidates: Vec<LinkSourceCandidate>,
}

/// Full `indexing_link_target` artifact document (`{ campaign_task_id, target }`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingLinkTargetArtifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub campaign_task_id: Option<String>,
    pub target: IndexingLinkTarget,
}

/// Per-target context built by `IhcBuildTargetContext`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingTargetContext {
    pub target: TargetArticleSummary,
    pub cluster: Option<ClusterContext>,
    pub diagnosis: TargetDiagnosis,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub source_candidates: Vec<LinkSourceCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetArticleSummary {
    pub url: String,
    pub slug: String,
    pub reason_code: String,
    pub title: String,
    pub h1: String,
    pub target_keyword: String,
    pub word_count: usize,
    pub incoming_links: usize,
    pub content_audit_health: String,
    pub article_id: i64,
    pub file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterContext {
    pub cluster_id: String,
    pub theme: String,
    pub sibling_count: usize,
    pub siblings: Vec<SiblingArticle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_headings: Option<Vec<String>>,
    pub exact_keyword_dupe: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiblingArticle {
    pub url: String,
    pub title: String,
    pub h1: String,
    pub word_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impressions: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetDiagnosis {
    pub has_links: bool,
    pub is_long: bool,
    pub has_cluster_siblings: bool,
    pub suspected_root_cause: String,
}
