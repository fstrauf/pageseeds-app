//! Serde types for Site State desk tools (issue #117 / #120).
//!
//! Field set matches epic #117 JSON shapes. Soft-dependency fields for the
//! evidence index (#119) ship empty-safe defaults until that lands.

use serde::{Deserialize, Serialize};

/// Default GSC window length for desk rollups.
pub const DEFAULT_PERIOD_DAYS: i64 = 28;

/// Cap full article body payloads so tool results stay token-friendly.
pub const BODY_SIZE_CAP: usize = 40_000;

/// Marker appended when body is truncated at [`BODY_SIZE_CAP`].
pub const BODY_TRUNCATION_NOTE: &str =
    "\n\n<!-- truncated: body continues beyond size cap -->";

// ── site_overview ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteOverview {
    pub project_id: String,
    pub generated_at: String,
    pub freshness: Freshness,
    pub totals: SiteTotals,
    pub top_pages: Vec<TopPage>,
    pub top_movers: Vec<TopMover>,
    pub not_indexed_sample: Vec<NotIndexedSample>,
    /// Deterministic flag strings only (no soft-cluster prose).
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Freshness {
    /// Newest GSC-related fetch timestamp (query metrics and/or page daily).
    pub gsc_at: Option<String>,
    /// Always null until evidence index (#119).
    pub evidence_index_at: Option<String>,
    /// Always 0.0 until evidence index (#119).
    pub evidence_coverage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteTotals {
    pub articles_live: usize,
    pub articles_redirected: usize,
    pub impressions: f64,
    pub clicks: f64,
    pub avg_ctr: f64,
    pub not_indexed: usize,
    /// Best-effort; 0 when link scan is not run (expensive for overview).
    pub orphans: usize,
    /// Stub: 0 until content_audit is wired into desk totals.
    pub validation_failures: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopPage {
    pub article_id: i64,
    pub slug: String,
    pub title: String,
    pub impressions: f64,
    pub clicks: f64,
    pub ctr: f64,
    pub avg_position: f64,
    pub target_keyword: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopMover {
    pub slug: String,
    pub clicks_delta: f64,
    pub impressions_delta: f64,
    /// "up" | "down" | "flat"
    pub direction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotIndexedSample {
    pub slug: String,
    pub reason: String,
}

// ── articles catalog ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct ArticlesFilter {
    pub status: Option<String>,
    pub min_impressions: f64,
    pub include_redirected: bool,
    pub limit: Option<usize>,
    pub period_days: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticlesCatalog {
    pub project_id: String,
    pub generated_at: String,
    pub filter: ArticlesFilterEcho,
    pub count: usize,
    pub articles: Vec<ArticleCatalogRow>,
}

/// Echo of the applied filter for agent transparency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticlesFilterEcho {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub min_impressions: f64,
    pub include_redirected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleCatalogRow {
    pub article_id: i64,
    pub slug: String,
    pub url: String,
    pub title: String,
    pub h1: Option<String>,
    pub target_keyword: Option<String>,
    /// Reserved until Phase intent extract; always null in #120.
    pub intent_card: Option<serde_json::Value>,
    pub status: String,
    pub published_at: Option<String>,
    pub last_edited_at: Option<String>,
    pub word_count: i64,
    pub serp: SerpFields,
    pub gsc: GscRollup,
    pub top_queries: Vec<QueryMetric>,
    pub links: LinkCounts,
    pub indexing_status: Option<String>,
    /// Empty until evidence index (#119).
    pub neighbors: Vec<serde_json::Value>,
    pub evidence: EvidenceStub,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerpFields {
    pub title: String,
    pub title_len: usize,
    pub meta_description: Option<String>,
    pub meta_len: usize,
    pub has_faq: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GscRollup {
    pub impressions: f64,
    pub clicks: f64,
    pub ctr: f64,
    pub avg_position: f64,
    pub period_days: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryMetric {
    pub query: String,
    pub impressions: f64,
    pub clicks: f64,
    pub avg_position: f64,
    pub ctr: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LinkCounts {
    pub inbound: i64,
    pub outbound: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceStub {
    pub content_hash: Option<String>,
    pub indexed_at: Option<String>,
    pub embedding_model: Option<String>,
    pub has_embedding: bool,
}

// ── article package ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticlePackage {
    pub article_id: i64,
    pub slug: String,
    pub catalog: ArticleCatalogRow,
    pub content: ArticleContent,
    pub queries: Vec<QueryMetric>,
    pub query_cannibalization: Vec<QueryCannibalization>,
    /// Empty until evidence index (#119); never null.
    pub neighbors: Vec<serde_json::Value>,
    pub validation: ValidationStub,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleContent {
    pub file: String,
    pub frontmatter: serde_json::Value,
    pub body_markdown: String,
    pub outline: Vec<OutlineHeading>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutlineHeading {
    pub level: usize,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryCannibalization {
    pub query: String,
    pub other_slugs: Vec<CannibalSlugMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CannibalSlugMetric {
    pub slug: String,
    pub impressions: f64,
    pub clicks: f64,
}

/// Stub only for #120 — full validation checks come later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationStub {
    pub ok: bool,
    pub checks: Vec<serde_json::Value>,
}
