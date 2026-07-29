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

// ── site_overview desk inventory thresholds (issues #204 / #205) ──────────────

/// Inclusive lower bound of avg position for striking-distance inventory.
pub const STRIKING_POS_MIN: f64 = 7.0;
/// Inclusive upper bound of avg position for striking-distance inventory.
pub const STRIKING_POS_MAX: f64 = 13.0;
/// Minimum recent-window impressions for striking-distance inventory.
pub const STRIKING_MIN_IMPRESSIONS: f64 = 200.0;

// Hard same-query floor/cap live in `db::ctr_query` (shared with cannibalization
// audit): `SHARED_QUERY_MIN_IMPRESSIONS` / `SHARED_QUERY_MAX_PAGES`.
/// Sample size cap shared by zero-impression, striking-distance, hard-cannibal,
/// redirect-equity, and non-catalog GSC inventory groups.
pub const OVERVIEW_INVENTORY_SAMPLE_CAP: usize = 10;

/// Minimum recent-window impressions for non-catalog residual GSC inventory (#261).
pub const NON_CATALOG_GSC_MIN_IMPRESSIONS: f64 = 50.0;

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
    /// Published live articles with zero recent-window impressions (#204).
    pub zero_impression: ZeroImpressionInventory,
    /// Live articles in the striking-distance position band (#204).
    pub striking_distance: StrikingDistanceInventory,
    /// Hard same-query multi-URL cannibal samples from `ctr_query_metrics` (#204).
    pub hard_cannibalization: HardCannibalizationInventory,
    /// Residual GSC on redirect sources with map destination metrics (#261).
    pub redirect_equity: RedirectEquityInventory,
    /// High-impression GSC pages outside live catalog and redirect map (#261).
    pub non_catalog_gsc: NonCatalogGscInventory,
    /// Deterministic flag strings only (no soft-cluster prose).
    pub hints: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZeroImpressionInventory {
    pub count: usize,
    pub sample: Vec<ZeroImpressionSample>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZeroImpressionSample {
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrikingDistanceInventory {
    pub count: usize,
    pub sample: Vec<StrikingDistanceSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrikingDistanceSample {
    pub slug: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub impressions: f64,
    pub clicks: f64,
    pub avg_position: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctr: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardCannibalizationInventory {
    pub count: usize,
    pub sample: Vec<HardCannibalizationSample>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardCannibalizationSample {
    pub query: String,
    pub slugs: Vec<HardCannibalSlugMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardCannibalSlugMetric {
    pub slug: String,
    pub impressions: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clicks: Option<f64>,
}

/// Residual demand still landing on redirect sources (#261).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RedirectEquityInventory {
    pub count: usize,
    pub sample: Vec<RedirectEquitySample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedirectEquitySample {
    pub source_slug: String,
    pub destination_slug: String,
    pub source_impressions: f64,
    pub source_clicks: f64,
    pub destination_impressions: f64,
    pub destination_clicks: f64,
    /// True when the destination slug is a live (non-redirected) catalog article.
    pub destination_in_catalog: bool,
}

/// High-impression GSC pages not in live catalog and not mapped redirects (#261).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NonCatalogGscInventory {
    pub count: usize,
    pub sample: Vec<NonCatalogGscSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonCatalogGscSample {
    /// Normalized slug (or page URL identity when slug extraction is empty).
    pub slug: String,
    pub impressions: f64,
    pub clicks: f64,
    /// `"redirect_source_missing_map"` | `"unknown"`.
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Freshness {
    /// Newest `gsc_page_daily.fetched_at` for the project (desk tape only).
    pub gsc_at: Option<String>,
    /// Whole days since [`Self::gsc_at`], or null when no tape / unparseable.
    pub age_days: Option<i64>,
    /// True when tape is missing or older than `GSC_METRICS_MAX_AGE_DAYS` (7).
    pub stale: bool,
    /// `"gsc_page_daily"` when any rows exist, else `"none"`.
    pub source: String,
    /// Recovery guidance when [`Self::stale`]; omitted when fresh.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
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
    pub freshness: Freshness,
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
    /// Count of distinct GSC page URLs that normalize to this catalog slug
    /// (underscore/hyphen/trailing-slash variants). 0 when no metrics / no pages.
    pub url_variants: usize,
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
