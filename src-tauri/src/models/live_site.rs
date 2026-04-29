use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct LiveSitePage {
    pub url: String,
    pub path: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_excerpt: Option<String>,
    #[serde(default)]
    pub word_count: i64,
    #[serde(default)]
    pub heading_count: i64,
    #[serde(default)]
    pub internal_links_out: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gsc_clicks: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gsc_impressions: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gsc_ctr: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gsc_position: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gsc_synced_at: Option<String>,
    pub last_crawled_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct LiveSiteImportResult {
    pub sitemap_url: String,
    pub discovered_urls: usize,
    pub pages_imported: usize,
    pub links_imported: usize,
    pub pages_failed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct LiveSiteGscSyncResult {
    pub site_url: String,
    pub start_date: String,
    pub end_date: String,
    pub rows_fetched: usize,
    pub pages_synced: usize,
    pub pages_unmatched: usize,
    pub synced_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct LiveSiteAuditSummary {
    pub total_pages: usize,
    pub healthy_pages: usize,
    pub pages_with_issues: usize,
    pub thin_content_pages: usize,
    pub missing_metadata_pages: usize,
    pub weak_heading_pages: usize,
    pub stale_crawl_pages: usize,
    pub weak_interlinking_pages: usize,
    pub orphan_pages: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct LiveSiteAuditPage {
    pub url: String,
    pub path: String,
    pub title: String,
    pub word_count: i64,
    pub heading_count: i64,
    pub internal_links_out: i64,
    pub internal_links_in: i64,
    pub has_meta_description: bool,
    pub has_h1: bool,
    pub last_crawled_at: String,
    pub crawl_age_days: i64,
    pub issue_flags: Vec<String>,
    pub issue_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct LiveSiteAuditReport {
    pub summary: LiveSiteAuditSummary,
    pub pages: Vec<LiveSiteAuditPage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct LiveSiteLinkProfile {
    pub url: String,
    pub path: String,
    pub title: String,
    pub outgoing_urls: Vec<String>,
    pub incoming_urls: Vec<String>,
    pub unresolved_links: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct LiveSiteLinkScanResult {
    pub total_pages: usize,
    pub total_internal_links: usize,
    pub pages_with_outgoing: usize,
    pub pages_with_incoming: usize,
    pub orphan_urls: Vec<String>,
    pub profiles: Vec<LiveSiteLinkProfile>,
}
