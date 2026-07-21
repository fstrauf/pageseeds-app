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

// ─── Content Review User-Selection Proposals ─────────────────────────────────

/// Artifact key for the validated, selectable proposal list stored on the
/// content_review / content_audit parent after success. The picker and selection
/// command both read this key.
pub const CONTENT_REVIEW_PROPOSALS_KEY: &str = "content_review_proposals";

/// A single validated follow-up proposal the user can select in the picker.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ContentReviewProposal {
    /// Stable selection id (e.g. `fix_content_article:{article_id}`).
    pub id: String,
    pub task_type: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Spawn payload (article_id, article_title, article_file, url_slug,
    /// target_keyword, suggestions, priority).
    pub params: serde_json::Value,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
}

/// A raw proposal that failed validation, with the reason recorded for the UI.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct DroppedProposal {
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// Validated proposal list stored on the parent task after content_review.
/// Cap of 5 proposals is enforced by `engine::content_review_selection`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[ts(export)]
pub struct ContentReviewSelectableArtifact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub findings_summary: Option<String>,
    #[serde(default)]
    pub proposals: Vec<ContentReviewProposal>,
    #[serde(default)]
    pub dropped: Vec<DroppedProposal>,
    /// Source of the proposals: `"recommendations"` | `"investigation"` | etc.
    #[serde(default)]
    pub source: String,
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
