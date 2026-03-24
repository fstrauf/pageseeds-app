/// GSC sync and collection execution module.
///
/// Covers:
///   - exec_gsc_sync_articles              (sync GSC metrics → articles.json)
///   - exec_collect_gsc                    (URL Inspection API → gsc_collection.json)
///   - exec_gsc_investigate                (agentic investigation of collection results)
///   - create_tasks_from_collection_after_exec  (post-completion task spawner)
///   - create_tasks_from_collection        (parse gsc_collection.json → fix tasks)
///   - normalize_site_for_url_match        (sc-domain: normalisation)
///   - normalize_url_for_comparison        (URL normalization for domain matching)

use rusqlite::Connection;

/// Normalize a URL for domain comparison by stripping scheme and www.
///
/// This ensures URLs like `https://www.example.com/page` and `https://example.com/page`
/// are treated as belonging to the same domain for validation purposes.
///
/// # Examples
/// - `https://www.example.com/page` → `example.com/page`
/// - `http://example.com/` → `example.com/`
/// - `https://EXAMPLE.COM` → `example.com`
pub(crate) fn normalize_url_for_comparison(url: &str) -> String {
    let lower = url.to_lowercase();
    let without_scheme = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("http://"))
        .unwrap_or(&lower);
    without_scheme.strip_prefix("www.").unwrap_or(without_scheme).to_string()
}

use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

// ─── GSC sync articles ────────────────────────────────────────────────────────

