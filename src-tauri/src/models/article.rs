use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Article {
    pub id: i64,
    pub title: String,
    pub url_slug: String,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_keyword: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keyword_difficulty: Option<String>,
    #[serde(default)]
    pub target_volume: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published_date: Option<String>,
    #[serde(default)]
    pub word_count: i64,
    pub status: String,
    #[serde(default)]
    pub content_gaps_addressed: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub estimated_traffic_monthly: Option<String>,
    pub project_id: String,
}
