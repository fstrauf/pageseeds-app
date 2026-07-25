use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct PageMetrics {
    pub page: String,
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct QueryMetrics {
    pub query: String,
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
}

/// Combined page + query metrics from a single GSC Search Analytics call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageQueryMetrics {
    pub page: String,
    pub query: String,
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
}

/// Per-page, per-day metrics from a `["page", "date"]` Search Analytics pull.
/// Stored append-only in `gsc_page_daily` — the time series behind
/// before/after outcome measurement (issue #23).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageDailyMetrics {
    pub page: String,
    pub date: String,
    pub clicks: f64,
    pub impressions: f64,
    pub ctr: f64,
    pub position: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct MoverMetrics {
    pub key: String,
    pub current_clicks: f64,
    pub current_impressions: f64,
    pub current_position: f64,
    pub previous_clicks: f64,
    pub previous_impressions: f64,
    pub previous_position: f64,
    pub clicks_delta: f64,
    pub impressions_delta: f64,
    pub position_delta: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct InspectionRecord {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexing_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub robots_txt_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_fetch_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crawl_allowed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexing_allowed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_crawl_time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_canonical: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_canonical: Option<String>,
    #[serde(default)]
    pub sitemaps: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    pub priority: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct Coverage404Record {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_crawled: Option<String>,
    pub category: String,
    pub reason: String,
    pub priority: i32,
    pub suggested_action: String,
    #[serde(default)]
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct RedirectRecord {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_crawled: Option<String>,
    pub redirect_type: String,
    pub issue: String,
    pub priority: i32,
    pub suggested_action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub final_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct GscAuthStatus {
    pub service_account_configured: bool,
    pub oauth_configured: bool,
    pub authenticated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sa_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oauth_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TokenState {
    pub access_token: String,
    pub expires_at: i64,
}

impl TokenState {
    pub fn is_expired(&self) -> bool {
        chrono::Utc::now().timestamp() >= self.expires_at - 60
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// GSC Drift Detection
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct GscDriftReport {
    pub site_url: String,
    pub sitemap_url: String,
    pub checked_at: String,
    pub sitemap_total: usize,
    pub gsc_total: usize,
    pub indexed_count: usize,
    pub not_indexed_count: usize,
    pub in_sitemap_not_in_gsc: Vec<DriftUrl>,
    /// Sitemap URLs that were never sent to the URL Inspection API because the
    /// collection run hit its inspection cap (issue #26). Informational only —
    /// these are NOT resubmit candidates and do not feed the indexing campaign.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coverage_capped_uninspected: Vec<DriftUrl>,
    pub in_gsc_not_in_sitemap: Vec<DriftUrl>,
    pub not_indexed: Vec<DriftUrl>,
    pub resubmit_priority: Vec<ResubmitCandidate>,
    /// Hours since gsc_collection.json was last written. None if the file does not exist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gsc_data_age_hours: Option<i32>,
    /// Hours since link_scan.json was last written. None if the file does not exist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_scan_age_hours: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct DriftUrl {
    pub url: String,
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    /// Sitemap `<lastmod>` value when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lastmod: Option<String>,
    /// Whether a matching MDX content file exists for this URL.
    pub has_content_file: bool,
    /// Frontmatter or structural issues that may prevent indexing (e.g. "noindex", "missing meta description").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct ResubmitCandidate {
    pub url: String,
    pub slug: String,
    pub reason_code: String,
    pub priority_score: i32,
    pub priority_reason: String,
    pub has_internal_links: bool,
    pub incoming_link_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gsc_impressions: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_keyword: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_date: Option<String>,
    /// Latest recovery history status for this URL (linked, pending, resolved, failed, or null).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct RecoveryStats {
    pub total_attempts: usize,
    pub linked: usize,
    pub resolved: usize,
    pub failed: usize,
    pub total_links_added: usize,
}
