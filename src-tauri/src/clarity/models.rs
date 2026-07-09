use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A single metric block returned by the Clarity Export API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarityMetricBlock {
    #[serde(rename = "metricName")]
    pub metric_name: String,
    pub information: Vec<ClarityDataPoint>,
}

/// One row inside a metric block. Dimension fields vary by request, so we keep
/// the raw value and expose helpers for the dimensions/values we care about.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarityDataPoint {
    /// Dimension keys and values that were requested (e.g. URL, Device).
    #[serde(flatten)]
    pub fields: HashMap<String, serde_json::Value>,
}

impl ClarityDataPoint {
    /// Known dimension names in the Clarity Export API.
    const DIMENSION_KEYS: &[&str] = &[
        "Browser",
        "Device",
        "Country/Region",
        "OS",
        "Source",
        "Medium",
        "Campaign",
        "Channel",
        "URL",
        "Page Title",
        "Referrer URL",
    ];

    /// Split the flat Clarity row into (dimensions, metric_values).
    pub fn split(&self) -> (HashMap<String, serde_json::Value>, HashMap<String, serde_json::Value>) {
        let mut dimensions = HashMap::new();
        let mut values = HashMap::new();
        for (k, v) in &self.fields {
            if Self::DIMENSION_KEYS.contains(&k.as_str()) {
                dimensions.insert(k.clone(), v.clone());
            } else {
                values.insert(k.clone(), v.clone());
            }
        }
        (dimensions, values)
    }
}

/// Normalised metric name variants we expect from the API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClarityMetric {
    Traffic,
    ScrollDepth,
    EngagementTime,
    PopularPages,
    DeadClickCount,
    ExcessiveScroll,
    RageClickCount,
    QuickbackClick,
    ScriptErrorCount,
    ErrorClickCount,
    Other,
}

impl ClarityMetric {
    pub fn from_api_name(name: &str) -> Self {
        match name {
            "Traffic" => Self::Traffic,
            "Scroll Depth" | "ScrollDepth" => Self::ScrollDepth,
            "Engagement Time" | "EngagementTime" => Self::EngagementTime,
            "Popular Pages" | "PopularPages" => Self::PopularPages,
            "Dead Click Count" | "DeadClickCount" => Self::DeadClickCount,
            "Excessive Scroll" | "ExcessiveScroll" => Self::ExcessiveScroll,
            "Rage Click Count" | "RageClickCount" => Self::RageClickCount,
            "Quickback Click" | "QuickbackClick" => Self::QuickbackClick,
            "Script Error Count" | "ScriptErrorCount" => Self::ScriptErrorCount,
            "Error Click Count" | "ErrorClickCount" => Self::ErrorClickCount,
            _ => Self::Other,
        }
    }

    #[allow(dead_code)]
    pub fn as_api_name(&self) -> &'static str {
        match self {
            Self::Traffic => "Traffic",
            Self::ScrollDepth => "Scroll Depth",
            Self::EngagementTime => "Engagement Time",
            Self::PopularPages => "Popular Pages",
            Self::DeadClickCount => "Dead Click Count",
            Self::ExcessiveScroll => "Excessive Scroll",
            Self::RageClickCount => "Rage Click Count",
            Self::QuickbackClick => "Quickback Click",
            Self::ScriptErrorCount => "Script Error Count",
            Self::ErrorClickCount => "Error Click Count",
            Self::Other => "Other",
        }
    }
}

/// Requested dimension names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ClarityDimension {
    Browser,
    Device,
    #[serde(rename = "Country/Region")]
    CountryRegion,
    OS,
    Source,
    Medium,
    Campaign,
    Channel,
    URL,
}

impl ClarityDimension {
    pub fn as_api_name(&self) -> &'static str {
        match self {
            Self::Browser => "Browser",
            Self::Device => "Device",
            Self::CountryRegion => "Country/Region",
            Self::OS => "OS",
            Self::Source => "Source",
            Self::Medium => "Medium",
            Self::Campaign => "Campaign",
            Self::Channel => "Channel",
            Self::URL => "URL",
        }
    }
}

