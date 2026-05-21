use tauri::State;

use crate::commands::AppState;
use crate::engine::spawner::{TaskSpec, TaskSpawner};
use crate::engine::task_store;
use crate::models::task::{Priority, Task};

/// Summary of indexing health for a project.
#[derive(Debug, serde::Serialize)]
pub struct IndexingHealthSummary {
    pub total_urls: usize,
    pub indexed: usize,
    pub not_indexed: usize,
    pub issues_by_reason: Vec<(String, usize)>,
    pub last_inspected_at: Option<String>,
}

/// Run a full health audit by creating the two manual tasks needed:
///   1. content_review (includes content_audit step)
///   2. indexing_health_campaign
///
/// ctr_audit and cannibalization_audit are auto-enqueued on schedule;
/// the dashboard shows their latest data automatically.
#[tauri::command]
pub fn run_health_audit(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<Task>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;

    let mut tasks = Vec::new();

    // Spawn content_review (includes content_audit with new checks)
    let content_task = TaskSpawner::spawn(
        &conn,
        TaskSpec {
            project_id: project_id.clone(),
            task_type: "content_review".to_string(),
            title: Some("Content Health Audit".to_string()),
            priority: Priority::Medium,
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    tasks.push(content_task);

    // Spawn indexing_health_campaign
    let indexing_task = TaskSpawner::spawn(
        &conn,
        TaskSpec {
            project_id: project_id.clone(),
            task_type: "indexing_health_campaign".to_string(),
            title: Some("Indexing Health Audit".to_string()),
            priority: Priority::Medium,
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    tasks.push(indexing_task);

    Ok(tasks)
}

/// Get a summary of indexing health from the SQLite gsc_url_indexing_status table.
#[tauri::command]
pub fn get_indexing_health_summary(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<IndexingHealthSummary, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;

    let statuses = crate::gsc::db::list_by_project(&conn, &project_id)
        .map_err(|e| e.to_string())?;

    let total = statuses.len();
    let indexed = statuses
        .iter()
        .filter(|s| s.last_reason_code.as_deref() == Some("indexed_pass"))
        .count();
    let not_indexed = total.saturating_sub(indexed);

    let mut reason_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for s in &statuses {
        if let Some(reason) = &s.last_reason_code {
            if reason != "indexed_pass" {
                *reason_counts.entry(reason.clone()).or_insert(0) += 1;
            }
        }
    }
    let mut issues_by_reason: Vec<(String, usize)> = reason_counts.into_iter().collect();
    issues_by_reason.sort_by(|a, b| b.1.cmp(&a.1));

    let last_inspected_at = statuses
        .iter()
        .filter_map(|s| s.last_inspected_at.as_ref())
        .max()
        .cloned();

    Ok(IndexingHealthSummary {
        total_urls: total,
        indexed,
        not_indexed,
        issues_by_reason,
        last_inspected_at,
    })
}

/// Read the full content_audit.json artifact for a project.
/// Returns the raw JSON value so the frontend can extract whatever
/// checks it needs (temporal URLs, page bloat, literal variables, etc.).
#[tauri::command]
pub fn get_content_audit_report(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<serde_json::Value, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
    let repo_root = std::path::Path::new(&project.path);
    let audit_path = repo_root
        .join(".github")
        .join("automation")
        .join("content_audit.json");

    if !audit_path.exists() {
        return Ok(serde_json::json!({
            "generated_at": null,
            "total_audited": 0,
            "health_summary": { "good": 0, "needs_improvement": 0, "poor": 0 },
            "articles": [],
        }));
    }

    let content = std::fs::read_to_string(&audit_path)
        .map_err(|e| format!("Failed to read content_audit.json: {}", e))?;
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Invalid JSON in content_audit.json: {}", e))?;

    Ok(value)
}
