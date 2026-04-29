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
