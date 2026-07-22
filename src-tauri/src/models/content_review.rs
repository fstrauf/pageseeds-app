use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Structured output from the content review recommendation agent.
/// Uses rig's Extractor<T> for type-safe generation via tool calling.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ContentReviewRecommendations {
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub total_articles: usize,
    #[serde(alias = "recommendations")]
    pub articles: Vec<ReviewArticleRecommendation>,
}

/// Single-article recommendations — used when processing one article at a time.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct SingleArticleRecommendations {
    #[serde(alias = "recommendations")]
    pub suggestions: Vec<ReviewSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ReviewArticleRecommendation {
    pub article_id: i64,
    pub article_title: String,
    #[serde(default)]
    pub article_file: String,
    #[serde(default)]
    pub url_slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_keyword: Option<String>,
    pub suggestions: Vec<ReviewSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
pub struct ReviewSuggestion {
    pub category: String,
    #[serde(alias = "current_text")]
    pub current: String,
    #[serde(alias = "proposed_replacement", alias = "suggested")]
    pub proposed: String,
    #[serde(alias = "issue", alias = "rationale")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
}

// ─── Content Fix Patch (structured replacement values) ───────────────────────

/// The agent returns this instead of raw MDX. Rust applies it deterministically.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, JsonSchema)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct ContentFixPatch {
    pub article_id: i64,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub changes: ContentFixChanges,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ts_rs::TS, JsonSchema)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct ContentFixChanges {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intro: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal_links: Option<Vec<ContentFixLink>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faq_questions: Option<Vec<ContentFixFaq>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eeat_signal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cta: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, JsonSchema)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct ContentFixLink {
    pub anchor_text: String,
    pub target_slug: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, JsonSchema)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct ContentFixFaq {
    pub question: String,
    pub answer: String,
}

// ─── Verification Report ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct ContentFixVerificationReport {
    pub summary: String,
    pub verified_count: usize,
    pub failed_count: usize,
    pub skipped_count: usize,
    pub fixes: Vec<ContentFixVerifiedItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct ContentFixVerifiedItem {
    pub category: String,
    pub status: String, // "verified" | "failed" | "skipped"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
}

// ─── Quality Gate Review ─────────────────────────────────────────────────────

/// Structured output from the article quality review agent.
/// Used by the review_article_quality task to gate articles before clustering/linking.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct ContentQualityReview {
    pub overall_pass: bool,
    #[serde(default)]
    pub usefulness_score: i64,
    #[serde(default)]
    pub image_score: i64,
    #[serde(default)]
    pub seo_score: i64,
    #[serde(default)]
    pub cluster_fit_score: i64,
    #[serde(default)]
    pub signal_score: Option<f64>,
    #[serde(default)]
    pub checks: Vec<QualityCheck>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct QualityCheck {
    pub id: String,
    pub label: String,
    pub pass: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// ─── Investigation Findings (content_review tool-calling path) ───────────────

/// Typed output from the content_review investigate step when the backend
/// supports tool calling. Stored as the `investigation_findings` artifact.
/// Does **not** write `recommendations.json` (so fix_content_article spawning
/// no-ops safely until a later issue wires proposed_tasks).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct InvestigationFindings {
    /// 1–2 sentence TL;DR of the investigation.
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<Finding>,
    /// Suggested downstream tasks (not validated or spawned by this step).
    #[serde(default)]
    pub proposed_tasks: Vec<ProposedTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct Finding {
    pub title: String,
    pub description: String,
    /// Tool-backed evidence supporting this finding.
    pub evidence: String,
    /// `critical` | `warning` | `info`
    pub severity: String,
    /// `auto_fixable` | `developer_actionable` | `hybrid` | `informational`
    pub fix_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct ProposedTask {
    /// Task type string from task_definitions (e.g. `ctr_audit`, `fix_content_article`).
    pub task_type: String,
    pub title: String,
    pub reason: String,
    /// Opaque task params; empty object when not needed.
    #[serde(default = "default_empty_object")]
    pub params: serde_json::Value,
}

fn default_empty_object() -> serde_json::Value {
    serde_json::json!({})
}