/// Native Rust replacement for `pageseeds automation seo gsc-sync-articles`.
///
/// Fetches page-level GSC metrics (90-day window) and writes a `gsc` block into
/// each matching article in automation/articles.json. Matching uses normalised
/// URL paths (scheme-stripped, trailing-slash removed, underscore→dash, lowercase)
/// with a secondary last-segment index.
pub(crate) fn exec_gsc_sync_articles(
    task: &Task,
    project_path: &str,
    gsc_token: Option<&str>,
) -> crate::engine::workflows::StepResult {
    use crate::config::env_resolver::EnvResolver;
    use regex::Regex;
    use std::collections::HashMap;
    let _ = task;

    let paths = ProjectPaths::from_path(project_path);
    let resolver = EnvResolver::new(project_path);

    // 1. Credentials
    let sa_path = match resolver.resolve("GSC_SERVICE_ACCOUNT_PATH")
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS"))
        .map(|(v, _)| v)
    {
        Some(p) => p,
        None => return crate::engine::workflows::StepResult {
            success: false,
            message: "GSC_SERVICE_ACCOUNT_PATH not configured — add it to ~/.config/automation/secrets.env".to_string(),
            output: None,
        },
    };

    // 2. Token
    let rt = tokio::runtime::Handle::current();
    let token = if let Some(token) = gsc_token {
        token.to_string()
    } else {
        match rt.block_on(crate::gsc::auth::get_service_account_token(&sa_path)) {
            Ok(t) => t.access_token,
            Err(e) => return crate::engine::workflows::StepResult {
                success: false,
                message: format!("GSC auth failed: {}", e),
                output: None,
            },
        }
    };

    // 3. articles.json
    let articles_path = paths.automation_dir.join("articles.json");
    let raw = match std::fs::read_to_string(&articles_path) {
        Ok(s) => s,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("articles.json not found: {}", e),
            output: None,
        },
    };
    let mut doc: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to parse articles.json: {}", e),
            output: None,
        },
    };

    // 4. site_url from manifest.json
    let site_url: String = {
        let manifest_path = paths.automation_dir.join("manifest.json");
        let from_manifest = std::fs::read_to_string(&manifest_path).ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| {
                v.get("gsc_site").or_else(|| v.get("url"))
                    .and_then(|u| u.as_str())
                    .map(String::from)
            });
        match from_manifest {
            Some(u) => u,
            None => return crate::engine::workflows::StepResult {
                success: false,
                message: "No site_url found in manifest.json — add 'url' or 'gsc_site' field".to_string(),
                output: None,
            },
        }
    };

    let base_url = if site_url.starts_with("sc-domain:") {
        format!("https://{}", &site_url["sc-domain:".len()..])
    } else {
        site_url.clone()
    };
    let base_url = base_url.trim_end_matches('/').to_string();
    let _ = &base_url;

    // 5. Fetch GSC metrics (90-day window)
    let days = 90i64;
    let end = chrono::Utc::now().date_naive() - chrono::Duration::days(1);
    let start = end - chrono::Duration::days(days - 1);
    let page_rows = match rt.block_on(crate::gsc::analytics::fetch_page_rows(
        &token, &site_url,
        &start.format("%Y-%m-%d").to_string(),
        &end.format("%Y-%m-%d").to_string(),
        1000,
    )) {
        Ok(rows) => rows,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("GSC fetch failed: {}", e),
            output: None,
        },
    };

    // 6. Build normalised path lookup
    let num_prefix_re = Regex::new(r"^\d+[_\-]+").unwrap();

    let normalize_path = |url: &str| -> String {
        let stripped = if let Some(rest) = url.strip_prefix("https://") { rest }
            else if let Some(rest) = url.strip_prefix("http://") { rest }
            else { url };
        let path = if let Some(slash) = stripped.find('/') { &stripped[slash..] } else { "/" };
        path.trim_end_matches('/').replace('_', "-").to_lowercase()
    };

    let mut gsc_by_path: HashMap<String, &crate::models::gsc::PageMetrics> = HashMap::new();
    for row in &page_rows {
        let p = normalize_path(&row.page);
        if !p.is_empty() { gsc_by_path.entry(p).or_insert(row); }
    }
    let mut gsc_by_segment: HashMap<String, &crate::models::gsc::PageMetrics> = HashMap::new();
    for (path, m) in &gsc_by_path {
        let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
        if !last.is_empty() {
            gsc_by_segment.entry(last.clone()).or_insert(m);
            let stripped = num_prefix_re.replace(&last, "").to_string();
            if stripped != last && !stripped.is_empty() {
                gsc_by_segment.entry(stripped).or_insert(m);
            }
        }
    }

    // 7. Match articles and write gsc block
    let now_iso = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let articles = doc["articles"].as_array_mut()
        .ok_or("no articles array")
        .unwrap();

    let mut matched = 0usize;
    let mut unmatched = 0usize;

    for article in articles.iter_mut() {
        let slug = article["url_slug"].as_str().unwrap_or("").to_string();
        let file_ref = article["file"].as_str().unwrap_or("").to_string();

        let article_path: String = if !slug.is_empty() {
            let s = slug.trim_matches('/').replace('_', "-").to_lowercase();
            format!("/{}", s)
        } else if !file_ref.is_empty() {
            let stem = std::path::Path::new(&file_ref)
                .file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let s = num_prefix_re.replace(&stem, "").to_string();
            format!("/{}", s.replace('_', "-").to_lowercase())
        } else {
            article["gsc"] = serde_json::Value::Null;
            unmatched += 1;
            continue;
        };

        let metrics = gsc_by_path.get(&article_path)
            .or_else(|| gsc_by_segment.get(article_path.trim_start_matches('/')));

        if let Some(m) = metrics {
            article["gsc"] = serde_json::json!({
                "impressions": m.impressions,
                "clicks": m.clicks,
                "ctr": (m.ctr * 10000.0).round() / 10000.0,
                "avg_position": (m.position * 10.0).round() / 10.0,
                "last_synced": now_iso,
                "period_days": days,
            });
            matched += 1;
        } else {
            article["gsc"] = serde_json::Value::Null;
            unmatched += 1;
        }
    }

    // 8. Write back
    let out = serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&articles_path, &out) {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to write articles.json: {}", e),
            output: None,
        };
    }

    let summary = serde_json::json!({
        "matched": matched,
        "unmatched": unmatched,
        "total": matched + unmatched,
        "gsc_rows": page_rows.len(),
        "site": site_url,
        "period_days": days,
    });

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "GSC sync: matched {}/{} articles ({} GSC pages fetched)",
            matched, matched + unmatched, page_rows.len()
        ),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
    }
}

// ─── GSC collection ───────────────────────────────────────────────────────────

