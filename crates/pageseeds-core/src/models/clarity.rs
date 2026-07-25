use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Connection status returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ClarityConnectionStatus {
    pub connected: bool,
    pub message: String,
}

/// A single exported row exposed over IPC.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ClarityExportRowPayload {
    pub clarity_date: String,
    pub dimension_set: String,
    pub metric_name: String,
    pub dimensions: std::collections::HashMap<String, serde_json::Value>,
    pub values: std::collections::HashMap<String, serde_json::Value>,
}

/// A single finding exposed over IPC.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ClarityFindingPayload {
    pub issue_type: String,
    pub severity: String,
    pub url: String,
    pub evidence: String,
    pub recommendation: String,
    pub clarity_dashboard_url: String,
}

/// A finding that was skipped during follow-up task creation, with the reason.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ClaritySkippedFinding {
    pub issue_type: String,
    pub url: String,
    pub reason: String,
}

/// Result of creating follow-up tasks from selected Clarity findings.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ClarityTaskCreationResult {
    pub created_tasks: Vec<crate::models::task::Task>,
    pub skipped: Vec<ClaritySkippedFinding>,
}

/// Summary payload exposed over IPC.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ClaritySummaryPayload {
    pub project_id: String,
    pub generated_at: String,
    pub days_analyzed: i64,
    pub page_scores: Vec<ClarityPageScorePayload>,
    pub top_findings: Vec<ClarityFindingPayload>,
}

/// Page score payload exposed over IPC.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ClarityPageScorePayload {
    pub url: String,
    pub total_sessions: f64,
    pub rage_click_count: f64,
    pub dead_click_count: f64,
    pub quickback_count: f64,
    pub excessive_scroll_count: f64,
    pub error_click_count: f64,
    pub script_error_count: f64,
    pub avg_engagement_seconds: f64,
    pub avg_scroll_depth: f64,
    pub rage_click_rate: f64,
    pub dead_click_rate: f64,
    pub quickback_rate: f64,
    pub z_score: f64,
    pub clarity_dashboard_url: String,
}
