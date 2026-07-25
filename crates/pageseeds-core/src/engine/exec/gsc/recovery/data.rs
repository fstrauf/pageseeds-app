use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::gsc::{DriftUrl, GscDriftReport, ResubmitCandidate};
use crate::models::task::Task;
use super::*;
// ─── Data structs for plan JSON ───────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct RecoveryPlan {
    pub(crate) generated_at: String,
    pub(crate) project_id: String,
    pub(crate) data_freshness: PlanFreshness,
    pub(crate) summary: PlanSummary,
    pub(crate) targets: Vec<RecoveryTarget>,
    pub(crate) skipped: Vec<SkippedTarget>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PlanFreshness {
    pub(crate) gsc_collected_at: Option<String>,
    pub(crate) gsc_data_age_hours: Option<u64>,
    pub(crate) link_scan_age_hours: Option<u64>,
    pub(crate) sitemap_fetched_at: Option<String>,
    pub(crate) partial_gsc_collection: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct PlanSummary {
    pub(crate) sitemap_total: usize,
    pub(crate) gsc_total: usize,
    pub(crate) eligible_targets: usize,
    pub(crate) skipped_targets: usize,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct RecoveryTarget {
    pub(crate) url: String,
    pub(crate) slug: String,
    pub(crate) article_id: i64,
    pub(crate) file: String,
    pub(crate) reason_code: String,
    pub(crate) priority_score: i64,
    pub(crate) priority_reason: String,
    pub(crate) incoming_link_count_before: usize,
    pub(crate) target_keyword: String,
    pub(crate) published_date: String,
    pub(crate) source_candidates: Vec<SourceCandidate>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SourceCandidate {
    pub(crate) article_id: i64,
    pub(crate) file: String,
    pub(crate) title: String,
    pub(crate) slug: String,
    pub(crate) score: i64,
    pub(crate) gsc_impressions: i64,
    pub(crate) reason: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct SkippedTarget {
    pub(crate) url: String,
    pub(crate) reason_code: String,
    pub(crate) skip_reason: String,
}

