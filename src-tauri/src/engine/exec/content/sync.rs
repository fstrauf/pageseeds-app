use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;
use rusqlite::Connection;

/// Native Rust implementation of `pageseeds content sync-and-validate`.
pub(crate) fn exec_content_sync(
    task: &Task,
    project_path: &str,
    conn: &Connection,
) -> crate::engine::workflows::StepResult {
    use crate::content::ops::sync_and_validate;

    log::info!(
        "[content_sync] starting for project={} path={}",
        task.project_id,
        project_path
    );

    let paths = ProjectPaths::from_path(project_path);
    match sync_and_validate(
        &paths.automation_dir,
        &paths.repo_root,
        false,
        conn,
        &task.project_id,
    ) {
        Ok(result) => {
            let output =
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| format!("{:?}", result));
            let ok = result.missing_files.is_empty() && result.malformed_file_refs.is_empty();
            crate::engine::workflows::StepResult {
                success: ok,
                message: format!(
                    "content_sync: {} — {}",
                    if ok { "OK" } else { "issues found" },
                    result.next_action
                ),
                output: Some(output),
            }
        }
        Err(e) => crate::engine::workflows::StepResult::fail(format!("content_sync failed: {}", e)),
    }
}

pub(crate) fn exec_format_validation(
    task: &Task,
    project_path: &str,
) -> crate::engine::workflows::StepResult {
    use crate::content::validator::validate_project;
    use crate::engine::setup_check::load_workspace_config;

    log::info!(
        "[format_validation] starting for project={} path={}",
        task.project_id,
        project_path
    );

    let paths = ProjectPaths::from_path(project_path);
    let schema =
        load_workspace_config(&paths.automation_dir).and_then(|cfg| cfg.frontmatter_schema);

    let content_dir = match crate::content::locator::resolve(&paths.repo_root, None).selected {
        Some(dir) => dir,
        None => {
            return crate::engine::workflows::StepResult::fail("Could not locate content directory for format validation".to_string())
        }
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
                output: Some(
                    serde_json::to_string_pretty(&serde_json::json!({
                        "files_checked": result.files_checked,
                        "error_count": result.error_count,
                        "warn_count": result.warn_count,
                        "info_count": result.info_count,
                        "auto_fixable_count": result.auto_fixable_count,
                        "output_path": out_path.display().to_string(),
                    }))
                    .unwrap_or_default(),
                ),
            }
        }
        Err(e) => crate::engine::workflows::StepResult::fail(format!("Format validation failed: {}", e)),
    }
}

pub(crate) fn exec_format_fix(
    task: &Task,
    project_path: &str,
) -> crate::engine::workflows::StepResult {
    use crate::content::validator::{apply_fixes, validate_project};
    use crate::engine::setup_check::load_workspace_config;

    log::info!(
        "[format_fix] starting for project={} path={}",
        task.project_id,
        project_path
    );

    let paths = ProjectPaths::from_path(project_path);
    let schema =
        load_workspace_config(&paths.automation_dir).and_then(|cfg| cfg.frontmatter_schema);

    let content_dir = match crate::content::locator::resolve(&paths.repo_root, None).selected {
        Some(dir) => dir,
        None => {
            return crate::engine::workflows::StepResult::fail("Could not locate content directory for format fix".to_string())
        }
    };

    let validation = match validate_project(&paths.repo_root, &content_dir, schema.as_ref()) {
        Ok(v) => v,
        Err(e) => {
            return crate::engine::workflows::StepResult::fail(format!("Format fix failed during validation: {}", e))
        }
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
                    result.files_checked,
                    result.files_fixed,
                    result.issues_remaining.len()
                ),
                output: Some(
                    serde_json::to_string_pretty(&serde_json::json!({
                        "files_checked": result.files_checked,
                        "files_fixed": result.files_fixed,
                        "issues_remaining": result.issues_remaining.len(),
                        "output_path": out_path.display().to_string(),
                    }))
                    .unwrap_or_default(),
                ),
            }
        }
        Err(e) => crate::engine::workflows::StepResult::fail(format!("Format fix failed: {}", e)),
    }
}

