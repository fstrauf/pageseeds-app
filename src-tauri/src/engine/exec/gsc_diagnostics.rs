/// Stateful GSC indexing diagnostics.
///
/// Unlike `collect_gsc` (bulk, one-shot), this workflow:
///   1. Maintains per-URL inspection history in SQLite (`gsc_url_indexing_status`)
///   2. Only re-checks URLs that are stale or have known issues
///   3. Spawns fix tasks only for new, regressed, or unresolved issues
///   4. Tracks whether previous fixes worked
use rusqlite::Connection;
use std::collections::HashMap;

use crate::config::env_resolver::EnvResolver;
use crate::engine::project_paths::ProjectPaths;
use crate::engine::spawner::{TaskSpawner, TaskSpec};
use crate::engine::workflows::StepResult;
use crate::gsc::db::{self, UrlIndexingStatus};
use crate::models::task::{AgentPolicy, TaskRunPolicy, Priority, Task};

/// Number of days before a previously-passing URL is re-checked.
const PASS_RECHECK_DAYS: i64 = 14;

/// Run the indexing diagnostics workflow.
///
/// 1. Load sitemap URLs
/// 2. Filter to URLs needing inspection (never checked, stale pass, or known issue)
/// 3. Call GSC URL Inspection API
/// 4. Compare to historical SQLite state
/// 5. Spawn fix tasks for new/regressed/unresolved issues
/// 6. Save updated state back to SQLite
pub(crate) fn exec_indexing_diagnostics(
    task: &Task,
    project_path: &str,
    _gsc_token: Option<&str>,
    conn: &Connection,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let resolver = EnvResolver::new(project_path);

    // 1. manifest.json → site_url + sitemap_url
    let manifest_path = paths.automation_dir.join("manifest.json");
    let manifest: serde_json::Value =
        match crate::engine::exec::common::read_json(&manifest_path, "manifest.json") {
            Ok(v) => v,
            Err(e) => return e,
        };

    let site_url = match manifest
        .get("gsc_site")
        .or_else(|| manifest.get("url"))
        .and_then(|v| v.as_str())
        .map(String::from)
    {
        Some(u) => u,
        None => {
            return StepResult {
                success: false,
                message: "No 'url' or 'gsc_site' field in manifest.json".to_string(),
                output: None,
            }
        }
    };

    let sitemap_url = manifest
        .get("sitemap")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| {
            let base = if site_url.starts_with("sc-domain:") {
                format!("https://{}", &site_url["sc-domain:".len()..])
            } else if !site_url.starts_with("http://") && !site_url.starts_with("https://") {
                format!("https://{}", site_url)
            } else {
                site_url.clone()
            };
            format!("{}/sitemap.xml", base.trim_end_matches('/'))
        });

    // 2. Credentials
    let sa_path = match resolver
        .resolve("GSC_SERVICE_ACCOUNT_PATH")
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS"))
        .map(|(v, _)| v)
    {
        Some(p) => p,
        None => {
            return StepResult {
                success: false,
                message: "GSC_SERVICE_ACCOUNT_PATH not configured — add it in Settings → Secrets"
                    .to_string(),
                output: None,
            }
        }
    };

    // 3. Fetch sitemap + inspect URLs (async I/O in a blocking thread)
    let sa_path_owned = sa_path.clone();
    let sitemap_url_owned = sitemap_url.clone();
    let site_url_owned = site_url.clone();

    // 3a. Fetch sitemap URLs synchronously (lightweight HTTP, can stay in thread)
    let sitemap_result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async move {
            let urls = crate::gsc::sitemap::fetch_sitemap_urls(&sitemap_url_owned, 200).await?;
            Ok::<_, crate::error::Error>(urls)
        })
    })
    .join();

    let all_urls = match sitemap_result {
        Ok(Ok(u)) => u,
        Ok(Err(e)) => {
            return StepResult {
                success: false,
                message: format!("Failed to fetch sitemap: {}", e),
                output: None,
            }
        }
        Err(_) => {
            return StepResult {
                success: false,
                message: "Sitemap fetch thread panicked".to_string(),
                output: None,
            }
        }
    };

    if all_urls.is_empty() {
        return StepResult {
            success: false,
            message: format!("Sitemap at '{}' is empty or unreachable", sitemap_url),
            output: None,
        };
    }

    // 4. Load existing statuses and filter URLs to inspect
    let existing_statuses = match db::list_by_project(conn, &task.project_id) {
        Ok(rows) => rows,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to load indexing status from DB: {}", e),
                output: None,
            }
        }
    };
    let status_map: HashMap<String, UrlIndexingStatus> = existing_statuses
        .into_iter()
        .map(|s| (s.url.clone(), s))
        .collect();

    let now = chrono::Utc::now();
    let mut urls_to_inspect: Vec<String> = Vec::new();

    for url in &all_urls {
        let needs_inspection = match status_map.get(url) {
            None => true, // never checked
            Some(status) => {
                let is_stale = status
                    .last_inspected_at
                    .as_ref()
                    .map(|dt| {
                        chrono::DateTime::parse_from_rfc3339(dt)
                            .ok()
                            .map(|parsed| {
                                let days_since =
                                    (now - parsed.with_timezone(&chrono::Utc)).num_days();
                                days_since >= PASS_RECHECK_DAYS
                            })
                            .unwrap_or(true)
                    })
                    .unwrap_or(true);

                let has_issue = status
                    .last_reason_code
                    .as_deref()
                    .map(|r| r != "indexed_pass")
                    .unwrap_or(false);

                is_stale || has_issue
            }
        };

        if needs_inspection {
            urls_to_inspect.push(url.clone());
        }
    }

    log::info!(
        "[indexing_diagnostics] {}/{} URLs need inspection for project {}",
        urls_to_inspect.len(),
        all_urls.len(),
        task.project_id
    );

    // 5. Call URL Inspection API only for filtered URLs
    let inspect_result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async move {
            // Always mint fresh from service account when available
            let token = crate::gsc::auth::get_service_account_token(&sa_path_owned)
                .await
                .map(|t| t.access_token)?;

            let records =
                crate::gsc::indexing::inspect_batch(&token, &site_url_owned, urls_to_inspect)
                    .await?;
            Ok::<_, crate::error::Error>(records)
        })
    })
    .join();

    let records = match inspect_result {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            return StepResult {
                success: false,
                message: format!("GSC inspection failed: {}", e),
                output: None,
            }
        }
        Err(_) => {
            return StepResult {
                success: false,
                message: "GSC inspection thread panicked".to_string(),
                output: None,
            }
        }
    };

    log::info!(
        "[indexing_diagnostics] {} URLs inspected for project {}",
        records.len(),
        task.project_id
    );

    let now_iso = now.to_rfc3339();

    let mut inspected_count = 0usize;
    let mut new_issue_count = 0usize;
    let mut regressed_count = 0usize;
    let mut resolved_count = 0usize;
    let mut unchanged_issue_count = 0usize;
    let mut spawned_tasks: Vec<String> = vec![];

    for record in &records {
        let reason = record.reason_code.as_deref().unwrap_or("unknown");
        let action = record.action.as_deref().unwrap_or("");
        let verdict = record.verdict.as_deref().unwrap_or("");
        let is_pass = reason == "indexed_pass";

        let previous = status_map.get(&record.url);
        let prev_reason = previous
            .as_ref()
            .and_then(|s| s.last_reason_code.as_deref());
        let prev_pass = prev_reason == Some("indexed_pass");

        // Determine if this is a change worth noting
        let is_new_issue = !is_pass && (previous.is_none() || prev_pass);
        let is_regression =
            !is_pass && previous.is_some() && Some(reason) != prev_reason && !prev_pass;
        let is_resolved = is_pass && previous.is_some() && !prev_pass;
        let is_unchanged_issue = !is_pass && previous.is_some() && Some(reason) == prev_reason;

        if is_new_issue {
            new_issue_count += 1;
        } else if is_regression {
            regressed_count += 1;
        } else if is_resolved {
            resolved_count += 1;
        } else if is_unchanged_issue {
            unchanged_issue_count += 1;
        }

        // Spawn fix task only if:
        //   - URL has an issue (not pass)
        //   - AND there is no active fix task already for this URL
        //   - AND the URL maps to an actual MDX file we can edit
        if !is_pass {
            let has_mdx = has_mdx_for_url(project_path, &record.url);
            if !has_mdx {
                log::info!(
                    "[indexing_diagnostics] skipping {} — no MDX file found for this URL",
                    record.url
                );
            } else {
                // Check if this URL has been fixed before (repeat offender)
                if let Ok(Some(prev)) = db::get_status(conn, &record.url, &task.project_id) {
                    if prev.last_task_resolved_at.is_some() {
                        let attempt = prev.fix_attempt_count;
                        let last_fix = prev.last_task_resolved_at.as_deref().unwrap_or("");
                        let summary = prev.last_fix_summary.as_deref().unwrap_or("(no summary)");
                        log::warn!(
                            "[indexing_diagnostics] REPEAT OFFENDER: {} still has issue '{}' after {} fix attempt(s), last fix on {}: {}",
                            record.url, reason, attempt, last_fix, summary
                        );
                    }
                }

                match db::has_active_fix_task(conn, &task.project_id, &record.url, reason) {
                    Ok(false) => {
                        if let Some(task_id) = spawn_fix_task(
                            conn,
                            task,
                            &record.url,
                            reason,
                            action,
                            verdict,
                            record.priority,
                        ) {
                            spawned_tasks.push(task_id);
                        }
                    }
                    Ok(true) => {
                        log::info!(
                            "[indexing_diagnostics] active fix task already exists for {}",
                            record.url
                        );
                    }
                    Err(e) => {
                        log::warn!(
                            "[indexing_diagnostics] failed to check active tasks for {}: {}",
                            record.url,
                            e
                        );
                    }
                }
            }
        }

        // Update or insert status record
        let consecutive_passes = if is_pass {
            previous.as_ref().map(|s| s.consecutive_passes).unwrap_or(0) + 1
        } else {
            0
        };

        let last_task_id = previous.as_ref().and_then(|s| s.last_task_id.clone());
        let last_task_type = previous.as_ref().and_then(|s| s.last_task_type.clone());
        let last_task_created_at = previous
            .as_ref()
            .and_then(|s| s.last_task_created_at.clone());

        let last_fix_summary = previous.as_ref().and_then(|s| s.last_fix_summary.clone());
        let fix_attempt_count = previous.as_ref().map(|s| s.fix_attempt_count).unwrap_or(0);
        let last_task_resolved_at = previous
            .as_ref()
            .and_then(|s| s.last_task_resolved_at.clone());

        let status = UrlIndexingStatus {
            url: record.url.clone(),
            project_id: task.project_id.clone(),
            last_inspected_at: Some(now_iso.clone()),
            last_reason_code: Some(reason.to_string()),
            last_verdict: Some(verdict.to_string()),
            last_action: Some(action.to_string()),
            consecutive_passes,
            last_task_created_at,
            last_task_type,
            last_task_id,
            last_fix_summary,
            fix_attempt_count,
            last_task_resolved_at,
            created_at: previous
                .as_ref()
                .map(|s| s.created_at.clone())
                .unwrap_or_else(|| now_iso.clone()),
            updated_at: now_iso.clone(),
        };

        if let Err(e) = db::upsert_status(conn, &status) {
            log::warn!(
                "[indexing_diagnostics] failed to upsert status for {}: {}",
                record.url,
                e
            );
        }

        inspected_count += 1;
    }

    // Build summary output
    let summary = serde_json::json!({
        "inspected_count": inspected_count,
        "new_issues": new_issue_count,
        "regressed": regressed_count,
        "resolved": resolved_count,
        "unchanged_issues": unchanged_issue_count,
        "spawned_tasks": spawned_tasks.len(),
        "spawned_task_ids": spawned_tasks,
        "site_url": site_url,
        "sitemap_url": sitemap_url,
        "checked_at": now_iso,
    });

    StepResult {
        success: true,
        message: format!(
            "Diagnostics complete: {} URLs checked, {} new issues, {} regressed, {} resolved, {} tasks spawned",
            inspected_count, new_issue_count, regressed_count, resolved_count, spawned_tasks.len()
        ),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
    }
}

