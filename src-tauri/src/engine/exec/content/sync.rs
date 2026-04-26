use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

/// Native Rust implementation of `pageseeds content sync-and-validate`.
pub(crate) fn exec_content_sync(task: &Task, project_path: &str) -> crate::engine::workflows::StepResult {
    use crate::content::ops::sync_and_validate;

    log::info!("[content_sync] starting for project={} path={}", task.project_id, project_path);

    let paths = ProjectPaths::from_path(project_path);
    match sync_and_validate(&paths.automation_dir, &paths.repo_root, false) {
        Ok(result) => {
            let output = serde_json::to_string_pretty(&result)
                .unwrap_or_else(|_| format!("{:?}", result));
            let ok = result.missing_files.is_empty() && result.malformed_file_refs.is_empty();
            crate::engine::workflows::StepResult {
                success: ok,
                message: format!("content_sync: {} — {}", if ok { "OK" } else { "issues found" }, result.next_action),
                output: Some(output),
            }
        }
        Err(e) => crate::engine::workflows::StepResult {
            success: false,
            message: format!("content_sync failed: {}", e),
            output: None,
        },
    }
}

pub(crate) fn exec_format_validation(task: &Task, project_path: &str) -> crate::engine::workflows::StepResult {
    use crate::content::validator::validate_project;
    use crate::engine::setup_check::load_workspace_config;

    log::info!("[format_validation] starting for project={} path={}", task.project_id, project_path);

    let paths = ProjectPaths::from_path(project_path);
    let schema = load_workspace_config(&paths.automation_dir).and_then(|cfg| cfg.frontmatter_schema);

    let content_dir = match crate::content::locator::resolve(&paths.repo_root, None).selected {
        Some(dir) => dir,
        None => return crate::engine::workflows::StepResult {
            success: false,
            message: "Could not locate content directory for format validation".to_string(),
            output: None,
        },
    };

    match validate_project(&paths.repo_root, &content_dir, schema.as_ref()) {
        Ok(result) => {
            let out_path = paths.automation_dir.join("format_issues.json");
            let out_str = serde_json::to_string_pretty(&result).unwrap_or_default() + "\n";
            let _ = std::fs::write(&out_path, out_str);

            let ok = result.error_count == 0 && result.warn_count == 0;
            crate::engine::workflows::StepResult {
                success: ok,
                message: format!(
                    "Format validation: {} files — {} errors, {} warnings, {} info",
                    result.files_checked, result.error_count, result.warn_count, result.info_count
                ),
                output: Some(serde_json::to_string_pretty(&serde_json::json!({
                    "files_checked": result.files_checked,
                    "error_count": result.error_count,
                    "warn_count": result.warn_count,
                    "info_count": result.info_count,
                    "auto_fixable_count": result.auto_fixable_count,
                    "output_path": out_path.display().to_string(),
                })).unwrap_or_default()),
            }
        }
        Err(e) => crate::engine::workflows::StepResult {
            success: false,
            message: format!("Format validation failed: {}", e),
            output: None,
        },
    }
}

pub(crate) fn exec_format_fix(task: &Task, project_path: &str) -> crate::engine::workflows::StepResult {
    use crate::content::validator::{validate_project, apply_fixes};
    use crate::engine::setup_check::load_workspace_config;

    log::info!("[format_fix] starting for project={} path={}", task.project_id, project_path);

    let paths = ProjectPaths::from_path(project_path);
    let schema = load_workspace_config(&paths.automation_dir).and_then(|cfg| cfg.frontmatter_schema);

    let content_dir = match crate::content::locator::resolve(&paths.repo_root, None).selected {
        Some(dir) => dir,
        None => return crate::engine::workflows::StepResult {
            success: false,
            message: "Could not locate content directory for format fix".to_string(),
            output: None,
        },
    };

    let validation = match validate_project(&paths.repo_root, &content_dir, schema.as_ref()) {
        Ok(v) => v,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Format fix failed during validation: {}", e),
            output: None,
        },
    };

    match apply_fixes(&validation.issues, &paths.repo_root) {
        Ok(result) => {
            let out_path = paths.automation_dir.join("format_fix_result.json");
            let out_str = serde_json::to_string_pretty(&result).unwrap_or_default() + "\n";
            let _ = std::fs::write(&out_path, out_str);

            crate::engine::workflows::StepResult {
                success: true,
                message: format!(
                    "Format fix: {} files checked, {} files fixed, {} issues remaining",
                    result.files_checked, result.files_fixed, result.issues_remaining.len()
                ),
                output: Some(serde_json::to_string_pretty(&serde_json::json!({
                    "files_checked": result.files_checked,
                    "files_fixed": result.files_fixed,
                    "issues_remaining": result.issues_remaining.len(),
                    "output_path": out_path.display().to_string(),
                })).unwrap_or_default()),
            }
        }
        Err(e) => crate::engine::workflows::StepResult {
            success: false,
            message: format!("Format fix failed: {}", e),
            output: None,
        },
    }
}
