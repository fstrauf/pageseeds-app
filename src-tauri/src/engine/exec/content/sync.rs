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

/// Sanitize content: rename `.md` → `.mdx`, repair article paths, then run format validation + fix.
pub(crate) fn exec_sanitize_content(task: &Task, project_path: &str) -> crate::engine::workflows::StepResult {
    use crate::content::cleaner::rename_md_to_mdx;
    use crate::content::article_resolver::repair_article_paths_in_batch;

    log::info!("[sanitize_content] starting for project={} path={}", task.project_id, project_path);

    let paths = ProjectPaths::from_path(project_path);

    // 1. Locate content directory
    let content_dir = match crate::content::locator::resolve(&paths.repo_root, None).selected {
        Some(dir) => dir,
        None => return crate::engine::workflows::StepResult {
            success: false,
            message: "Could not locate content directory for sanitize".to_string(),
            output: None,
        },
    };

    // 2. Fix malformed frontmatter closers BEFORE anything else.
    //    If --- is appended to the last field line, the frontmatter parser will
    //    fail or use the wrong boundary, and downstream validation will corrupt content.
    let structurally_fixed = match crate::content::cleaner::fix_malformed_frontmatter_closers(&content_dir) {
        Ok(v) => v,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Sanitize failed during structural fix: {}", e),
                output: None,
            };
        }
    };

    // 3. Rename .md → .mdx
    let renamed = match rename_md_to_mdx(&content_dir) {
        Ok(v) => v,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Sanitize failed during rename: {}", e),
            output: None,
        },
    };

    // 4. Repair articles.json / DB paths (in case stored paths were .md or got out of sync)
    let db_path = crate::db::default_db_path();
    let repaired = match rusqlite::Connection::open(&db_path) {
        Ok(conn) => {
            match repair_article_paths_in_batch(&paths.repo_root, &task.project_id, &conn) {
                Ok((count, _changes)) => count,
                Err(e) => {
                    log::warn!("[sanitize_content] Path repair failed: {}", e);
                    0
                }
            }
        }
        Err(e) => {
            log::warn!("[sanitize_content] Could not open DB for path repair: {}", e);
            0
        }
    };

    // 5. Run format validation + fix inline
    let schema = crate::engine::setup_check::load_workspace_config(&paths.automation_dir)
        .and_then(|cfg| cfg.frontmatter_schema);

    let validation = match crate::content::validator::validate_project(&paths.repo_root, &content_dir, schema.as_ref()) {
        Ok(v) => v,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: renamed.is_empty() && structurally_fixed.is_empty(),
                message: format!(
                    "Fixed {} closers, renamed {} .md → .mdx, repaired {} paths, but validation failed: {}",
                    structurally_fixed.len(), renamed.len(), repaired, e
                ),
                output: None,
            };
        }
    };

    let fix_result = match crate::content::validator::apply_fixes(&validation.issues, &paths.repo_root) {
        Ok(r) => r,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: renamed.is_empty() && structurally_fixed.is_empty(),
                message: format!(
                    "Fixed {} closers, renamed {} .md → .mdx, repaired {} paths, but fix failed: {}",
                    structurally_fixed.len(), renamed.len(), repaired, e
                ),
                output: None,
            };
        }
    };

    let renamed_names: Vec<String> = renamed
        .iter()
        .map(|(old, new)| {
            format!(
                "{} → {}",
                old.file_name().unwrap_or_default().to_string_lossy(),
                new.file_name().unwrap_or_default().to_string_lossy()
            )
        })
        .collect();

    let structurally_fixed_names: Vec<String> = structurally_fixed
        .iter()
        .map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string())
        .collect();

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Sanitized: {} closers fixed, {} .md → .mdx, {} paths repaired, {} files checked, {} fixed, {} issues remain",
            structurally_fixed.len(),
            renamed.len(),
            repaired,
            fix_result.files_checked,
            fix_result.files_fixed,
            fix_result.issues_remaining.len()
        ),
        output: Some(serde_json::to_string_pretty(&serde_json::json!({
            "structurally_fixed": structurally_fixed_names,
            "renamed": renamed_names,
            "paths_repaired": repaired,
            "files_checked": fix_result.files_checked,
            "files_fixed": fix_result.files_fixed,
            "issues_remaining": fix_result.issues_remaining.len(),
        })).unwrap_or_default()),
    }
}