fn has_mdx_for_url(project_path: &str, url: &str) -> bool {
    let content_dir = crate::content::locator::resolve(std::path::Path::new(project_path), None)
        .selected
        .unwrap_or_else(|| std::path::PathBuf::from(project_path));

    let slug = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let slug = if let Some(pos) = slug.find('/') {
        &slug[pos + 1..]
    } else {
        slug
    };

    if slug.is_empty() {
        return false;
    }

    let last_segment = slug
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(slug);
    // Strip numeric prefix from URL last segment too (e.g. "127_net_worth_tracker" → "net_worth_tracker")
    let last_segment_clean = strip_numeric_prefix(last_segment).replace('_', "-");

    let full_slug_dashed = strip_numeric_prefix(slug.trim_end_matches('/'))
        .replace('/', "-")
        .replace('_', "-");

    for entry in walkdir::WalkDir::new(&content_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mdx") {
            continue;
        }

        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let stem_clean = strip_numeric_prefix(stem).replace('_', "-");

        // Match on last segment (e.g. URL ends in "net-worth-tracker", file is "127_net_worth_tracker")
        if stem_clean == last_segment_clean {
            return true;
        }

        // Full slug match for flat structures
        if stem_clean == full_slug_dashed {
            return true;
        }

        // Match relative path (e.g. "posts/net_worth_tracker.mdx")
        if let Ok(rel) = path.strip_prefix(&content_dir) {
            let rel_str = rel
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            let rel_without_ext = rel_str.trim_end_matches(".mdx").trim_end_matches(".md");
            let rel_clean = strip_numeric_prefix(rel_without_ext)
                .replace('/', "-")
                .replace('_', "-");
            if rel_clean == full_slug_dashed {
                return true;
            }
        }
    }

    false
}

