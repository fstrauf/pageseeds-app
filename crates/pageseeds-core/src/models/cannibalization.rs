/// Typed models for cannibalization strategy review and approval.
///
/// These cross the Tauri IPC boundary to the frontend review UI.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

// ─── Enums ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum Confidence {
    High,
    Medium,
    #[default]
    Low,
}

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Confidence::High => "high",
            Confidence::Medium => "medium",
            Confidence::Low => "low",
        }
    }
}

impl std::fmt::Display for Confidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for Confidence {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for Confidence {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "high" => Ok(Confidence::High),
            "medium" => Ok(Confidence::Medium),
            _ => Ok(Confidence::Low),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ApprovalStatus {
    #[default]
    Pending,
    Approved,
    Rejected,
    NeedsReview,
}

impl ApprovalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalStatus::Pending => "pending",
            ApprovalStatus::Approved => "approved",
            ApprovalStatus::Rejected => "rejected",
            ApprovalStatus::NeedsReview => "needs_review",
        }
    }
}

impl std::fmt::Display for ApprovalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for ApprovalStatus {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for ApprovalStatus {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "approved" => Ok(ApprovalStatus::Approved),
            "rejected" => Ok(ApprovalStatus::Rejected),
            "needs_review" => Ok(ApprovalStatus::NeedsReview),
            _ => Ok(ApprovalStatus::Pending),
        }
    }
}

// ─── Recommendation structs ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct MergeRecommendation {
    pub cluster_id: String,
    pub confidence: Confidence,
    pub keep_url: String,
    pub redirect_urls: Vec<String>,
    #[serde(default)]
    pub merge_before_redirect: bool,
    #[serde(default)]
    pub merge_instructions: Vec<String>,
    pub reason: String,
    #[serde(default)]
    pub approval_status: ApprovalStatus,
}

/// Raw agent output for a single merge candidate analysis.
/// Used by `extract_structured` in the cannibalization audit pipeline.
///
/// **The agent selects pages by stable article `id` only — it never emits URL
/// strings.** URL materialization is a deterministic step in the workflow
/// (`exec_can_analyze_candidates` resolves these ids to canonical
/// `/blog/<slug>` paths via `slug::format_blog_link`). This prevents the agent
/// from introducing malformed or non-resolvable slugs (e.g. underscores) into
/// merge plans that feed 301 redirects.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct CandidateAnalysisOutput {
    #[serde(default)]
    pub cluster_id: String,
    /// The `id` of the page to keep (must exist in the candidate's `pages`).
    #[serde(default)]
    pub keep_id: i64,
    /// The `id`s of the pages to 301-redirect into the keeper.
    #[serde(default)]
    pub redirect_ids: Vec<i64>,
    #[serde(default)]
    pub merge_before_redirect: bool,
    #[serde(default)]
    pub merge_instructions: Vec<String>,
    #[serde(default)]
    pub reason: String,
    /// Accepts both boolean and object (e.g. {"reason": "..."}) for backward
    /// compatibility with prompts that instruct the agent to return an object.
    #[serde(default, deserialize_with = "deserialize_no_action")]
    pub no_action: bool,
    #[serde(default)]
    pub confidence: String,
    #[serde(default)]
    pub cluster_theme: String,
}

fn deserialize_no_action<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Bool(b) => Ok(b),
        serde_json::Value::Object(_) => Ok(true),
        serde_json::Value::Null => Ok(false),
        _ => Ok(false),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct HubRecommendation {
    pub topic: String,
    pub suggested_url: String,
    pub suggested_title: String,
    pub intent: String,
    #[serde(default)]
    pub source_pages: Vec<i64>,
    #[serde(default, alias = "articles_to_link")]
    pub spoke_pages: Vec<i64>,
    #[serde(default)]
    pub outline: Vec<String>,
    #[serde(default)]
    pub approval_status: ApprovalStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct CalculatorRecommendation {
    pub strategy: String,
    pub ticker_universe: String,
    #[serde(default)]
    pub priority_tickers: Vec<String>,
    pub indexing_policy: String,
    pub reason: String,
    #[serde(default)]
    pub approval_status: ApprovalStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct TerritoryRecommendation {
    pub theme: String,
    pub priority: String,
    #[serde(default)]
    pub demand_evidence: Vec<String>,
    #[serde(default)]
    pub suggested_tasks: Vec<String>,
    #[serde(default)]
    pub approval_status: ApprovalStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct StrategyRisk {
    pub risk: String,
    pub mitigation: String,
}

// ─── Top-level strategy ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct CannibalizationStrategy {
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub merge_recommendations: Vec<MergeRecommendation>,
    #[serde(default)]
    pub hub_recommendations: Vec<HubRecommendation>,
    #[serde(default)]
    pub calculator_recommendations: Vec<CalculatorRecommendation>,
    #[serde(default)]
    pub territory_recommendations: Vec<TerritoryRecommendation>,
    #[serde(default)]
    pub risks: Vec<StrategyRisk>,
}

// ─── Territory Strategy ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct TerritoryStrategy {
    pub theme: String,
    pub priority: String,
    #[serde(default)]
    pub target_keywords: Vec<String>,
    #[serde(default)]
    pub competitor_gaps: Vec<String>,
    #[serde(default)]
    pub content_recommendations: Vec<TerritoryContentRec>,
    #[serde(default)]
    pub existing_coverage: Vec<TerritoryCoverageItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct TerritoryContentRec {
    pub title: String,
    pub url_slug: String,
    pub intent: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct TerritoryCoverageItem {
    pub article_id: i64,
    pub title: String,
    pub url_slug: String,
    pub overlap: String,
}

// ─── Selection input for task-drawer picker ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct CannibalizationSelection {
    pub recommendation_type: String,
    pub recommendation_id: String,
}

// ─── Review state (DB row) ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct StrategyReview {
    pub id: i64,
    pub strategy_id: String,
    pub project_id: String,
    pub recommendation_type: String,
    pub recommendation_id: String,
    pub approval_status: ApprovalStatus,
    pub approved_by: Option<String>,
    pub approved_at: Option<String>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ─── Frontend view model ──────────────────────────────────────────────────────

/// Status of an existing fix task for a recommendation.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct RecommendationTaskStatus {
    pub recommendation_type: String,
    pub recommendation_id: String,
    pub task_id: Option<String>,
    pub task_status: Option<String>,
}

/// A strategy with per-recommendation approval state merged in.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct StrategyWithReviews {
    pub strategy: CannibalizationStrategy,
    pub reviews: Vec<StrategyReview>,
    #[serde(default)]
    pub task_statuses: Vec<RecommendationTaskStatus>,
    pub strategy_id: String,
    pub project_id: String,
}