/// Native Rust implementation of the GSC collection step.
///
/// 1. Reads sitemap URL from manifest.json.
/// 2. Mints a service account token.
/// 3. Fetches all sitemap URLs (up to 200).
/// 4. Calls the URL Inspection API for each URL.
/// 5. Classifies each result into a reason code.
/// 6. Writes `gsc_collection.json` to the automation dir.
pub(crate) fn exec_collect_gsc(
    task: &Task,
    project_path: &str,
    gsc_token: Option<&str>,
) -> crate::engine::workflows::StepResult {
    use crate::config::env_resolver::EnvResolver;
    use std::collections::HashMap;
    let _ = task;

    let paths = ProjectPaths::from_path(project_path);
    let resolver = EnvResolver::new(project_path);

    // 1. manifest.json → site_url + sitemap_url
    let manifest_path = paths.automation_dir.join("manifest.json");
    let manifest: serde_json::Value = match std::fs::read_to_string(&manifest_path) {
        Ok(s) => match serde_json::from_str(&s) {
            Ok(v) => v,
            Err(e) => return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to parse manifest.json: {}", e),
                output: None,
            },
        },
        Err(_) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "manifest.json not found at {} — run 'Init Workspace' first",
                manifest_path.display()
            ),
            output: None,
        },
    };

    let site_url = match manifest.get("gsc_site").or_else(|| manifest.get("url"))
        .and_then(|v| v.as_str()).map(String::from)
    {
        Some(u) => u,
        None => return crate::engine::workflows::StepResult {
            success: false,
            message: "No 'url' or 'gsc_site' field in manifest.json".to_string(),
            output: None,
        },
    };

    let sitemap_url = manifest.get("sitemap")
        .and_then(|v| v.as_str())
        .map(String::from)
        .unwrap_or_else(|| format!("{}/sitemap.xml", normalize_site_for_url_match(&site_url)));

    log::info!("[collect_gsc] site_url={} sitemap_url={}", site_url, sitemap_url);
    let site_match_prefix = normalize_site_for_url_match(&site_url);

    // 2. Credentials + token
    let sa_path = match resolver.resolve("GSC_SERVICE_ACCOUNT_PATH")
        .or_else(|| resolver.resolve("GOOGLE_APPLICATION_CREDENTIALS"))
        .map(|(v, _)| v)
    {
        Some(p) => p,
        None => return crate::engine::workflows::StepResult {
            success: false,
            message: "GSC_SERVICE_ACCOUNT_PATH not configured — add it in Settings → Secrets".to_string(),
            output: None,
        },
    };

    let rt = tokio::runtime::Handle::current();
    let token = if let Some(token) = gsc_token {
        token.to_string()
    } else {
        match rt.block_on(crate::gsc::auth::get_service_account_token(&sa_path)) {
            Ok(t) => t.access_token,
            Err(e) => return crate::engine::workflows::StepResult {
                success: false,
                message: format!("GSC auth failed: {}", e),
                output: None,
            },
        }
    };

    // 3. Fetch sitemap URLs
    let urls = match rt.block_on(crate::gsc::sitemap::fetch_sitemap_urls(&sitemap_url, 200)) {
        Ok(u) if !u.is_empty() => u,
        Ok(_) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Sitemap at '{}' is empty or unreachable", sitemap_url),
            output: None,
        },
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to fetch sitemap: {}", e),
            output: None,
        },
    };

    log::info!("[collect_gsc] {} URLs to inspect", urls.len());

    // Fast-fail: check that sitemap domain matches gsc_site
    let sample_size = urls.len().min(10);
    
    // Normalize for comparison (strip scheme and www.)
    let site_normalized = normalize_url_for_comparison(&site_match_prefix);
    let sample_matches = urls.iter().take(sample_size)
        .filter(|u| normalize_url_for_comparison(u).starts_with(&site_normalized)).count();
    
    // Debug: log the comparison
    if sample_size > 0 {
        let first_urls: Vec<&str> = urls.iter().take(3).map(|s| s.as_str()).collect();
        log::info!("[collect_gsc] site_match_prefix='{}' (normalized: '{}'), sample URLs: {:?}", 
            site_match_prefix, site_normalized, first_urls);
        log::info!("[collect_gsc] URL match check: {}/{} match normalized prefix '{}'", 
            sample_matches, sample_size, site_normalized);
    }
    
    if sample_size > 0 && sample_matches == 0 {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "GSC site URL mismatch: 0/{} sitemap URLs match '{}'. Check 'url'/'gsc_site' in manifest.json.",
                sample_size, site_match_prefix
            ),
            output: None,
        };
    }

    // 4. URL Inspection API
    let records = match rt.block_on(crate::gsc::indexing::inspect_batch(&token, &site_url, urls.clone())) {
        Ok(r) => r,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("URL Inspection API failed: {}", e),
            output: None,
        },
    };

    // 5. Domain validation (normalize for www. comparison)
    let site_domain_normalized = normalize_url_for_comparison(&site_match_prefix);
    let url_matching = records.iter()
        .filter(|r| normalize_url_for_comparison(&r.url).starts_with(&site_domain_normalized))
        .count();
    if records.len() > 5 && url_matching < records.len() / 2 {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "GSC site URL mismatch: only {}/{} URLs match '{}'. Check 'url' in manifest.json.",
                url_matching, records.len(), site_match_prefix
            ),
            output: None,
        };
    }

    // 6. Build output
    let mut counts: HashMap<String, u32> = HashMap::new();
    for rec in &records {
        *counts.entry(rec.reason_code.as_deref().unwrap_or("unknown").to_string()).or_insert(0) += 1;
    }

    let issues_found = records.iter()
        .filter(|r| r.reason_code.as_deref().unwrap_or("") != "indexed_pass")
        .count();

    let mut items: Vec<serde_json::Value> = records.iter().map(|r| serde_json::json!({
        "url": r.url,
        "verdict": r.verdict,
        "coverage_state": r.coverage_state,
        "reason_code": r.reason_code,
        "action": r.action,
        "priority": r.priority,
    })).collect();
    items.sort_by_key(|item| item["priority"].as_i64().unwrap_or(999));

    let now_iso = chrono::Utc::now().to_rfc3339();
    let collection = serde_json::json!({
        "meta": {
            "site_url": site_url,
            "sitemap_url": sitemap_url,
            "collected_at": now_iso,
            "total_urls": records.len(),
            "issues_found": issues_found,
        },
        "counts": counts,
        "items": items,
    });

    // 7. Write gsc_collection.json
    let output_path = paths.automation_dir.join("gsc_collection.json");
    if let Err(e) = std::fs::create_dir_all(&paths.automation_dir) {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to create automation dir: {}", e),
            output: None,
        };
    }
    let json_str = serde_json::to_string_pretty(&collection).unwrap_or_default();
    if let Err(e) = std::fs::write(&output_path, &json_str) {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to write gsc_collection.json: {}", e),
            output: None,
        };
    }

    log::info!("[collect_gsc] wrote {} — {} URLs, {} issues", output_path.display(), records.len(), issues_found);

    crate::engine::workflows::StepResult {
        success: true,
        message: format!("{} URLs inspected, {} issues found", records.len(), issues_found),
        output: Some(json_str),
    }
}