/// One flattened export row ready for SQLite storage or JSON export.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarityExportRow {
    pub clarity_date: String,
    pub dimension_set: String,
    pub metric_name: String,
    pub dimensions: HashMap<String, serde_json::Value>,
    pub values: HashMap<String, serde_json::Value>,
}

impl ClarityExportRow {
    pub fn from_data_point(
        clarity_date: &str,
        dimension_set: &str,
        metric_name: &str,
        point: &ClarityDataPoint,
    ) -> Self {
        let (dimensions, values) = point.split();
        Self {
            clarity_date: clarity_date.to_string(),
            dimension_set: dimension_set.to_string(),
            metric_name: metric_name.to_string(),
            dimensions,
            values,
        }
    }
}

/// Metadata for a collection run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarityCollectionMeta {
    pub project_id: String,
    pub exported_at: String,
    pub days: u8,
    pub requests_made: usize,
}

/// Full collection artifact written to the repo automation dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarityCollection {
    pub meta: ClarityCollectionMeta,
    pub rows: Vec<ClarityExportRow>,
}

/// Summary statistics for a single page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarityPageScore {
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

/// A single agentic finding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarityFinding {
    pub issue_type: String,
    pub severity: String,
    pub url: String,
    pub evidence: String,
    pub recommendation: String,
    pub clarity_dashboard_url: String,
}

/// Full summary artifact written to the repo automation dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaritySummary {
    pub meta: ClaritySummaryMeta,
    pub page_scores: Vec<ClarityPageScore>,
    pub top_findings: Vec<ClarityFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaritySummaryMeta {
    pub project_id: String,
    pub generated_at: String,
    pub days_analyzed: i64,
}

/// Result returned from the agentic investigation step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarityInvestigationResult {
    pub findings: Vec<ClarityFinding>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_point_splits_dimensions_and_values() {
        let mut fields = HashMap::new();
        fields.insert("URL".to_string(), serde_json::json!("/pricing"));
        fields.insert("Device".to_string(), serde_json::json!("Mobile"));
        fields.insert("totalSessionCount".to_string(), serde_json::json!("1200"));
        fields.insert("rageClickCount".to_string(), serde_json::json!(45));

        let point = ClarityDataPoint { fields };
        let (dims, vals) = point.split();

        assert_eq!(dims.get("URL").unwrap().as_str().unwrap(), "/pricing");
        assert_eq!(dims.get("Device").unwrap().as_str().unwrap(), "Mobile");
        assert_eq!(vals.get("totalSessionCount").unwrap().as_str().unwrap(), "1200");
        assert_eq!(vals.get("rageClickCount").unwrap().as_i64().unwrap(), 45);
    }

    #[test]
    fn export_row_from_data_point_preserves_structure() {
        let mut fields = HashMap::new();
        fields.insert("URL".to_string(), serde_json::json!("/blog"));
        fields.insert("totalSessionCount".to_string(), serde_json::json!("500"));

        let point = ClarityDataPoint { fields };
        let row = ClarityExportRow::from_data_point("2026-06-29", "url", "Traffic", &point);

        assert_eq!(row.clarity_date, "2026-06-29");
        assert_eq!(row.dimension_set, "url");
        assert_eq!(row.metric_name, "Traffic");
        assert_eq!(row.dimensions.get("URL").unwrap().as_str().unwrap(), "/blog");
        assert_eq!(row.values.get("totalSessionCount").unwrap().as_str().unwrap(), "500");
    }

    #[test]
    fn metric_from_api_name_maps_known_values() {
        assert!(matches!(
            ClarityMetric::from_api_name("Rage Click Count"),
            ClarityMetric::RageClickCount
        ));
        assert!(matches!(
            ClarityMetric::from_api_name("Quickback Click"),
            ClarityMetric::QuickbackClick
        ));
        assert!(matches!(
            ClarityMetric::from_api_name("Unknown Metric"),
            ClarityMetric::Other
        ));
    }
}
