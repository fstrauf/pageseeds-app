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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
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
