//! Residual GSC inventories for site overview (#261).
//!
//! Pure inventory pipelines:
//! - redirect-source residual equity (mapped destinations)
//! - high-impression GSC pages outside live catalog and redirect map
//!
//! Called from [`super::builders::build_site_overview`] orchestration only.

use std::collections::{HashMap, HashSet};

use crate::db::{rollup_for_slug, GscDailyWindowMetrics};

use super::types::{
    NonCatalogGscInventory, NonCatalogGscSample, RedirectEquityInventory, RedirectEquitySample,
    NON_CATALOG_GSC_MIN_IMPRESSIONS, OVERVIEW_INVENTORY_SAMPLE_CAP,
};

/// Residual GSC still attributed to redirect sources, mapped to destinations (#261).
pub(crate) fn build_redirect_equity_inventory(
    redirect_map: &HashMap<String, String>,
    page_index: &HashMap<String, Vec<String>>,
    recent_by_page: &HashMap<String, GscDailyWindowMetrics>,
    live_catalog_slugs: &HashSet<String>,
) -> RedirectEquityInventory {
    let mut candidates: Vec<RedirectEquitySample> = Vec::new();
    for (source_slug, dest_slug) in redirect_map {
        let (source_metrics, _) = rollup_for_slug(page_index, recent_by_page, source_slug);
        let Some(src) = source_metrics else {
            continue;
        };
        // Skip zero residual impressions (noise / no tape on source).
        if src.impressions <= 0.0 {
            continue;
        }
        let (dest_metrics, _) = rollup_for_slug(page_index, recent_by_page, dest_slug);
        let (dest_impr, dest_clicks) = match dest_metrics {
            Some(m) => (m.impressions, m.clicks),
            None => (0.0, 0.0),
        };
        candidates.push(RedirectEquitySample {
            source_slug: source_slug.clone(),
            destination_slug: dest_slug.clone(),
            source_impressions: src.impressions,
            source_clicks: src.clicks,
            destination_impressions: dest_impr,
            destination_clicks: dest_clicks,
            destination_in_catalog: live_catalog_slugs.contains(dest_slug),
        });
    }
    candidates.sort_by(|a, b| {
        b.source_impressions
            .partial_cmp(&a.source_impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let count = candidates.len();
    candidates.truncate(OVERVIEW_INVENTORY_SAMPLE_CAP);
    RedirectEquityInventory {
        count,
        sample: candidates,
    }
}

/// High-impression GSC pages outside live catalog and redirect map (#261).
pub(crate) fn build_non_catalog_gsc_inventory(
    page_index: &HashMap<String, Vec<String>>,
    recent_by_page: &HashMap<String, GscDailyWindowMetrics>,
    live_catalog_slugs: &HashSet<String>,
    redirect_sources: &HashSet<String>,
) -> NonCatalogGscInventory {
    let mut candidates: Vec<NonCatalogGscSample> = Vec::new();
    for slug in page_index.keys() {
        if live_catalog_slugs.contains(slug) {
            continue;
        }
        // Mapped redirect sources belong in redirect_equity only.
        if redirect_sources.contains(slug) {
            continue;
        }
        let (metrics, _) = rollup_for_slug(page_index, recent_by_page, slug);
        let Some(m) = metrics else {
            continue;
        };
        if m.impressions < NON_CATALOG_GSC_MIN_IMPRESSIONS {
            continue;
        }
        candidates.push(NonCatalogGscSample {
            slug: slug.clone(),
            impressions: m.impressions,
            clicks: m.clicks,
        });
    }
    candidates.sort_by(|a, b| {
        b.impressions
            .partial_cmp(&a.impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let count = candidates.len();
    candidates.truncate(OVERVIEW_INVENTORY_SAMPLE_CAP);
    NonCatalogGscInventory {
        count,
        sample: candidates,
    }
}
