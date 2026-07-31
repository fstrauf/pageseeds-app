//! CLI Path B research: session owns themes/seeds; deterministic pull + select.
//!
//! Mirrors [`write_package`]: domain logic lives here, CLI is thin flags + JSON.
//! Uses existing `custom_keyword_research` (no nested seed extraction/validation LLM).
//!
//! Flow:
//!   research-context → ensure shortlist fresh (territory when empty/stale) →
//!   session proposes seeds → research-pull → select-keywords → write Path B
//!
//! No LLM calls live in this module.
//! Shortlist refresh side effects live in [`super::research_shortlist_refresh`];
//! [`build_research_strategy_package`] stays pure read. Prefer
//! [`build_research_context`] for the full CLI envelope.

use std::collections::HashSet;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::research_shortlist::{self, ResearchShortlistEntry};
use crate::engine::keyword_selection::{
    extract_selectable_keywords, find_research_selection_artifact, parse_artifact_json,
};
use crate::engine::spawner::{DeduplicationPolicy, TaskSpec, TaskSpawner};
use crate::engine::task_store;
use crate::models::research::FilterFunnel;
use crate::models::task::{AgentPolicy, Priority, Task, TaskStatus};
use crate::strategy::{ContentStrategySummary, StrategyLoadStatus};

// Re-export refresh / re-annotate surface so callers can use `research_package::*` paths.
pub use crate::engine::research_shortlist_refresh::{
    ensure_research_shortlist_fresh, inject_gsc_uncovered_seeds, inject_strategy_shortlist_seeds,
    reannotate_shortlist_strategy, shortlist_refresh_reason, ShortlistRefreshResult,
    MAX_GSC_UNCOVERED_INJECTS, MAX_STRATEGY_SHORTLIST_INJECTS, MIN_UNCOVERED_QUERY_IMPRESSIONS,
    RESEARCH_SHORTLIST_MAX_AGE_DAYS,
};

// ─── Strategy package ────────────────────────────────────────────────────────

/// Compact shortlist row for session strategy (not full DB row).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortlistSummaryEntry {
    pub id: Option<i64>,
    pub theme: String,
    pub seeds: Vec<String>,
    pub source: String,
    pub status: String,
    pub priority: String,
    pub health_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_impressions: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub article_count: Option<i64>,
    /// Strategy cluster the theme maps to (project.md), when matched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy_cluster: Option<String>,
    /// Lifecycle status of the matched cluster: active/maintain/legacy/planned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy_status: Option<String>,
}

impl From<&ResearchShortlistEntry> for ShortlistSummaryEntry {
    fn from(e: &ResearchShortlistEntry) -> Self {
        Self {
            id: e.id,
            theme: e.theme.clone(),
            seeds: e.seeds.clone(),
            source: e.source.clone(),
            status: e.status.clone(),
            priority: e.priority.clone(),
            health_status: e.health_status.clone(),
            signal_score: e.signal_score,
            total_impressions: e.total_impressions,
            article_count: e.article_count,
            strategy_cluster: e.strategy_cluster.clone(),
            strategy_status: e.strategy_status.clone(),
        }
    }
}

/// Counts by shortlist health_status and workflow status.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShortlistHealthCounts {
    pub promising: usize,
    pub unproven: usize,
    pub depleted: usize,
    pub pending: usize,
    pub researched: usize,
    pub covered: usize,
}

/// Deterministic package for session strategy before proposing seeds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchStrategyPackage {
    pub project_id: String,
    pub shortlist: Vec<ShortlistSummaryEntry>,
    pub health_counts: ShortlistHealthCounts,
    /// Active research tasks (todo / queued / in_progress / review), if any.
    pub open_research_task_ids: Vec<String>,
    /// Static operator guidance (no LLM).
    pub guidance: Vec<String>,
    /// Structured content strategy from `project.md` (Search Keywords + clusters).
    /// Always present; fields may be empty when project.md is missing or incomplete.
    /// Named `content_strategy` to avoid clashing with this outer strategy package.
    pub content_strategy: ContentStrategySummary,
}

/// Static operator guidance for research strategy packages.
///
/// Order is intentional (#275): Primary/ACTIVE new-article seed rule first,
/// then never do_not_expand/LEGACY, then shortlist health as prioritization /
/// fallback only — never equal-priority new-article seed source when strategy
/// is present. Desk/shortlist/brand are judgment aids / empty-strategy fallback.
const STRATEGY_GUIDANCE: &[&str] = &[
    "For new-article research: seed research-pull from content_strategy.primary_keywords + ACTIVE cluster keywords first when present (intentional PLANNED pillars OK when expanding a planned pillar).",
    "Never seed or expand do_not_expand phrases or LEGACY clusters — even if API volume ranks them high.",
    "Shortlist health for prioritization and fallback only when strategy empty or Primary/ACTIVE exhausted/covered: prefer health_status=promising and status=pending; avoid depleted; deprioritize MAINTAIN vs ACTIVE/primary. Do not treat top shortlist by impressions/promising as default new-article seeds when Primary or ACTIVE exist.",
    "Desk (site-overview/articles/GSC), shortlist, and brand are judgment aids for existing-page fixes and strategy-empty fallback — not equal-priority new-article seed peers when strategy is present.",
    "Pull candidates with research-pull -K \"seed1,seed2,...\" (deterministic custom_keyword_research; no nested theme LLM).",
    "After pull, reject LEGACY / do_not_expand candidates before select-keywords; then select-keywords -I <task-id> -K kw1,kw2 (max 3), then write-context / write-submit Path B.",
    "Desktop research_keywords (nested seed extraction) remains available for UI; prefer research-pull on CLI weekly path.",
];

/// Dynamic line when strategy has Primary/ACTIVE fuel but pending shortlist is
/// empty or only adjacent (no `strategy_status=active` pending rows).
const STRATEGY_ADJACENT_SHORTLIST_GUIDANCE: &str = "Strategy Primary/ACTIVE present but pending shortlist is adjacent-only or empty — seed research-pull from content_strategy.primary_keywords / ACTIVE clusters, not territory GSC heads.";

/// True when summary has Primary keywords or any ACTIVE cluster keywords.
fn strategy_has_primary_or_active(cs: &ContentStrategySummary) -> bool {
    !cs.primary_keywords.is_empty()
        || cs
            .active_clusters
            .iter()
            .any(|c| !c.keywords.is_empty())
}

