use super::AppState;
use crate::clarity::client::{ClarityClient, ClarityClientConfig};
use crate::config::env_resolver::EnvResolver;
use crate::engine::project_paths::ProjectPaths;
use crate::models::clarity::{
    ClarityConnectionStatus, ClarityFindingPayload, ClaritySummaryPayload,
    ClarityTaskCreationResult,
};
use crate::models::project::Project;
use tauri::State;

#[tauri::command]
pub fn clarity_get_status(_state: State<'_, AppState>, project: Project) -> ClarityConnectionStatus {
    let resolver = EnvResolver::new(&project.path);
    let has_token = resolver.resolve("CLARITY_API_TOKEN").is_some();
    let project_id = project.clarity_project_id.as_deref().filter(|id| !id.is_empty());

    match (has_token, project_id) {
        (true, Some(id)) => ClarityConnectionStatus {
            connected: true,
            message: format!("Clarity project {} configured", id),
        },
        (true, None) => ClarityConnectionStatus {
            connected: false,
            message: "CLARITY_API_TOKEN set but clarity_project_id is missing".to_string(),
        },
        (false, Some(id)) => ClarityConnectionStatus {
            connected: false,
            message: format!(
                "clarity_project_id {} configured but CLARITY_API_TOKEN missing in secrets",
                id
            ),
        },
        (false, None) => ClarityConnectionStatus {
            connected: false,
            message: "Clarity not configured: add clarity_project_id to project settings and CLARITY_API_TOKEN to secrets".to_string(),
        },
    }
}

#[tauri::command]
pub async fn clarity_test_connection(project: Project) -> Result<ClarityConnectionStatus, String> {
    let resolver = EnvResolver::new(&project.path);
    let token = resolver
        .resolve("CLARITY_API_TOKEN")
        .map(|(v, _)| v)
        .ok_or("CLARITY_API_TOKEN not configured")?;
    let project_id = project
        .clarity_project_id
        .as_deref()
        .filter(|id| !id.is_empty())
        .ok_or("clarity_project_id not set in project settings")?;

    let client = ClarityClient::new(ClarityClientConfig::new(token, project_id.to_string()));

    match client.test_connection().await {
        Ok(_) => Ok(ClarityConnectionStatus {
            connected: true,
            message: "Successfully connected to Microsoft Clarity".to_string(),
        }),
        Err(e) => Ok(ClarityConnectionStatus {
            connected: false,
            message: e,
        }),
    }
}

#[tauri::command]
pub fn clarity_get_summary(_state: State<'_, AppState>, project: Project) -> Result<Option<ClaritySummaryPayload>, String> {
    let paths = ProjectPaths::from_path(&project.path);
    match crate::clarity::export::read_summary(&paths.automation_dir) {
        Ok(Some(summary)) => Ok(Some(ClaritySummaryPayload {
            project_id: summary.meta.project_id,
            generated_at: summary.meta.generated_at,
            days_analyzed: summary.meta.days_analyzed,
            page_scores: summary
                .page_scores
                .into_iter()
                .map(|p| crate::models::clarity::ClarityPageScorePayload {
                    url: p.url,
                    total_sessions: p.total_sessions,
                    rage_click_count: p.rage_click_count,
                    dead_click_count: p.dead_click_count,
                    quickback_count: p.quickback_count,
                    excessive_scroll_count: p.excessive_scroll_count,
                    error_click_count: p.error_click_count,
                    script_error_count: p.script_error_count,
                    avg_engagement_seconds: p.avg_engagement_seconds,
                    avg_scroll_depth: p.avg_scroll_depth,
                    rage_click_rate: p.rage_click_rate,
                    dead_click_rate: p.dead_click_rate,
                    quickback_rate: p.quickback_rate,
                    z_score: p.z_score,
                    clarity_dashboard_url: p.clarity_dashboard_url,
                })
                .collect(),
            top_findings: summary
                .top_findings
                .into_iter()
                .map(|f| crate::models::clarity::ClarityFindingPayload {
                    issue_type: f.issue_type,
                    severity: f.severity,
                    url: f.url,
                    evidence: f.evidence,
                    recommendation: f.recommendation,
                    clarity_dashboard_url: f.clarity_dashboard_url,
                })
                .collect(),
        })),
        Ok(None) => Ok(None),
        Err(e) => Err(format!("Failed to read Clarity summary: {}", e)),
    }
}

/// Create follow-up tasks from user-selected Clarity findings in the task drawer.
#[tauri::command]
pub fn create_clarity_tasks_from_selection(
    state: State<'_, AppState>,
    parent_task_id: String,
    findings: Vec<ClarityFindingPayload>,
) -> Result<ClarityTaskCreationResult, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;
    crate::clarity::follow_up::spawn_tasks_from_selection(&db, &parent_task_id, &findings)
        .map_err(|e| e.into())
}