/// Normalise a GSC property identifier into a URL prefix suitable for `starts_with`.
///
/// Converts various GSC site formats to a canonical `https://domain` prefix,
/// stripping `www.` to ensure consistent matching regardless of subdomain.
///
/// - `sc-domain:example.com` → `https://example.com`
/// - `sc-domain:www.example.com` → `https://example.com`
/// - `https://example.com/`  → `https://example.com`
/// - `https://www.example.com/` → `https://example.com`
/// - `http://example.com` → `https://example.com`
pub(crate) fn normalize_site_for_url_match(site_url: &str) -> String {
    // Strip sc-domain: prefix if present
    let without_prefix = site_url.strip_prefix("sc-domain:").unwrap_or(site_url);
    
    // Use shared normalization for scheme/www stripping, then reconstruct as https://
    let normalized = normalize_url_for_comparison(without_prefix);
    format!("https://{}", normalized.trim_start_matches('/'))
}

/// Post-completion hook: reads gsc_collection.json and spawns fix tasks.
///
/// Called from `execute_task_with_token` after a successful `collect_gsc` task.
pub(crate) fn create_tasks_from_collection_after_exec(conn: &Connection, parent_task: &Task, project_path: &str) -> Vec<String> {
    let paths = ProjectPaths::from_path(project_path);
    let collection_path = paths.automation_dir.join("gsc_collection.json");

    let json_str = match std::fs::read_to_string(&collection_path) {
        Ok(s) => s,
        Err(_) => {
            log::info!("[collect_gsc] gsc_collection.json not found — no tasks created");
            return vec![];
        }
    };
    let data: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[collect_gsc] failed to parse gsc_collection.json: {}", e);
            return vec![];
        }
    };

    let created_ids = create_tasks_from_collection(conn, parent_task, &data);
    log::info!("[collect_gsc] spawned {} fix tasks", created_ids.len());
    created_ids
}