fn strip_numeric_prefix(s: &str) -> String {
    let re = regex::Regex::new(r"^\d+[_\-]+").unwrap();
    re.replace(s, "").to_string()
}

fn spawn_fix_task(
    conn: &Connection,
    parent: &Task,
    url: &str,
    reason: &str,
    action: &str,
    verdict: &str,
    priority_val: i32,
) -> Option<String> {
    let task_type = match reason {
        "robots_blocked" | "noindex" | "fetch_error" | "canonical_mismatch" => "fix_technical",
        "api_error" => "fix_gsc_access",
        _ => "fix_indexing",
    };

    let url_slug = {
        let without_scheme = url
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        if let Some(slash_pos) = without_scheme.find('/') {
            &without_scheme[slash_pos..]
        } else {
            url
        }
    };

    let reason_human = reason.replace('_', " ");
    let title = format!("Fix {}: {}", reason_human, url_slug);
    let description = format!(
        "URL: {}\nIssue: {}\nAction: {}\nVerdict: {}",
        url, reason, action, verdict
    );

    let priority_enum = if priority_val <= 30 {
        Priority::High
    } else {
        Priority::Medium
    };

    let spec = TaskSpec {
        project_id: parent.project_id.clone(),
        task_type: task_type.to_string(),
        title: Some(title),
        description: Some(description),
        phase: Some("implementation".to_string()),
        run_policy: Some(TaskRunPolicy::AutoEnqueue),
        priority: priority_enum,
        agent_policy: AgentPolicy::Optional,
        idempotency_key: None, // allow re-creation when previous task is done
        artifacts: vec![],
        depends_on: vec![],
        ..Default::default()
    };

    match TaskSpawner::spawn(conn, spec) {
        Ok(task) => {
            if let Err(e) =
                db::record_task_created(conn, url, &parent.project_id, &task.id, task_type)
            {
                log::warn!(
                    "[indexing_diagnostics] failed to record task creation: {}",
                    e
                );
            }
            Some(task.id)
        }
        Err(e) => {
            log::warn!("[indexing_diagnostics] failed to create fix task: {}", e);
            None
        }
    }
}