/// True when any pending shortlist row is tagged `strategy_status == "active"`.
fn pending_shortlist_has_active(shortlist: &[ShortlistSummaryEntry]) -> bool {
    shortlist.iter().any(|e| {
        e.status == "pending" && e.strategy_status.as_deref() == Some("active")
    })
}

/// Recovery guidance when strategy is empty/partial (#276).
///
/// Prepended as `guidance[0]` so operators and weekly agents see that hard
/// gates / ACTIVE boosts are silent no-ops — research still succeeds.
fn strategy_status_recovery_guidance(status: StrategyLoadStatus) -> String {
    format!(
        "content_strategy.status is {} — hard gates and ACTIVE boosts are no-ops until project.md matches the Search Keywords + Content Clusters (STATUS) contract (see strategy module docs / pageseeds-cli strategy). Fix before trusting research-pull selection.",
        status.as_str()
    )
}

/// Build static + optional dynamic guidance for a research strategy package.
fn build_strategy_guidance(
    content_strategy: &ContentStrategySummary,
    shortlist: &[ShortlistSummaryEntry],
) -> Vec<String> {
    let mut guidance: Vec<String> = Vec::new();
    // #276: loud recovery first when strategy is empty or partial.
    if matches!(
        content_strategy.status,
        StrategyLoadStatus::Empty | StrategyLoadStatus::Partial
    ) {
        guidance.push(strategy_status_recovery_guidance(content_strategy.status));
    }
    guidance.extend(STRATEGY_GUIDANCE.iter().map(|s| (*s).to_string()));
    if strategy_has_primary_or_active(content_strategy) {
        let pending_empty = !shortlist.iter().any(|e| e.status == "pending");
        if pending_empty || !pending_shortlist_has_active(shortlist) {
            guidance.push(STRATEGY_ADJACENT_SHORTLIST_GUIDANCE.to_string());
        }
    }
    guidance
}

/// Full Path B `research-context` envelope: pure strategy package + shortlist refresh fields.
///
/// Serializes to a flat JSON object (strategy fields flattened) matching the prior CLI merge shape:
/// `shortlist_refreshed`, `shortlist_refresh_reason`, optional `territory`, optional
/// `shortlist_refresh_error`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchContextPackage {
    #[serde(flatten)]
    pub strategy: ResearchStrategyPackage,
    pub shortlist_refreshed: bool,
    pub shortlist_refresh_reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub territory: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shortlist_refresh_error: Option<String>,
}

/// Ensure shortlist freshness then build the pure strategy package into one envelope.
///
/// Side effects live in [`super::research_shortlist_refresh`]:
/// [`ensure_research_shortlist_fresh`], [`reannotate_shortlist_strategy`],
/// [`inject_strategy_shortlist_seeds`], and [`inject_gsc_uncovered_seeds`] so
/// `project.md` edits show up without waiting for the 7-day territory TTL
/// (issue #258), Primary/ACTIVE strategy bullets become pending research fuel
/// even with 0 GSC impressions (issue #274), and aggregated query-level GSC
/// demand without article/strategy coverage seeds the shortlist (issue #304).
/// CLI should call this and `serde_json::to_value` only — no package field
/// composition in the binary.
pub fn build_research_context(
    conn: &Connection,
    project_id: &str,
    max_age_days: i64,
) -> Result<ResearchContextPackage, String> {
    let refresh = ensure_research_shortlist_fresh(conn, project_id, max_age_days);
    // Always re-annotate strategy columns from live project.md (no-op when empty).
    let _ = reannotate_shortlist_strategy(conn, project_id);
    // Always inject Primary/ACTIVE strategy seeds (runs even when territory is
    // skipped_fresh — must not be gated solely on empty/stale territory).
    let _ = inject_strategy_shortlist_seeds(conn, project_id);
    // Always inject uncovered GSC query demand (issue #304).
    let _ = inject_gsc_uncovered_seeds(conn, project_id);
    let strategy = build_research_strategy_package(conn, project_id)?;
    Ok(ResearchContextPackage {
        strategy,
        shortlist_refreshed: refresh.shortlist_refreshed,
        shortlist_refresh_reason: refresh.shortlist_refresh_reason,
        territory: refresh.territory,
        shortlist_refresh_error: refresh.error,
    })
}

/// Build a strategy package from research_shortlist + open research tasks.
/// No LLM / no shortlist writes. Prefer [`build_research_context`] when strategy
/// re-annotation and shortlist refresh side effects are desired.
pub fn build_research_strategy_package(
    conn: &Connection,
    project_id: &str,
) -> Result<ResearchStrategyPackage, String> {
    if project_id.trim().is_empty() {
        return Err("project_id is required".to_string());
    }

    let entries = research_shortlist::list_entries(conn, project_id, None)
        .map_err(|e| e.to_string())?;

    // Live recompute strategy tags so package JSON reflects current project.md
    // even if re-annotate was skipped (e.g. pure package callers). Empty strategy
    // leaves stored columns (or None) — never fabricates blocks.
    let live_strategy = crate::strategy::load_for_project(conn, project_id);
    let health_counts = count_shortlist_health(&entries);
    let shortlist: Vec<ShortlistSummaryEntry> = entries
        .iter()
        .map(|e| {
            let mut summary = ShortlistSummaryEntry::from(e);
            if !live_strategy.is_empty() {
                match crate::strategy::match_cluster(&live_strategy, &e.theme) {
                    Some((name, status)) => {
                        summary.strategy_cluster = Some(name.to_string());
                        summary.strategy_status = Some(status.as_str().to_string());
                    }
                    None => {
                        summary.strategy_cluster = None;
                        summary.strategy_status = None;
                    }
                }
            }
            summary
        })
        .collect();

    let open_research_task_ids = list_open_research_task_ids(conn, project_id)?;
    let content_strategy = load_content_strategy_summary(conn, project_id);
    let guidance = build_strategy_guidance(&content_strategy, &shortlist);

    Ok(ResearchStrategyPackage {
        project_id: project_id.to_string(),
        shortlist,
        health_counts,
        open_research_task_ids,
        guidance,
        content_strategy,
    })
}

/// Load structured content strategy for a project (graceful empty).
fn load_content_strategy_summary(conn: &Connection, project_id: &str) -> ContentStrategySummary {
    ContentStrategySummary::from(&crate::strategy::load_for_project(conn, project_id))
}