/// Parse gsc_collection.json and create specific fix tasks in SQLite.
///
/// Maps reason codes to task types:
///   robots_blocked / noindex / fetch_error / canonical_mismatch → fix_technical
///   not_indexed_*                                               → fix_indexing
///   api_error                                                   → fix_gsc_access (batched)
///   (all indexed)                                               → investigate_gsc
pub(crate) fn create_tasks_from_collection(
    conn: &Connection,
    parent_task: &Task,
    data: &serde_json::Value,
) -> Vec<String> {
    use crate::engine::task_store;
    use crate::models::task::{Task as TaskModel, TaskRun};

    let items = match data["items"].as_array() {
        Some(a) => a,
        None => return vec![],
    };

    let now = chrono::Utc::now().to_rfc3339();
    let mut created_ids: Vec<String> = vec![];
    let mut seen_issues = std::collections::HashSet::<String>::new();
    let mut api_error_count = 0u32;

    for item in items.iter().take(20) {
        let url = item["url"].as_str().unwrap_or("");
        let reason = item["reason_code"].as_str().unwrap_or("unknown");
        let action = item["action"].as_str().unwrap_or("");
        let verdict = item["verdict"].as_str().unwrap_or("");
        let priority_val = item["priority"].as_i64().unwrap_or(999);

        if reason == "indexed_pass" { continue; }

        if reason == "api_error" {
            api_error_count += 1;
            continue;
        }

        let issue_key = format!("{}:{}", reason, url);
        if seen_issues.contains(&issue_key) { continue; }
        seen_issues.insert(issue_key);

        let task_type = match reason {
            "robots_blocked" | "noindex" | "fetch_error" | "canonical_mismatch" => "fix_technical",
            _ => "fix_indexing",
        };

        let existing: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE project_id=?1 AND type=?2 AND status IN ('todo','in_progress') AND (title LIKE ?3 OR description LIKE ?3)",
            rusqlite::params![&parent_task.project_id, task_type, format!("%{}%", url)],
            |r| r.get(0),
        ).unwrap_or(0);
        if existing > 0 { continue; }

        let url_slug = {
            let without_scheme = url.trim_start_matches("https://").trim_start_matches("http://");
            if let Some(slash_pos) = without_scheme.find('/') { &without_scheme[slash_pos..] } else { url }
        };
        let reason_human = reason.replace('_', " ");
        let title = format!("Fix {}: {}", reason_human, url_slug);
        let description = format!("URL: {}\nIssue: {}\nAction: {}\nVerdict: {}", url, reason, action, verdict);

        let priority = if priority_val <= 30 { "high" } else { "medium" };
        let task_id = format!("task-{}", chrono::Utc::now().timestamp_millis() + created_ids.len() as i64);
        let priority_enum = if priority_val <= 30 { crate::models::task::Priority::High } else { crate::models::task::Priority::Medium };

        let new_task = TaskModel {
            id: task_id,
            task_type: task_type.to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: priority_enum,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::Optional,
            title: Some(title),
            description: Some(description),
            project_id: parent_task.project_id.clone(),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun::default(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        match task_store::create_task(conn, &new_task) {
            Ok(_) => created_ids.push(new_task.id.clone()),
            Err(e) => log::warn!("[collect_gsc] failed to create fix task: {}", e),
        }
    }

    // One batched fix_gsc_access task for all API errors
    if api_error_count > 0 {
        let existing: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tasks WHERE project_id=?1 AND type='fix_gsc_access' AND status IN ('todo','in_progress')",
            rusqlite::params![&parent_task.project_id],
            |r| r.get(0),
        ).unwrap_or(0);
        if existing == 0 {
            let task_id = format!("task-gsc-access-{}", chrono::Utc::now().timestamp_millis());
            let new_task = crate::models::task::Task {
                id: task_id,
                task_type: "fix_gsc_access".to_string(),
                phase: "implementation".to_string(),
                status: crate::models::task::TaskStatus::Todo,
                priority: crate::models::task::Priority::High,
                execution_mode: crate::models::task::ExecutionMode::Manual,
                agent_policy: crate::models::task::AgentPolicy::Optional,
                title: Some(format!("Fix GSC API access errors ({} URLs affected)", api_error_count)),
                description: Some("GSC URL Inspection API returned errors. Check service account property access.".to_string()),
                project_id: parent_task.project_id.clone(),
                depends_on: vec![],
                artifacts: vec![],
                run: crate::models::task::TaskRun::default(),
                created_at: chrono::Utc::now().to_rfc3339(),
                updated_at: chrono::Utc::now().to_rfc3339(),
            };
            if task_store::create_task(conn, &new_task).is_ok() {
                created_ids.push(new_task.id.clone());
            }
        }
    }

    // If no issues — all pages indexed — trigger investigation
    if created_ids.is_empty() && api_error_count == 0 {
        let all_indexed = items.iter()
            .all(|i| i["reason_code"].as_str().unwrap_or("") == "indexed_pass");
        if all_indexed {
            let existing: i64 = conn.query_row(
                "SELECT COUNT(*) FROM tasks WHERE project_id=?1 AND type='investigate_gsc' AND status IN ('todo','in_progress')",
                rusqlite::params![&parent_task.project_id],
                |r| r.get(0),
            ).unwrap_or(0);
            if existing == 0 {
                let task_id = format!("task-inv-gsc-{}", chrono::Utc::now().timestamp_millis());
                let new_task = crate::models::task::Task {
                    id: task_id,
                    task_type: "investigate_gsc".to_string(),
                    phase: "investigation".to_string(),
                    status: crate::models::task::TaskStatus::Todo,
                    priority: crate::models::task::Priority::Medium,
                    execution_mode: crate::models::task::ExecutionMode::Automatic,
                    agent_policy: crate::models::task::AgentPolicy::Required,
                    title: Some("Investigate GSC — all pages indexed, look for opportunities".to_string()),
                    description: Some("gsc_collection.json shows all pages are indexed. Run investigation to find optimization opportunities.".to_string()),
                    project_id: parent_task.project_id.clone(),
                    depends_on: vec![],
                    artifacts: vec![],
                    run: crate::models::task::TaskRun::default(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                    updated_at: chrono::Utc::now().to_rfc3339(),
                };
                if task_store::create_task(conn, &new_task).is_ok() {
                    created_ids.push(new_task.id.clone());
                }
            }
        }
    }

    created_ids
}

// ─── GSC summary (deterministic pre-step for investigate_gsc) ────────────────

/// Deterministic pre-step for `investigate_gsc`.
///
/// Reads gsc_collection.json and produces a compact structured summary grouped
/// by reason_code, with counts, percentages, and up to 5 example URLs per group.
/// Writes gsc_summary.json to the automation dir.
///
/// The agentic investigation step reads this summary rather than raw collection data,
/// so the agent interprets patterns and recommends actions instead of re-doing trivial
/// counting and grouping that a `group_by().count()` handles exactly.
pub(crate) fn exec_gsc_summarise(
    task: &Task,
    project_path: &str,
) -> crate::engine::workflows::StepResult {
    use serde_json::{json, Value};
    use std::collections::HashMap;
    let _ = task;

    let paths = ProjectPaths::from_path(project_path);
    let collection_path = paths.automation_dir.join("gsc_collection.json");

    let raw = match std::fs::read_to_string(&collection_path) {
        Ok(s) => s,
        Err(_) => return crate::engine::workflows::StepResult {
            success: false,
            message: "gsc_collection.json not found — run collect_gsc first".to_string(),
            output: None,
        },
    };

    let collection: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to parse gsc_collection.json: {}", e),
            output: None,
        },
    };

    let items = match collection.get("items").and_then(|v| v.as_array()) {
        Some(arr) => arr.clone(),
        None => return crate::engine::workflows::StepResult {
            success: false,
            message: "gsc_collection.json has no 'items' array".to_string(),
            output: None,
        },
    };

    let total = items.len();
    let mut by_reason: HashMap<String, Vec<String>> = HashMap::new();
    let mut indexed_count = 0usize;

    for item in &items {
        let reason = item.get("reason_code")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();
        let url = item.get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if reason == "indexed_pass" { indexed_count += 1; }
        by_reason.entry(reason).or_default().push(url);
    }

    let non_indexed_count = total - indexed_count;

    let mut groups: Vec<Value> = by_reason.iter().map(|(reason, urls)| {
        let count = urls.len();
        let pct = if total > 0 { (count * 100) / total } else { 0 };
        let examples: Vec<&String> = urls.iter().take(5).collect();
        json!({
            "reason_code": reason,
            "count": count,
            "percentage": pct,
            "example_urls": examples,
        })
    }).collect();

    // Sort by count descending so the most common issues appear first.
    groups.sort_by(|a, b| {
        let ca = a.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
        let cb = b.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
        cb.cmp(&ca)
    });

    let summary = json!({
        "total_inspected": total,
        "indexed_count": indexed_count,
        "non_indexed_count": non_indexed_count,
        "by_reason": groups,
    });

    let summary_path = paths.automation_dir.join("gsc_summary.json");
    let summary_str = serde_json::to_string_pretty(&summary).unwrap_or_default();
    if let Err(e) = std::fs::write(&summary_path, &summary_str) {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!("Failed to write gsc_summary.json: {}", e),
            output: None,
        };
    }

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "GSC summary: {} total, {} indexed, {} non-indexed ({} reason groups)",
            total, indexed_count, non_indexed_count, by_reason.len()
        ),
        output: Some(summary_str),
    }
}