/// Sanitize content: rename `.md` → `.mdx`, repair article paths, then run read-only format validation.
pub(crate) fn exec_sanitize_content(
    task: &Task,
    project_path: &str,
) -> crate::engine::workflows::StepResult {
    use crate::content::article_resolver::repair_article_paths_in_batch;
    use crate::content::cleaner::rename_md_to_mdx;

    log::info!(
        "[sanitize_content] starting for project={} path={}",
        task.project_id,
        project_path
    );

    let paths = ProjectPaths::from_path(project_path);

    // 1. Locate content directory
    let content_dir = match crate::content::locator::resolve(&paths.repo_root, None).selected {
        Some(dir) => dir,
        None => {
            return crate::engine::workflows::StepResult::fail("Could not locate content directory for sanitize".to_string())
        }
    };

    // 2. Fix malformed frontmatter closers BEFORE anything else.
    //    If --- is appended to the last field line, the frontmatter parser will
    //    fail or use the wrong boundary, and downstream validation will corrupt content.
    let structurally_fixed =
        match crate::content::cleaner::fix_malformed_frontmatter_closers(&content_dir) {
            Ok(v) => v,
            Err(e) => {
                return crate::engine::workflows::StepResult::fail(format!("Sanitize failed during structural fix: {}", e));
            }
        };

    // 3. Rename .md → .mdx
    let renamed = match rename_md_to_mdx(&content_dir) {
        Ok(v) => v,
        Err(e) => {
            return crate::engine::workflows::StepResult::fail(format!("Sanitize failed during rename: {}", e))
        }
    };

    // 4. Repair articles.json / DB paths (in case stored paths were .md or got out of sync)
    //    then re-scan frontmatter and update the DB + JSON export.
    let db_path = crate::db::default_db_path();
    let mut repaired = 0usize;
    let mut metadata_updated = 0usize;
    if let Ok(conn) = rusqlite::Connection::open(&db_path) {
        match repair_article_paths_in_batch(&paths.repo_root, &task.project_id, &conn) {
            Ok(result) => repaired = result.repaired,
            Err(e) => log::warn!("[sanitize_content] Path repair failed: {}", e),
        }

        match crate::content::ops::sync_article_metadata_from_disk(
            &paths.repo_root,
            &task.project_id,
            &conn,
        ) {
            Ok(count) => metadata_updated = count,
            Err(e) => log::warn!("[sanitize_content] Metadata sync failed: {}", e),
        }

        if metadata_updated > 0 {
            if let Err(e) = crate::content::article_index::export_projection(
                &conn,
                &task.project_id,
                &paths.repo_root,
            ) {
                log::warn!(
                    "[sanitize_content] Failed to export articles.json after metadata sync: {}",
                    e
                );
            }
        }
    } else {
        log::warn!("[sanitize_content] Could not open DB for path repair / metadata sync");
    }

    // 5. Run format validation read-only (report only — do NOT auto-fix).
    //    Broad auto-fixes on frontmatter destroy structured YAML (FAQ lists,
    //    citations, comments). Only explicit format-fix step applies fixes.
    let schema = crate::engine::setup_check::load_workspace_config(&paths.automation_dir)
        .and_then(|cfg| cfg.frontmatter_schema);

    let validation = match crate::content::validator::validate_project(
        &paths.repo_root,
        &content_dir,
        schema.as_ref(),
    ) {
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
        .map(|p| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Sanitized: {} closers fixed, {} .md → .mdx, {} paths repaired, {} metadata synced, {} files checked, {} issues found (not auto-fixed)",
            structurally_fixed.len(),
            renamed.len(),
            repaired,
            metadata_updated,
            validation.files_checked,
            validation.issues.len()
        ),
        output: Some(serde_json::to_string_pretty(&serde_json::json!({
            "structurally_fixed": structurally_fixed_names,
            "renamed": renamed_names,
            "paths_repaired": repaired,
            "metadata_updated": metadata_updated,
            "files_checked": validation.files_checked,
            "errors": validation.error_count,
            "warnings": validation.warn_count,
            "info": validation.info_count,
            "issues": validation.issues,
        })).unwrap_or_default()),
    }
}