fn count_shortlist_health(entries: &[ResearchShortlistEntry]) -> ShortlistHealthCounts {
    let mut counts = ShortlistHealthCounts::default();
    for e in entries {
        match e.health_status.as_str() {
            "promising" => counts.promising += 1,
            "depleted" => counts.depleted += 1,
            // Default / unproven / unknown bucket as unproven.
            _ => counts.unproven += 1,
        }
        match e.status.as_str() {
            "pending" => counts.pending += 1,
            "researched" => counts.researched += 1,
            "covered" => counts.covered += 1,
            _ => {}
        }
    }
    counts
}

fn list_open_research_task_ids(conn: &Connection, project_id: &str) -> Result<Vec<String>, String> {
    let tasks = task_store::list_tasks_light(conn, project_id).map_err(|e| e.to_string())?;
    let open_statuses = [
        TaskStatus::Todo,
        TaskStatus::Queued,
        TaskStatus::InProgress,
        TaskStatus::Review,
    ];
    let research_types = [
        "research_keywords",
        "custom_keyword_research",
        "research_landing_pages",
    ];
    Ok(tasks
        .into_iter()
        .filter(|t| research_types.contains(&t.task_type.as_str()))
        .filter(|t| open_statuses.contains(&t.status))
        .map(|t| t.id)
        .collect())
}

// ─── Research pull ───────────────────────────────────────────────────────────

/// Options for [`research_pull`].
#[derive(Debug, Clone)]
pub struct ResearchPullOpts {
    /// Explicit seeds/themes (one line each in task.description after normalize).
    pub seeds: Vec<String>,
    pub title: Option<String>,
    /// When true, execute the spawned task immediately (CLI happy path).
    pub execute: bool,
    pub priority: Priority,
}

impl Default for ResearchPullOpts {
    fn default() -> Self {
        Self {
            seeds: Vec::new(),
            title: None,
            execute: true,
            priority: Priority::Medium,
        }
    }
}

/// Result of create (+ optional execute) for session-owned seed research.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchPullResult {
    pub task_id: String,
    pub task_type: String,
    pub status: String,
    pub seeds: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selectable_keywords: Option<Vec<String>>,
    pub executed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execute_success: Option<bool>,
    pub message: String,
    /// Aggregate selection funnel when available from the final-selection artifact (#263).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_funnel: Option<FilterFunnel>,
    /// Per-candidate strategy hard-drop telemetry from selection artifact (#260).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy_rejected_items: Option<Vec<crate::strategy::StrategyRejection>>,
    /// Candidates surviving the strategy hard gate (before top-N / post-filters).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy_kept: Option<usize>,
}

/// Normalize seeds: trim, drop empty, dedupe case-insensitively (first spelling wins).
pub fn normalize_seeds(seeds: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for s in seeds {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.to_lowercase();
        if seen.insert(key) {
            out.push(trimmed.to_string());
        }
    }
    out
}

