//! CLI Path B research: session owns themes/seeds; deterministic pull + select.
//!
//! Mirrors [`write_package`]: domain logic lives here, CLI is thin flags + JSON.
//! Uses existing `custom_keyword_research` (no nested seed extraction/validation LLM).
//!
//! Flow:
//!   research-context → session proposes seeds → research-pull → select-keywords → write Path B
//!
//! No LLM calls live in this module.

use std::collections::HashSet;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::db::research_shortlist::{self, ResearchShortlistEntry};
use crate::engine::keyword_selection::extract_selectable_keywords;
use crate::engine::spawner::{DeduplicationPolicy, TaskSpec, TaskSpawner};
use crate::engine::task_store;
use crate::models::task::{AgentPolicy, Priority, Task, TaskStatus};

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
}

const STRATEGY_GUIDANCE: &[&str] = &[
    "Propose 2–6 seed themes from desk (site-overview/articles/GSC) + shortlist + brand context.",
    "Prefer shortlist health_status=promising and status=pending; avoid depleted themes.",
    "Pull candidates with research-pull -K \"seed1,seed2,...\" (deterministic custom_keyword_research; no nested theme LLM).",
    "After pull, select-keywords -I <task-id> -K kw1,kw2 (max 3), then write-context / write-submit Path B.",
    "Desktop research_keywords (nested seed extraction) remains available for UI; prefer research-pull on CLI weekly path.",
];

/// Build a strategy package from research_shortlist + open research tasks.
/// No LLM. No side effects.
pub fn build_research_strategy_package(
    conn: &Connection,
    project_id: &str,
) -> Result<ResearchStrategyPackage, String> {
    if project_id.trim().is_empty() {
        return Err("project_id is required".to_string());
    }

    let entries = research_shortlist::list_entries(conn, project_id, None)
        .map_err(|e| e.to_string())?;

    let health_counts = count_shortlist_health(&entries);
    let shortlist: Vec<ShortlistSummaryEntry> = entries.iter().map(ShortlistSummaryEntry::from).collect();

    let open_research_task_ids = list_open_research_task_ids(conn, project_id)?;

    Ok(ResearchStrategyPackage {
        project_id: project_id.to_string(),
        shortlist,
        health_counts,
        open_research_task_ids,
        guidance: STRATEGY_GUIDANCE.iter().map(|s| (*s).to_string()).collect(),
    })
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
        return Ok(ResearchPullResult {
            task_id: task.id.clone(),
            task_type: task.task_type.clone(),
            status: task.status.as_str().to_string(),
            seeds,
            selectable_keywords: extract_selectable_if_any(&task),
            executed: false,
            execute_success: None,
            message: "Created custom_keyword_research task (not executed). Run execute-task or research-pull without --no-execute.".to_string(),
        });
    }

    // Skip re-execute if already in review/done with selection artifact.
    if matches!(task.status, TaskStatus::Review | TaskStatus::Done) {
        let kws = extract_selectable_if_any(&task);
        if kws.as_ref().map(|k| !k.is_empty()).unwrap_or(false) {
            return Ok(ResearchPullResult {
                task_id: task.id.clone(),
                task_type: task.task_type.clone(),
                status: task.status.as_str().to_string(),
                seeds,
                selectable_keywords: kws,
                executed: false,
                execute_success: Some(true),
                message: "Reused existing research task already in review/done with selectable keywords.".to_string(),
            });
        }
    }

    let exec = crate::engine::executor::execute_task_with_token(conn, &task.id, None, None, false)
        .await
        .map_err(|e| e.to_string())?;

    let fresh = task_store::get_task(conn, &task.id).map_err(|e| e.to_string())?;
    let selectable = extract_selectable_if_any(&fresh);

    let message = if exec.success {
        format!(
            "Research pull completed (status={}). {} selectable keyword(s). Use select-keywords -I {} -K ...",
            fresh.status.as_str(),
            selectable.as_ref().map(|k| k.len()).unwrap_or(0),
            fresh.id
        )
    } else {
        format!("Research pull execution failed: {}", exec.message)
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
                        }
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
    }
}