// ─── GSC investigation ────────────────────────────────────────────────────────

/// Agentic investigation step for `investigate_gsc`.
///
/// Reads gsc_summary.json (produced by the deterministic `gsc_summarise` pre-step)
/// and passes the structured summary to the LLM. The agent interprets *why* certain
/// reason groups are occurring, identifies cross-cutting patterns, and recommends
/// corrective actions — judgment that `group_by().count()` cannot provide.
///
/// Falls back to gsc_collection.json if the summary is not yet written.
pub(crate) fn exec_gsc_investigate(
    step: &crate::engine::workflows::WorkflowStep,
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> crate::engine::workflows::StepResult {
    use crate::engine::agent;
    use std::path::Path;
    let _ = step;

    let paths = ProjectPaths::from_path(project_path);

    // Prefer the pre-processed summary; fall back to raw collection if missing.
    let summary_path = paths.automation_dir.join("gsc_summary.json");
    let collection_path = paths.automation_dir.join("gsc_collection.json");

    let (context_json, context_label) = if let Ok(s) = std::fs::read_to_string(&summary_path) {
        (s, "GSC Summary (pre-processed)")
    } else if let Ok(s) = std::fs::read_to_string(&collection_path) {
        (s, "GSC Collection (raw)")
    } else {
        return crate::engine::workflows::StepResult {
            success: false,
            message: "Neither gsc_summary.json nor gsc_collection.json found — run collect_gsc first".to_string(),
            output: None,
        };
    };

    let prompt = format!(
        "## Task: Investigate GSC Indexing Results\n\n\
         - Task ID: {}\n\
         - Site: {}\n\
         - Repo: {}\n\n\
         ## {}\n\n\
         ```json\n{}\n```\n\n\
         ## Instructions\n\n\
         The data above groups pages by indexing reason code with counts and example URLs.\n\
         Your job is to interpret the patterns — not count or regroup them.\n\n\
         For each non-indexed reason group:\n\
         1. Explain the likely root cause in one sentence\n\
         2. Recommend a specific corrective action\n\
         3. Assign a priority (high/medium/low) based on count and impact\n\n\
         Return a JSON object:\n\
         ```json\n\
         {{\n  \"summary\": \"...\",\n  \"issues_found\": [\n    {{\n      \
         \"reason_code\": \"...\",\n      \"url_count\": 0,\n      \"root_cause\": \"...\",\n      \
         \"recommendation\": \"...\",\n      \"priority\": \"high|medium|low\"\n    \
         }}\n  ]\n}}\n\
         ```",
        task.id,
        project_path,
        project_path,
        context_label,
        context_json,
    );

    match agent::run_agent(agent_provider, &prompt, Path::new(project_path)) {
        Ok(output) => crate::engine::workflows::StepResult {
            success: true,
            message: format!("GSC investigation complete ({} chars)", output.len()),
            output: Some(output),
        },
        Err(e) => crate::engine::workflows::StepResult {
            success: false,
            message: format!("GSC investigation agent failed: {}", e),
            output: None,
        },
    }
}