/// Create a `custom_keyword_research` task from explicit seeds; optionally execute.
///
/// Pipeline contract: seeds are written one-per-line in `task.description`
/// (see `research_pipeline` fallback when seed-extraction artifact is missing).
/// No nested theme LLM.
pub async fn research_pull(
    conn: &Connection,
    project_id: &str,
    opts: ResearchPullOpts,
) -> Result<ResearchPullResult, String> {
    if project_id.trim().is_empty() {
        return Err("project_id is required".to_string());
    }

    let seeds = normalize_seeds(&opts.seeds);
    if seeds.is_empty() {
        return Err(
            "At least one non-empty seed is required (-K seed1,seed2,...)".to_string(),
        );
    }

    let title = opts.title.unwrap_or_else(|| {
        format!(
            "Research pull: {} seed{}",
            seeds.len(),
            if seeds.len() == 1 { "" } else { "s" }
        )
    });

    let description = seeds.join("\n");
    let idempotency_key = research_pull_idempotency_key(project_id, &seeds);

    let task = TaskSpawner::spawn(
        conn,
        TaskSpec {
            project_id: project_id.to_string(),
            task_type: "custom_keyword_research".to_string(),
            title: Some(title),
            description: Some(description),
            priority: opts.priority,
            agent_policy: AgentPolicy::None,
            idempotency_key: Some(idempotency_key),
            // Same seeds same calendar day: reuse active task; allow re-pull next day
            // or after done/failed/cancelled (SkipIfActive).
            dedup_policy: Some(DeduplicationPolicy::SkipIfActive),
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;

    if !opts.execute {
        let telemetry = extract_strategy_telemetry(&task);
        return Ok(ResearchPullResult {
            task_id: task.id.clone(),
            task_type: task.task_type.clone(),
            status: task.status.as_str().to_string(),
            seeds,
            selectable_keywords: extract_selectable_if_any(&task),
            executed: false,
            execute_success: None,
            message: "Created custom_keyword_research task (not executed). Run execute-task or research-pull without --no-execute.".to_string(),
            filter_funnel: extract_filter_funnel(&task),
            strategy_rejected_items: telemetry.rejected_items,
            strategy_kept: telemetry.kept,
        });
    }

    // Skip re-execute if already in review/done with selection artifact.
    if matches!(task.status, TaskStatus::Review | TaskStatus::Done) {
        let kws = extract_selectable_if_any(&task);
        if kws.as_ref().map(|k| !k.is_empty()).unwrap_or(false) {
            let funnel = extract_filter_funnel(&task);
            let telemetry = extract_strategy_telemetry(&task);
            let message = thin_result_message(
                "Reused existing research task already in review/done with selectable keywords.",
                kws.as_ref().map(|k| k.len()).unwrap_or(0),
                funnel.as_ref(),
            );
            return Ok(ResearchPullResult {
                task_id: task.id.clone(),
                task_type: task.task_type.clone(),
                status: task.status.as_str().to_string(),
                seeds,
                selectable_keywords: kws,
                executed: false,
                execute_success: Some(true),
                message,
                filter_funnel: funnel,
                strategy_rejected_items: telemetry.rejected_items,
                strategy_kept: telemetry.kept,
            });
        }
    }

    let exec = crate::engine::executor::execute_task_with_token(
        conn,
        &task.id,
        None,
        &crate::engine::executor::ExecuteOpts::default(),
    )
    .await
    .map_err(|e| e.to_string())?;

    let fresh = task_store::get_task(conn, &task.id).map_err(|e| e.to_string())?;
    let selectable = extract_selectable_if_any(&fresh);
    let funnel = extract_filter_funnel(&fresh);
    let telemetry = extract_strategy_telemetry(&fresh);
    let selectable_count = selectable.as_ref().map(|k| k.len()).unwrap_or(0);

    let message = if exec.success {
        let base = format!(
            "Research pull completed (status={}). {} selectable keyword(s). Use select-keywords -I {} -K ...",
            fresh.status.as_str(),
            selectable_count,
            fresh.id
        );
        thin_result_message(&base, selectable_count, funnel.as_ref())
    } else {
        // Empty / failed path: surface funnel summary when present on the error message.
        let base = format!("Research pull execution failed: {}", exec.message);
        match funnel.as_ref() {
            Some(f) if !exec.message.contains("filter_funnel:") => {
                format!("{} ({})", base, f.summary_line())
            }
            _ => base,
        }
    };

    Ok(ResearchPullResult {
        task_id: fresh.id,
        task_type: fresh.task_type,
        status: fresh.status.as_str().to_string(),
        seeds,
        selectable_keywords: selectable,
        executed: true,
        execute_success: Some(exec.success),
        message,
        filter_funnel: funnel,
        strategy_rejected_items: telemetry.rejected_items,
        strategy_kept: telemetry.kept,
    })
}

fn extract_selectable_if_any(task: &Task) -> Option<Vec<String>> {
    let kws = extract_selectable_keywords(task);
    if kws.is_empty() {
        None
    } else {
        Some(kws)
    }
}

/// Extract `filter_funnel` from the final-selection artifact when present.
/// Reuses the canonical selection-artifact helpers (fence-stripping + key chain).
fn extract_filter_funnel(task: &Task) -> Option<FilterFunnel> {
    let v = parse_artifact_json(task, find_research_selection_artifact(task))?;
    serde_json::from_value(v.get("filter_funnel")?.clone()).ok()
}

/// Strategy gate telemetry from the final-selection artifact (#260).
struct StrategyTelemetry {
    rejected_items: Option<Vec<crate::strategy::StrategyRejection>>,
    kept: Option<usize>,
}

fn extract_strategy_telemetry(task: &Task) -> StrategyTelemetry {
    let Some(v) = parse_artifact_json(task, find_research_selection_artifact(task)) else {
        return StrategyTelemetry {
            rejected_items: None,
            kept: None,
        };
    };
    let rejected_items = v
        .get("strategy_rejected_items")
        .and_then(|items| serde_json::from_value(items.clone()).ok())
        .and_then(|items: Vec<crate::strategy::StrategyRejection>| {
            if items.is_empty() {
                None
            } else {
                Some(items)
            }
        });
    let kept = v
        .get("strategy_kept")
        .and_then(|k| k.as_u64())
        .map(|n| n as usize)
        .filter(|n| *n > 0);
    StrategyTelemetry {
        rejected_items,
        kept,
    }
}

/// When the selectable pool is thin but non-empty, append a short funnel note
/// so operators see stage dropoff without digging into the full artifact.
fn thin_result_message(base: &str, selectable_count: usize, funnel: Option<&FilterFunnel>) -> String {
    const THIN_THRESHOLD: usize = 3;
    match funnel {
        Some(f) if selectable_count > 0 && selectable_count <= THIN_THRESHOLD => {
            format!("{} ({})", base, f.summary_line())
        }
        _ => base.to_string(),
    }
}

/// Deterministic key: project + calendar day + hash of sorted normalized seeds.
fn research_pull_idempotency_key(project_id: &str, seeds: &[String]) -> String {
    let day = chrono::Utc::now().format("%Y-%m-%d");
    let mut sorted: Vec<String> = seeds.iter().map(|s| s.to_lowercase()).collect();
    sorted.sort();
    let mut hasher = Sha256::new();
    hasher.update(sorted.join("\n").as_bytes());
    let digest = hasher.finalize();
    let hash_hex = digest
        .iter()
        .take(8)
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    format!("research_pull:{project_id}:{day}:{hash_hex}")
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::workflows::handlers::default_handlers;
    use crate::models::task::{
        FollowUpPolicy, TaskArtifact, TaskReviewSurface, TaskRun, TaskRunPolicy,
    };

    fn shortlist_table_sql() -> &'static str {
        "CREATE TABLE research_shortlist (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id TEXT NOT NULL,
            theme TEXT NOT NULL,
            seeds TEXT NOT NULL DEFAULT '[]',
            source TEXT NOT NULL,
            status TEXT NOT NULL,
            priority TEXT NOT NULL,
            article_count INTEGER,
            total_impressions REAL,
            signal_score REAL,
            health_status TEXT NOT NULL,
            strategy_cluster TEXT,
            strategy_status TEXT,
            last_reviewed_at TEXT,
            added_at TEXT NOT NULL,
            researched_at TEXT,
            covered_at TEXT
        );"
    }

    fn tasks_table_sql() -> &'static str {
        "CREATE TABLE tasks (
            id TEXT PRIMARY KEY,
            type TEXT NOT NULL,
            phase TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'todo',
            priority TEXT NOT NULL DEFAULT 'medium',
            run_policy TEXT NOT NULL DEFAULT 'user_enqueue',
            review_surface TEXT NOT NULL DEFAULT 'none',
            follow_up_policy TEXT NOT NULL DEFAULT 'none',
            agent_policy TEXT NOT NULL DEFAULT 'none',
            title TEXT,
            description TEXT,
            project_id TEXT NOT NULL,
            depends_on TEXT NOT NULL DEFAULT '[]',
            artifacts TEXT NOT NULL DEFAULT '[]',
            run_attempts INTEGER DEFAULT 0,
            run_last_error TEXT,
            run_provider TEXT,
            not_before TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE TABLE task_idempotency_keys (
            key TEXT PRIMARY KEY,
            task_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT
        );
        CREATE TABLE task_runs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id TEXT NOT NULL,
            attempt INTEGER NOT NULL,
            provider TEXT,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            success INTEGER,
            error TEXT,
            prompt_tokens INTEGER,
            completion_tokens INTEGER
        );"
    }

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!(
            "{}
             {}",
            shortlist_table_sql(),
            tasks_table_sql()
        ))
        .unwrap();
        conn
    }

    fn insert_shortlist(
        conn: &Connection,
        project_id: &str,
        theme: &str,
        status: &str,
        health: &str,
        seeds: &[&str],
    ) {
        let seeds_json = serde_json::to_string(seeds).unwrap();
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, health_status, added_at)
             VALUES (?1, ?2, ?3, 'test', ?4, 'medium', ?5, ?6)",
            rusqlite::params![
                project_id,
                theme,
                seeds_json,
                status,
                health,
                chrono::Utc::now().to_rfc3339()
            ],
        )
        .unwrap();
    }

    #[test]
    fn normalize_seeds_trims_dedupes_and_drops_empty() {
        let raw = vec![
            "  Delta Hedging  ".to_string(),
            "".to_string(),
            "delta hedging".to_string(),
            "Theta Decay".to_string(),
            "   ".to_string(),
            "theta decay".to_string(),
        ];
        let got = normalize_seeds(&raw);
        assert_eq!(got, vec!["Delta Hedging".to_string(), "Theta Decay".to_string()]);
    }

    #[test]
    fn normalize_seeds_empty_input() {
        assert!(normalize_seeds(&[]).is_empty());
        assert!(normalize_seeds(&["".into(), "  ".into()]).is_empty());
    }

    #[test]
    fn strategy_package_builds_from_shortlist_rows() {
        let conn = in_memory_db();
        insert_shortlist(&conn, "proj1", "delta hedging", "pending", "promising", &["delta hedge"]);
        insert_shortlist(&conn, "proj1", "theta decay", "pending", "unproven", &[]);
        insert_shortlist(&conn, "proj1", "old theme", "covered", "depleted", &[]);
        insert_shortlist(&conn, "proj1", "done research", "researched", "unproven", &[]);
        insert_shortlist(&conn, "other", "ignore", "pending", "promising", &[]);

        let pkg = build_research_strategy_package(&conn, "proj1").unwrap();
        assert_eq!(pkg.project_id, "proj1");
        assert_eq!(pkg.shortlist.len(), 4);
        assert_eq!(pkg.health_counts.promising, 1);
        assert_eq!(pkg.health_counts.unproven, 2);
        assert_eq!(pkg.health_counts.depleted, 1);
        assert_eq!(pkg.health_counts.pending, 2);
        assert_eq!(pkg.health_counts.researched, 1);
        assert_eq!(pkg.health_counts.covered, 1);
        assert!(!pkg.guidance.is_empty());
        assert!(pkg.open_research_task_ids.is_empty());
        // No project.md → empty strategy → strategy fields absent (serde skip).
        assert!(pkg.shortlist.iter().all(|e| e.strategy_cluster.is_none()));
        assert!(pkg.shortlist.iter().all(|e| e.strategy_status.is_none()));
        // #276: empty strategy is loud in JSON + guidance leads with recovery.
        assert_eq!(pkg.content_strategy.status, StrategyLoadStatus::Empty);
        assert!(
            pkg.guidance[0].contains("content_strategy.status is empty")
                && pkg.guidance[0].contains("no-ops"),
            "guidance[0] must lead with empty-status recovery; got {:?}",
            pkg.guidance[0]
        );
        // Empty strategy → no dynamic adjacent-shortlist line (#275).
        assert!(
            !pkg.guidance
                .iter()
                .any(|g| g.contains(STRATEGY_ADJACENT_SHORTLIST_GUIDANCE)
                    || g.contains("adjacent-only")),
            "dynamic adjacent guidance must not appear when strategy is empty"
        );
    }

    #[test]
    fn strategy_package_status_ok_fixture_no_recovery_first() {
        let (conn, dir) = strategy_fixture_db(
            r#"# Example Project

## Search Keywords

### Primary Keywords
- seo tools
- keyword research

### Problem Keywords
- thin content

### Audience Keywords
- content marketers

### Legacy Service Keywords (do not expand)
- custom web design
- wordpress agency

## Content Clusters And Priorities

### Cluster 1: SEO Fundamentals (ACTIVE)
- on-page seo
- technical seo

### Cluster 2: Alternatives (MAINTAIN)
- competitor alternatives

### Cluster 3: Services (LEGACY)
- web design packages

### Cluster 4: New Pillar (PLANNED)
- ai content ops
"#,
        );

        let pkg = build_research_strategy_package(&conn, "proj1").unwrap();
        assert_eq!(pkg.content_strategy.status, StrategyLoadStatus::Ok);
        assert_eq!(pkg.content_strategy.primary_keywords.len(), 2);
        assert_eq!(pkg.content_strategy.active_clusters.len(), 1);
        assert_eq!(pkg.content_strategy.do_not_expand.len(), 2);
        // Recovery line must not be first (or present as status recovery for ok).
        assert!(
            !pkg.guidance[0].contains("content_strategy.status is empty")
                && !pkg.guidance[0].contains("content_strategy.status is partial"),
            "ok strategy must not lead with status recovery; guidance[0]={:?}",
            pkg.guidance[0]
        );
        assert!(
            !pkg.guidance
                .iter()
                .any(|g| g.starts_with("content_strategy.status is empty")
                    || g.starts_with("content_strategy.status is partial")),
            "ok strategy must not include empty/partial recovery guidance"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strategy_package_status_partial_recovery_first() {
        // Problem keywords + Unknown cluster only → partial (no gate fuel).
        let (conn, dir) = strategy_fixture_db(
            r#"
## Search Keywords
### Problem Keywords
- thin content
## Content Clusters
### Cluster 1: Some Topic
- foo
"#,
        );

        let pkg = build_research_strategy_package(&conn, "proj1").unwrap();
        assert_eq!(pkg.content_strategy.status, StrategyLoadStatus::Partial);
        assert!(!pkg.content_strategy.problem_keywords.is_empty());
        assert!(pkg.content_strategy.primary_keywords.is_empty());
        assert!(pkg.content_strategy.active_clusters.is_empty());
        assert!(
            pkg.guidance[0].contains("content_strategy.status is partial")
                && pkg.guidance[0].contains("no-ops"),
            "guidance[0] must lead with partial-status recovery; got {:?}",
            pkg.guidance[0]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strategy_guidance_primary_active_before_shortlist_health() {
        // Static order (#275): Primary/ACTIVE seed rule appears before shortlist health.
        let primary_idx = STRATEGY_GUIDANCE
            .iter()
            .position(|g| g.contains("primary_keywords") && g.contains("ACTIVE"))
            .expect("Primary/ACTIVE seed guidance");
        let shortlist_idx = STRATEGY_GUIDANCE
            .iter()
            .position(|g| g.contains("Shortlist health") || g.contains("health_status=promising"))
            .expect("shortlist health guidance");
        assert!(
            primary_idx < shortlist_idx,
            "Primary/ACTIVE guidance (idx {primary_idx}) must precede shortlist health (idx {shortlist_idx})"
        );
        // Desk/shortlist must not be equal-priority seed peers when strategy present.
        assert!(
            STRATEGY_GUIDANCE
                .iter()
                .any(|g| g.contains("not equal-priority new-article seed")),
            "desk/shortlist equal-peer seed wording should be demoted"
        );
    }

    #[test]
    fn strategy_guidance_dynamic_line_when_pending_adjacent_only() {
        // Strategy has Primary/ACTIVE but pending shortlist has no active-tagged rows.
        let (conn, dir) = strategy_fixture_db(
            r#"# Test

## Search Keywords

### Primary Keywords
- keyword research

## Content Clusters

### Cluster 1: SEO Fundamentals (ACTIVE)
- technical seo

### Cluster 2: Old Services (LEGACY)
- web design packages
"#,
        );
        // Pending territory-style head (matches LEGACY only — not active).
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, health_status, added_at)
             VALUES ('proj1', 'web design packages', '[]', 'territory_analysis', 'pending', 'high',
                     'promising', ?1)",
            rusqlite::params![chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();

        let pkg = build_research_strategy_package(&conn, "proj1").unwrap();
        assert!(
            strategy_has_primary_or_active(&pkg.content_strategy),
            "fixture must load Primary/ACTIVE"
        );
        assert!(
            !pending_shortlist_has_active(&pkg.shortlist),
            "pending rows should not be strategy_status=active"
        );
        assert!(
            pkg.guidance
                .iter()
                .any(|g| g.contains(STRATEGY_ADJACENT_SHORTLIST_GUIDANCE)
                    || g.contains("adjacent-only or empty")),
            "dynamic adjacent guidance missing; guidance={:?}",
            pkg.guidance
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strategy_guidance_dynamic_line_when_pending_empty() {
        let (conn, dir) = strategy_fixture_db(
            r#"# Test

## Search Keywords

### Primary Keywords
- seo tools

## Content Clusters

### Cluster 1: SEO Fundamentals (ACTIVE)
- technical seo
"#,
        );
        // No shortlist rows at all.
        let pkg = build_research_strategy_package(&conn, "proj1").unwrap();
        assert!(pkg.shortlist.is_empty());
        assert!(
            pkg.guidance
                .iter()
                .any(|g| g.contains("adjacent-only or empty")),
            "empty pending shortlist with strategy should push dynamic line; guidance={:?}",
            pkg.guidance
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strategy_guidance_no_dynamic_line_when_pending_has_active() {
        let (conn, dir) = strategy_fixture_db(
            r#"# Test

## Search Keywords

### Primary Keywords
- keyword research

## Content Clusters

### Cluster 1: SEO Fundamentals (ACTIVE)
- technical seo
"#,
        );
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, health_status, added_at)
             VALUES ('proj1', 'technical seo', '[]', 'strategy_inject', 'pending', 'high',
                     'promising', ?1)",
            rusqlite::params![chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();

        let pkg = build_research_strategy_package(&conn, "proj1").unwrap();
        assert_eq!(
            pkg.shortlist[0].strategy_status.as_deref(),
            Some("active")
        );
        assert!(
            !pkg.guidance
                .iter()
                .any(|g| g.contains("adjacent-only or empty")),
            "dynamic line must not appear when pending has active-tagged row; guidance={:?}",
            pkg.guidance
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Full schema so strategy can resolve project path + project.md.
    fn strategy_fixture_db(project_md: &str) -> (Connection, std::path::PathBuf) {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-research-pkg-strategy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let automation = dir.join(".github").join("automation");
        std::fs::create_dir_all(&automation).unwrap();
        std::fs::write(automation.join("project.md"), project_md).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('proj1', 'Test', ?1, 1, 'workspace')",
            rusqlite::params![dir.to_string_lossy()],
        )
        .unwrap();
        (conn, dir)
    }

    #[test]
    fn strategy_package_surfaces_strategy_cluster_and_status() {
        let (conn, dir) = strategy_fixture_db(
            r#"# Test

## Content Clusters

### Cluster 1: SEO Fundamentals (ACTIVE)
- technical seo

### Cluster 2: Old Services (LEGACY)
- web design packages
"#,
        );
        // Seed DB with stale/wrong annotation; package must live-recompute.
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, health_status,
              strategy_cluster, strategy_status, added_at)
             VALUES ('proj1', 'technical seo', '[]', 'test', 'pending', 'high', 'promising',
                     'Stale Cluster', 'legacy', ?1)",
            rusqlite::params![chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, health_status, added_at)
             VALUES ('proj1', 'web design packages', '[]', 'test', 'pending', 'medium', 'unproven', ?1)",
            rusqlite::params![chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();

        let pkg = build_research_strategy_package(&conn, "proj1").unwrap();
        let by = |t: &str| pkg.shortlist.iter().find(|e| e.theme == t).unwrap();

        let active = by("technical seo");
        assert_eq!(active.strategy_cluster.as_deref(), Some("SEO Fundamentals"));
        assert_eq!(active.strategy_status.as_deref(), Some("active"));

        let legacy = by("web design packages");
        assert_eq!(legacy.strategy_cluster.as_deref(), Some("Old Services"));
        assert_eq!(legacy.strategy_status.as_deref(), Some("legacy"));

        // Serde includes strategy fields when present.
        let value = serde_json::to_value(&pkg).unwrap();
        let rows = value["shortlist"].as_array().unwrap();
        let clusters: Vec<&str> = rows
            .iter()
            .filter_map(|r| r["strategy_cluster"].as_str())
            .collect();
        assert!(clusters.contains(&"SEO Fundamentals"));
        assert!(clusters.contains(&"Old Services"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn research_context_reannotates_on_fresh_shortlist() {
        // Even when territory TTL skips, build_research_context re-annotates.
        let (conn, dir) = strategy_fixture_db(
            r#"# Test

## Content Clusters

### Cluster 1: SEO Fundamentals (ACTIVE)
- technical seo
"#,
        );
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, health_status,
              strategy_cluster, strategy_status, added_at)
             VALUES ('proj1', 'technical seo', '[]', 'territory_analysis', 'pending', 'high',
                     'promising', NULL, NULL, ?1)",
            rusqlite::params![now],
        )
        .unwrap();

        let envelope =
            build_research_context(&conn, "proj1", RESEARCH_SHORTLIST_MAX_AGE_DAYS).unwrap();
        assert!(!envelope.shortlist_refreshed);
        assert_eq!(
            envelope.shortlist_refresh_reason,
            shortlist_refresh_reason::SKIPPED_FRESH
        );
        let row = &envelope.strategy.shortlist[0];
        assert_eq!(row.strategy_cluster.as_deref(), Some("SEO Fundamentals"));
        assert_eq!(row.strategy_status.as_deref(), Some("active"));

        // DB columns also updated (re-annotate side effect).
        let db_rows = research_shortlist::list_entries(&conn, "proj1", None).unwrap();
        assert_eq!(
            db_rows[0].strategy_cluster.as_deref(),
            Some("SEO Fundamentals")
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn strategy_package_includes_open_research_task_ids() {
        let conn = in_memory_db();
        TaskSpawner::spawn(
            &conn,
            TaskSpec {
                project_id: "proj1".to_string(),
                task_type: "custom_keyword_research".to_string(),
                title: Some("open".into()),
                ..Default::default()
            },
        )
        .unwrap();
        TaskSpawner::spawn(
            &conn,
            TaskSpec {
                project_id: "proj1".to_string(),
                task_type: "research_keywords".to_string(),
                title: Some("done one".into()),
                ..Default::default()
            },
        )
        .unwrap();
        // Mark second as done so it is not open.
        let tasks = task_store::list_tasks(&conn, "proj1").unwrap();
        let done_id = tasks
            .iter()
            .find(|t| t.task_type == "research_keywords")
            .unwrap()
            .id
            .clone();
        task_store::update_task_status(&conn, &done_id, TaskStatus::Done).unwrap();

        let pkg = build_research_strategy_package(&conn, "proj1").unwrap();
        assert_eq!(pkg.open_research_task_ids.len(), 1);
        assert!(pkg.open_research_task_ids[0].starts_with("task-"));
    }

    #[tokio::test]
    async fn research_pull_rejects_empty_seeds() {
        let conn = in_memory_db();
        let err = research_pull(
            &conn,
            "proj1",
            ResearchPullOpts {
                seeds: vec!["  ".into(), "".into()],
                execute: false,
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
        assert!(err.contains("At least one"), "err={err}");
    }

    #[tokio::test]
    async fn research_pull_creates_custom_keyword_research_with_description_lines() {
        let conn = in_memory_db();
        let result = research_pull(
            &conn,
            "proj1",
            ResearchPullOpts {
                seeds: vec![
                    "delta hedging".into(),
                    "  theta decay  ".into(),
                    "Delta Hedging".into(), // dedupe
                ],
                title: Some("My pull".into()),
                execute: false,
                priority: Priority::High,
            },
        )
        .await
        .unwrap();

        assert_eq!(result.task_type, "custom_keyword_research");
        assert_eq!(result.status, "todo");
        assert_eq!(result.seeds, vec!["delta hedging", "theta decay"]);
        assert!(!result.executed);
        assert!(result.selectable_keywords.is_none());

        let task = task_store::get_task(&conn, &result.task_id).unwrap();
        assert_eq!(task.task_type, "custom_keyword_research");
        assert_eq!(task.title.as_deref(), Some("My pull"));
        assert_eq!(
            task.description.as_deref(),
            Some("delta hedging\ntheta decay")
        );
        assert_eq!(task.priority, Priority::High);
        // Lifecycle from task_definitions: KeywordPicker + UserSelection.
        assert_eq!(task.review_surface, TaskReviewSurface::KeywordPicker);
        assert_eq!(task.follow_up_policy, FollowUpPolicy::UserSelection);
    }

    #[tokio::test]
    async fn research_pull_idempotent_same_day_same_seeds() {
        let conn = in_memory_db();
        let opts = ResearchPullOpts {
            seeds: vec!["seed a".into(), "seed b".into()],
            execute: false,
            ..Default::default()
        };
        let a = research_pull(&conn, "proj1", opts.clone()).await.unwrap();
        let b = research_pull(&conn, "proj1", opts).await.unwrap();
        assert_eq!(a.task_id, b.task_id);
    }

    #[test]
    fn custom_keyword_research_plan_skips_seed_llm_steps() {
        let task = Task {
            id: "t1".into(),
            project_id: "proj1".into(),
            task_type: "custom_keyword_research".into(),
            phase: "research".into(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::KeywordPicker,
            follow_up_policy: FollowUpPolicy::UserSelection,
            agent_policy: AgentPolicy::None,
            title: None,
            description: Some("theme one\ntheme two".into()),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        };
        let handlers = default_handlers();
        let handler = handlers
            .iter()
            .find(|h| h.supports(&task))
            .expect("ResearchHandler");
        let steps = handler.plan(&task);
        let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "ensure_coverage_fresh",
                "research_ahrefs_pipeline",
                "research_final_selection"
            ]
        );
        assert!(!names.contains(&"research_seed_extraction"));
        assert!(!names.contains(&"research_seed_validation"));
        // Path B pull must never nest territory analysis (issue #192).
        assert!(!names.contains(&"research_territory_analysis"));
    }

    #[test]
    fn research_context_envelope_merges_refresh_fields_flat() {
        // Typed envelope must serialize the same flat JSON keys as the old CLI merge.
        let conn = in_memory_db();
        insert_shortlist(
            &conn,
            "proj1",
            "existing",
            "pending",
            "promising",
            &["seed"],
        );
        // Fresh territory row so ensure skips.
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE research_shortlist SET source = 'territory_analysis', added_at = ?1
             WHERE project_id = 'proj1'",
            rusqlite::params![now],
        )
        .unwrap();

        let envelope = build_research_context(
            &conn,
            "proj1",
            RESEARCH_SHORTLIST_MAX_AGE_DAYS,
        )
        .unwrap();
        assert!(!envelope.shortlist_refreshed);
        assert_eq!(
            envelope.shortlist_refresh_reason,
            shortlist_refresh_reason::SKIPPED_FRESH
        );
        assert!(envelope.territory.is_none());
        assert!(envelope.shortlist_refresh_error.is_none());
        assert_eq!(envelope.strategy.project_id, "proj1");
        assert_eq!(envelope.strategy.shortlist.len(), 1);

        let value = serde_json::to_value(&envelope).unwrap();
        let obj = value.as_object().unwrap();
        assert!(obj.contains_key("project_id"));
        assert!(obj.contains_key("shortlist"));
        assert!(obj.contains_key("health_counts"));
        assert!(obj.contains_key("open_research_task_ids"));
        assert!(obj.contains_key("guidance"));
        assert!(obj.contains_key("content_strategy"));
        assert_eq!(obj["shortlist_refreshed"], false);
        assert_eq!(obj["shortlist_refresh_reason"], "skipped_fresh");
        assert!(!obj.contains_key("territory"));
        assert!(!obj.contains_key("shortlist_refresh_error"));
        // Flatten: strategy is not nested under a "strategy" key.
        assert!(!obj.contains_key("strategy"));
    }

    #[test]
    fn research_context_maps_failed_refresh_to_shortlist_refresh_error() {
        let conn = in_memory_db();
        let envelope = build_research_context(&conn, "", RESEARCH_SHORTLIST_MAX_AGE_DAYS);
        // Empty project_id fails pure build after ensure fails.
        assert!(envelope.is_err());

        // Non-empty project with ensure-only failure path still builds strategy when
        // project_id is valid: use failed ensure via empty project_id is already covered
        // above. Verify field rename for a synthetic package.
        let synthetic = ResearchContextPackage {
            strategy: ResearchStrategyPackage {
                project_id: "p".into(),
                shortlist: vec![],
                health_counts: ShortlistHealthCounts::default(),
                open_research_task_ids: vec![],
                guidance: vec![],
                content_strategy: ContentStrategySummary::default(),
            },
            shortlist_refreshed: false,
            shortlist_refresh_reason: shortlist_refresh_reason::FAILED.to_string(),
            territory: None,
            shortlist_refresh_error: Some("boom".into()),
        };
        let value = serde_json::to_value(&synthetic).unwrap();
        assert_eq!(value["shortlist_refresh_error"], "boom");
        assert!(value.get("error").is_none());
    }

    #[test]
    fn selectable_keywords_available_after_pull_artifact() {
        // select-keywords path: final selection artifact on custom_keyword_research.
        let task = Task {
            id: "t1".into(),
            project_id: "proj1".into(),
            task_type: "custom_keyword_research".into(),
            phase: "research".into(),
            status: TaskStatus::Review,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::KeywordPicker,
            follow_up_policy: FollowUpPolicy::UserSelection,
            agent_policy: AgentPolicy::None,
            title: None,
            description: Some("delta hedging".into()),
            depends_on: vec![],
            artifacts: vec![TaskArtifact {
                key: "research_final_selection".into(),
                path: None,
                artifact_type: Some("json".into()),
                source: None,
                content: Some(
                    serde_json::json!({
                        "difficulty": {
                            "results": [
                                {
                                    "keyword": "delta hedge strategy",
                                    "difficulty": 25,
                                    "volume": "1,000-5,000",
                                    "intent": "informational",
                                    "winnability": "target"
                                }
                            ]
                        },
                        "filter_funnel": {
                            "pre_filter": 12,
                            "volume_dropped": 4,
                            "volume_unknown_kept": 2,
                            "no_data_or_kd_dropped": 3,
                            "intent_dropped": 1,
                            "strategy_rejected": 1,
                            "relevance_dropped": 2,
                            "winnability_avoid_dropped": 1,
                            "final_selected": 3
                        },
                        "strategy_rejected_items": [
                            {
                                "keyword": "custom web design agency",
                                "reason": "do_not_expand"
                            }
                        ],
                        "strategy_kept": 5
                    })
                    .to_string(),
                ),
            }],
            run: TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        };
        let kws = extract_selectable_keywords(&task);
        assert_eq!(kws, vec!["delta hedge strategy".to_string()]);
        let funnel = extract_filter_funnel(&task).expect("funnel from artifact");
        assert_eq!(funnel.pre_filter, 12);
        assert_eq!(funnel.volume_dropped, 4);
        assert_eq!(funnel.volume_unknown_kept, 2);
        assert_eq!(funnel.relevance_dropped, 2);
        assert_eq!(funnel.final_selected, 3);
        let telemetry = extract_strategy_telemetry(&task);
        let items = telemetry.rejected_items.expect("strategy items");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].keyword, "custom web design agency");
        assert_eq!(
            items[0].reason,
            crate::strategy::StrategyRejectReason::DoNotExpand
        );
        assert_eq!(telemetry.kept, Some(5));
    }

    #[test]
    fn research_pull_result_serializes_filter_funnel() {
        let result = ResearchPullResult {
            task_id: "t1".into(),
            task_type: "custom_keyword_research".into(),
            status: "review".into(),
            seeds: vec!["delta".into()],
            selectable_keywords: Some(vec!["kw a".into(), "kw b".into()]),
            executed: true,
            execute_success: Some(true),
            message: "thin".into(),
            filter_funnel: Some(FilterFunnel {
                pre_filter: 10,
                volume_dropped: 2,
                volume_unknown_kept: 1,
                no_data_or_kd_dropped: 3,
                intent_dropped: 1,
                strategy_rejected: 0,
                relevance_dropped: 0,
                winnability_avoid_dropped: 0,
                final_selected: 2,
            }),
            strategy_rejected_items: Some(vec![crate::strategy::StrategyRejection {
                keyword: "legacy phrase".into(),
                reason: crate::strategy::StrategyRejectReason::LegacyCluster,
            }]),
            strategy_kept: Some(4),
        };
        let v = serde_json::to_value(&result).unwrap();
        assert_eq!(v["filter_funnel"]["volume_unknown_kept"], 1);
        assert_eq!(v["filter_funnel"]["final_selected"], 2);
        assert_eq!(v["strategy_kept"], 4);
        assert_eq!(v["strategy_rejected_items"][0]["reason"], "legacy_cluster");
    }

    #[test]
    fn thin_result_message_appends_funnel_when_small_pool() {
        let f = FilterFunnel {
            pre_filter: 20,
            volume_dropped: 5,
            volume_unknown_kept: 2,
            no_data_or_kd_dropped: 8,
            intent_dropped: 2,
            strategy_rejected: 1,
            relevance_dropped: 0,
            winnability_avoid_dropped: 0,
            final_selected: 2,
        };
        let msg = thin_result_message("base", 2, Some(&f));
        assert!(msg.starts_with("base (filter_funnel:"));
        assert!(msg.contains("volume_dropped=5"));
        // Large pool: no funnel note.
        assert_eq!(thin_result_message("base", 10, Some(&f)), "base");
    }
}
