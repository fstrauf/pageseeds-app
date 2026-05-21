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

/// Read the full content audit report for a project.
/// Returns the raw JSON value so the frontend can extract whatever
/// checks it needs (temporal URLs, page bloat, literal variables, etc.).
#[tauri::command]
pub fn get_content_audit_report(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<serde_json::Value, String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // Primary: read from database
    if let Ok(Some(json)) = crate::db::content_audit::get_audit_report_as_json(&db, &project_id) {
        return Ok(json);
    }

    // Fallback: legacy JSON file during transition
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

/// Open a generated feature spec markdown file in the user's default editor.
/// Uses VS Code when available, otherwise falls back to the system default.
#[tauri::command]
pub fn open_feature_spec_in_vscode(
    state: State<'_, AppState>,
    task_id: String,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| e.to_string())?;

    // 1. Find the task to get its project and stored artifact path
    let task = task_store::get_task(&db, &task_id).map_err(|e| e.to_string())?;
    let project = task_store::get_project(&db, &task.project_id).map_err(|e| e.to_string())?;

    // 2. Try to find the exact path from the task artifact (stored by executor)
    let artifact_path = task.artifacts.iter().find_map(|a| {
        if a.key == "generate_feature_spec" || a.key == "feature_spec_path" {
            a.path.clone().or_else(|| a.content.clone())
        } else {
            None
        }
    });

    let repo_root = std::path::Path::new(&project.path);
    let path_to_open = if let Some(path_str) = artifact_path {
        let p = std::path::PathBuf::from(path_str);
        if p.exists() { p } else { repo_root.join(".github").join("automation").join("seo_feature_spec.md") }
    } else {
        // Fallback: deterministic unique filename
        let spec_path = repo_root
            .join(".github")
            .join("automation")
            .join(format!("seo_feature_spec_{}.md", task_id));
        if spec_path.exists() {
            spec_path
        } else {
            repo_root.join(".github").join("automation").join("seo_feature_spec.md")
        }
    };

    if !path_to_open.exists() {
        return Err(format!(
            "Feature spec not found at {}",
            path_to_open.display()
        ));
    }

    // 3. Open in VS Code, ensuring the repo window is focused first.
    //    Strategy: open the repo folder with -r (reuse / focus window), then open the file.
    //    This guarantees the file opens in the repo's VS Code: window, not the last active one.
    let path_str = path_to_open.to_string_lossy().to_string();
    let repo_str = repo_root.to_string_lossy().to_string();

    let result = if cfg!(target_os = "macos") {
        // On macOS the `code` CLI may not be in PATH for GUI apps.
        // Try common install locations before falling back to `open`.
        let code_candidates = [
            "/usr/local/bin/code",
            "/opt/homebrew/bin/code",
            "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
        ];
        let mut code_bin: Option<&str> = None;
        for candidate in &code_candidates {
            if std::path::Path::new(candidate).exists() {
                code_bin = Some(candidate);
                break;
            }
        }

        if let Some(code) = code_bin {
            // Step 1: focus / open the repo folder window
            let _ = std::process::Command::new(code)
                .args(["-r", &repo_str])
                .spawn();
            // Small delay to let VS Code: focus the window (non-blocking is fine)
            std::thread::sleep(std::time::Duration::from_millis(300));
            // Step 2: open the spec file in the focused window
            std::process::Command::new(code)
                .args(["-r", &path_str])
                .spawn()
        } else {
            // Fallback: open via bundle ID (cannot guarantee window selection)
            std::process::Command::new("open")
                .args(["-b", "com.microsoft.VSCode", &path_str])
                .spawn()
        }
    } else if cfg!(target_os = "windows") {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "code", "-r", &repo_str])
            .spawn();
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::process::Command::new("cmd")
            .args(["/c", "code", "-r", &path_str])
            .spawn()
    } else {
        let _ = std::process::Command::new("code")
            .args(["-r", &repo_str])
            .spawn();
        std::thread::sleep(std::time::Duration::from_millis(300));
        std::process::Command::new("code")
            .args(["-r", &path_str])
            .spawn()
    };

    match result {
        Ok(_) => Ok(()),
        Err(e) => Err(format!(
            "Failed to open VS Code ({}). Is the 'code' command installed? \
             In VS Code, run Cmd+Shift+P → 'Shell Command: Install code command in PATH'.",
            e
        )),
    }
}
