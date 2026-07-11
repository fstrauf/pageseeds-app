use tauri::State;

use crate::commands::AppState;
use crate::engine::task_store;

/// Type for investigation results.
#[derive(Debug, serde::Serialize)]
pub struct InvestigationResult {
    pub id: String,
    pub question: String,
    pub answer: String,
    pub summary: String,
    pub evidence: serde_json::Value,
    pub findings: Vec<serde_json::Value>,
    pub created_at: String,
}

/// Run an agentic investigation: the agent has access to data tools
/// and explores freely to answer the user's question.
#[tauri::command]
pub async fn investigate(
    state: State<'_, AppState>,
    project_id: String,
    question: String,
) -> Result<InvestigationResult, String> {
    let (project_path, agent_provider, db_path) = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        let project = task_store::get_project(&db, &project_id).map_err(|e| e.to_string())?;
        let project_path = project.path.clone();
        let agent_provider = crate::db::global_settings::get_agent_provider(&db);
        let db_path = crate::db::default_db_path();
        (project_path, agent_provider, db_path)
    };

    let db_path_str = db_path.to_string_lossy().to_string();

    let result = crate::engine::exec::investigate::exec_investigate(
        &project_id,
        &project_path,
        &db_path_str,
        &question,
        &agent_provider,
    )
    .await?;

    let id = format!("inv-{}", chrono::Utc::now().timestamp_millis());
    let created_at = chrono::Utc::now().to_rfc3339();

    let answer = result["answer"].as_str().unwrap_or("No answer produced.").to_string();
    let summary = result["summary"].as_str().unwrap_or("").to_string();
    let findings = result["findings"]
        .as_array()
        .cloned()
        .unwrap_or_default();

    // Save evidence to automation dir
    let paths = crate::engine::project_paths::ProjectPaths::from_path(&project_path);
    let inv_dir = paths.automation_dir.join("investigations").join(&id);
    if let Err(e) = std::fs::create_dir_all(&inv_dir) {
        log::warn!("[investigate] Failed to create investigation dir: {e}");
    } else {
        let evidence_path = inv_dir.join("evidence.json");
        if let Err(e) = std::fs::write(&evidence_path, serde_json::to_string_pretty(&result).unwrap_or_default()) {
            log::warn!("[investigate] Failed to write evidence: {e}");
        }
        let answer_path = inv_dir.join("answer.md");
        let md = format!(
            "# Investigation: {question}\n\n**Date:** {created_at}\n\n## Answer\n\n{answer}\n\n## Findings\n\n",
        );
        if let Err(e) = std::fs::write(&answer_path, md) {
            log::warn!("[investigate] Failed to write answer: {e}");
        }
    }

    Ok(InvestigationResult {
        id,
        question,
        answer,
        summary,
        evidence: result,
        findings,
        created_at,
    })
}
