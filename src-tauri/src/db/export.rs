/// JSON import/export to maintain compatibility with task_list.json and articles.json
/// formats used by the pageseeds-cli Python tooling.
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

use crate::error::{Error, Result};
use crate::models::article::Article;

// ─── task_list.json (v4) ─────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct TaskListJson {
    pub version: u32,
    pub metadata: TaskListMetadata,
    pub tasks: Vec<Value>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TaskListMetadata {
    #[serde(default)]
    pub next_task_sequence: Option<i64>,
    #[serde(default)]
    pub project_id: Option<String>,
}

/// Import all tasks from a task_list.json string into the database.
/// Returns the number of tasks upserted.
pub fn import_task_list(conn: &Connection, project_id: &str, json: &str) -> Result<usize> {
    let parsed: Value = serde_json::from_str(json)?;
    let tasks = parsed["tasks"]
        .as_array()
        .ok_or_else(|| Error::Other("No 'tasks' array in task_list.json".into()))?;

    let now = chrono::Utc::now().to_rfc3339();
    let mut count = 0;

    for t in tasks {
        let id = match t["id"].as_str() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };

        let task_type = t["type"].as_str().unwrap_or("unknown").to_string();
        let phase = t["phase"].as_str().unwrap_or("implementation").to_string();
        let status = t["status"].as_str().unwrap_or("todo").to_string();
        let priority = t["priority"].as_str().unwrap_or("medium").to_string();
        // New policy fields (v5+ format). Fall back to execution_mode mapping for legacy exports.
        let run_policy = t["run_policy"].as_str()
            .or_else(|| t["execution_mode"].as_str().map(|em| match em {
                "automatic" | "batchable" => "auto_enqueue",
                _ => "user_enqueue",
            }))
            .unwrap_or("user_enqueue")
            .to_string();
        let review_surface = t["review_surface"].as_str().unwrap_or("none").to_string();
        let follow_up_policy = t["follow_up_policy"].as_str().unwrap_or("none").to_string();
        let agent_policy = t["agent_policy"].as_str().unwrap_or("none").to_string();
        let title = t["title"].as_str().map(String::from);
        let description = t["description"].as_str().map(String::from);

        let depends_on = serde_json::to_string(&t["depends_on"]).unwrap_or_else(|_| "[]".into());
        let artifacts = serde_json::to_string(&t["artifacts"]).unwrap_or_else(|_| "[]".into());

        let run = &t["run"];
        let run_attempts = run["attempts"].as_u64().unwrap_or(0) as i64;
        let run_last_error = run["last_error"].as_str().map(String::from);
        let run_provider = run["provider"].as_str().map(String::from);

        conn.execute(
            "INSERT INTO tasks (
                id, type, phase, status, priority, execution_mode, agent_policy,
                run_policy, review_surface, follow_up_policy,
                title, description, project_id, depends_on, artifacts,
                run_attempts, run_last_error, run_provider, created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)
             ON CONFLICT(id) DO UPDATE SET
                status           = excluded.status,
                priority         = excluded.priority,
                title            = excluded.title,
                description      = excluded.description,
                run_policy       = excluded.run_policy,
                review_surface   = excluded.review_surface,
                follow_up_policy = excluded.follow_up_policy,
                depends_on       = excluded.depends_on,
                artifacts        = excluded.artifacts,
                run_attempts     = excluded.run_attempts,
                run_last_error   = excluded.run_last_error,
                run_provider     = excluded.run_provider,
                updated_at       = excluded.updated_at",
            rusqlite::params![
                id,
                task_type,
                phase,
                status,
                priority,
                "manual", // execution_mode legacy column
                agent_policy,
                run_policy,
                review_surface,
                follow_up_policy,
                title,
                description,
                project_id,
                depends_on,
                artifacts,
                run_attempts,
                run_last_error,
                run_provider,
                now,
                now,
            ],
        )?;
        count += 1;
    }
    Ok(count)
}

/// Export all tasks for a project to task_list.json format (v4).
pub fn export_task_list(conn: &Connection, project_id: &str) -> Result<String> {
    let mut stmt = conn.prepare(
        "SELECT id, type, phase, status, priority, run_policy, review_surface, follow_up_policy, agent_policy,
                title, description, depends_on, artifacts,
                run_attempts, run_last_error, run_provider, created_at, updated_at
         FROM tasks WHERE project_id = ?1 ORDER BY created_at ASC",
    )?;

    let tasks: Vec<Value> = stmt
        .query_map([project_id], |row| {
            let id: String = row.get(0)?;
            let task_type: String = row.get(1)?;
            let phase: String = row.get(2)?;
            let status: String = row.get(3)?;
            let priority: String = row.get(4)?;
            let run_policy: String = row.get(5)?;
            let review_surface: String = row.get(6)?;
            let follow_up_policy: String = row.get(7)?;
            let agent_policy: String = row.get(8)?;
            let title: Option<String> = row.get(9)?;
            let description: Option<String> = row.get(10)?;
            let depends_on_str: String = row.get(11)?;
            let artifacts_str: String = row.get(12)?;
            let run_attempts: i64 = row.get(13)?;
            let run_last_error: Option<String> = row.get(14)?;
            let run_provider: Option<String> = row.get(15)?;
            Ok((
                id,
                task_type,
                phase,
                status,
                priority,
                run_policy,
                review_surface,
                follow_up_policy,
                agent_policy,
                title,
                description,
                depends_on_str,
                artifacts_str,
                run_attempts,
                run_last_error,
                run_provider,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(
            |(
                id,
                task_type,
                phase,
                status,
                priority,
                run_policy,
                review_surface,
                follow_up_policy,
                agent_policy,
                title,
                description,
                depends_on_str,
                artifacts_str,
                run_attempts,
                run_last_error,
                run_provider,
            )| {
                let depends_on: Value =
                    serde_json::from_str(&depends_on_str).unwrap_or(Value::Array(vec![]));
                let artifacts: Value =
                    serde_json::from_str(&artifacts_str).unwrap_or(Value::Array(vec![]));
                let mut obj = serde_json::json!({
                    "id": id,
                    "type": task_type,
                    "phase": phase,
                    "status": status,
                    "priority": priority,
                    "run_policy": run_policy,
                    "review_surface": review_surface,
                    "follow_up_policy": follow_up_policy,
                    "agent_policy": agent_policy,
                    "depends_on": depends_on,
                    "artifacts": artifacts,
                    "run": {
                        "attempts": run_attempts,
                        "last_error": run_last_error,
                        "provider": run_provider,
                    }
                });
                if let Some(t) = title {
                    obj["title"] = Value::String(t);
                }
                if let Some(d) = description {
                    obj["description"] = Value::String(d);
                }
                obj
            },
        )
        .collect();

    let output = TaskListJson {
        version: 4,
        metadata: TaskListMetadata {
            next_task_sequence: None,
            project_id: Some(project_id.to_string()),
        },
        tasks,
    };

    Ok(serde_json::to_string_pretty(&output)?)
}

/// Write task_list.json to the project's .github/automation directory.
pub fn write_task_list_to_repo(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
) -> Result<()> {
    let json = export_task_list(conn, project_id)?;
    let out_dir = project_path.join(".github").join("automation");
    std::fs::create_dir_all(&out_dir)?;
    std::fs::write(out_dir.join("task_list.json"), json)?;
    Ok(())
}

/// Read task_list.json from the project's .github/automation directory and import.
pub fn read_task_list_from_repo(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
) -> Result<usize> {
    let json_path = project_path
        .join(".github")
        .join("automation")
        .join("task_list.json");
    if !json_path.exists() {
        return Ok(0);
    }
    let json = std::fs::read_to_string(&json_path)?;
    import_task_list(conn, project_id, &json)
}

// ─── articles.json ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct ArticlesJson {
    #[serde(rename = "nextArticleId")]
    pub next_article_id: i64,
    pub articles: Vec<Value>,
}

/// Import articles from articles.json into the database.
pub fn import_articles(conn: &Connection, project_id: &str, json: &str) -> Result<usize> {
    let parsed: ArticlesJson = serde_json::from_str(json)?;

    conn.execute(
        "INSERT INTO articles_meta (project_id, next_article_id)
         VALUES (?1, ?2)
         ON CONFLICT(project_id) DO UPDATE SET next_article_id = excluded.next_article_id",
        rusqlite::params![project_id, parsed.next_article_id],
    )?;

    let mut count = 0;
    for a in &parsed.articles {
        let id = match a["id"].as_i64() {
            Some(v) => v,
            None => continue,
        };
        let title = a["title"].as_str().unwrap_or("").to_string();
        let url_slug = a["url_slug"].as_str().unwrap_or("").to_string();
        let file = a["file"].as_str().unwrap_or("").to_string();
        let target_keyword = a["target_keyword"].as_str().map(String::from);
        let keyword_difficulty = a["keyword_difficulty"].as_str().map(String::from);
        let target_volume = a["target_volume"].as_i64().unwrap_or(0);
        let published_date = a["published_date"].as_str().map(String::from);
        let word_count = a["word_count"].as_i64().unwrap_or(0);
        let status = a["status"].as_str().unwrap_or("draft").to_string();
        let review_status = a["review_status"].as_str().map(String::from);
        let review_started_at = a["review_started_at"].as_str().map(String::from);
        let last_reviewed_at = a["last_reviewed_at"].as_str().map(String::from);
        let review_count = a["review_count"].as_i64().unwrap_or(0);
        let page_type = a["page_type"].as_str().map(String::from);
        let gaps =
            serde_json::to_string(&a["content_gaps_addressed"]).unwrap_or_else(|_| "[]".into());
        let estimated_traffic = a["estimated_traffic_monthly"].as_str().map(String::from);

        conn.execute(
            "INSERT INTO articles (
                id, title, url_slug, file, target_keyword, keyword_difficulty,
                target_volume, published_date, word_count, status,
                review_status, review_started_at, last_reviewed_at, review_count,
                content_gaps_addressed, estimated_traffic_monthly, page_type, project_id
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
             ON CONFLICT(id, project_id) DO UPDATE SET
                title                       = excluded.title,
                url_slug                    = excluded.url_slug,
                file                        = excluded.file,
                target_keyword              = excluded.target_keyword,
                keyword_difficulty          = excluded.keyword_difficulty,
                target_volume               = excluded.target_volume,
                published_date              = excluded.published_date,
                word_count                  = excluded.word_count,
                status                      = excluded.status,
                review_status               = excluded.review_status,
                review_started_at           = excluded.review_started_at,
                last_reviewed_at            = excluded.last_reviewed_at,
                review_count                = excluded.review_count,
                content_gaps_addressed      = excluded.content_gaps_addressed,
                estimated_traffic_monthly   = excluded.estimated_traffic_monthly,
                page_type                   = excluded.page_type",
            rusqlite::params![
                id,
                title,
                url_slug,
                file,
                target_keyword,
                keyword_difficulty,
                target_volume,
                published_date,
                word_count,
                status,
                review_status,
                review_started_at,
                last_reviewed_at,
                review_count,
                gaps,
                estimated_traffic,
                page_type,
                project_id,
            ],
        )?;
        count += 1;
    }
    Ok(count)
}

/// Export articles for a project back to articles.json format.
pub fn export_articles(conn: &Connection, project_id: &str) -> Result<String> {
    let next_article_id: i64 = conn
        .query_row(
            "SELECT next_article_id FROM articles_meta WHERE project_id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .unwrap_or(1);

    let mut stmt = conn.prepare(
        "SELECT id, title, url_slug, file, target_keyword, keyword_difficulty,
                target_volume, published_date, word_count, status,
                review_status, review_started_at, last_reviewed_at, review_count,
                content_gaps_addressed, estimated_traffic_monthly, page_type
         FROM articles WHERE project_id = ?1 ORDER BY id ASC",
    )?;

    let articles: Vec<Value> = stmt
        .query_map([project_id], |row| {
            let id: i64 = row.get(0)?;
            let title: String = row.get(1)?;
            let url_slug: String = row.get(2)?;
            let file: String = row.get(3)?;
            let target_keyword: Option<String> = row.get(4)?;
            let keyword_difficulty: Option<String> = row.get(5)?;
            let target_volume: i64 = row.get(6)?;
            let published_date: Option<String> = row.get(7)?;
            let word_count: i64 = row.get(8)?;
            let status: String = row.get(9)?;
            let review_status: Option<String> = row.get(10)?;
            let review_started_at: Option<String> = row.get(11)?;
            let last_reviewed_at: Option<String> = row.get(12)?;
            let review_count: i64 = row.get::<_, Option<i64>>(13)?.unwrap_or(0);
            let gaps_str: String = row.get(14)?;
            let estimated_traffic: Option<String> = row.get(15)?;
            let page_type: Option<String> = row.get(16)?;
            Ok((
                id,
                title,
                url_slug,
                file,
                target_keyword,
                keyword_difficulty,
                target_volume,
                published_date,
                word_count,
                status,
                review_status,
                review_started_at,
                last_reviewed_at,
                review_count,
                gaps_str,
                estimated_traffic,
                page_type,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(
            |(
                id,
                title,
                url_slug,
                file,
                target_keyword,
                keyword_difficulty,
                target_volume,
                published_date,
                word_count,
                status,
                review_status,
                review_started_at,
                last_reviewed_at,
                review_count,
                gaps_str,
                estimated_traffic,
                page_type,
            )| {
                let gaps: Value = serde_json::from_str(&gaps_str).unwrap_or(Value::Array(vec![]));
                let mut article = serde_json::json!({
                    "id": id,
                    "title": title,
                    "url_slug": url_slug,
                    "file": file,
                    "target_keyword": target_keyword.unwrap_or_default(),
                    "keyword_difficulty": keyword_difficulty.unwrap_or_default(),
                    "target_volume": target_volume,
                    "published_date": published_date.unwrap_or_default(),
                    "word_count": word_count,
                    "status": status,
                    "content_gaps_addressed": gaps,
                    "estimated_traffic_monthly": estimated_traffic.unwrap_or_default(),
                });
                if let Some(page_type) = page_type.filter(|s| !s.is_empty()) {
                    article["page_type"] = Value::String(page_type);
                }
                if let Some(review_status) = review_status.filter(|s| !s.is_empty()) {
                    article["review_status"] = Value::String(review_status);
                }
                if let Some(review_started_at) = review_started_at.filter(|s| !s.is_empty()) {
                    article["review_started_at"] = Value::String(review_started_at);
                }
                if let Some(last_reviewed_at) = last_reviewed_at.filter(|s| !s.is_empty()) {
                    article["last_reviewed_at"] = Value::String(last_reviewed_at);
                }
                if review_count > 0 {
                    article["review_count"] = Value::from(review_count);
                }
                article
            },
        )
        .collect();

    let output = ArticlesJson {
        next_article_id,
        articles,
    };
    Ok(serde_json::to_string_pretty(&output)?)
}

/// Canonical location of articles.json within a repo: .github/automation/articles.json
fn articles_json_path(project_path: &Path) -> std::path::PathBuf {
    project_path
        .join(".github")
        .join("automation")
        .join("articles.json")
}

fn validate_export_date_policy(conn: &Connection, project_id: &str) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT id, status, published_date FROM articles WHERE project_id = ?1 ORDER BY id ASC",
    )?;

    let articles: Vec<Article> = stmt
        .query_map([project_id], |row| {
            let id: i64 = row.get(0)?;
            let status: String = row.get(1)?;
            let published_date: Option<String> = row.get(2)?;
            Ok(Article {
                id,
                title: String::new(),
                url_slug: String::new(),
                file: String::new(),
                target_keyword: None,
                keyword_difficulty: None,
                target_volume: 0,
                published_date,
                word_count: 0,
                status,
                review_status: None,
                review_started_at: None,
                last_reviewed_at: None,
                review_count: 0,
                content_gaps_addressed: vec![],
                estimated_traffic_monthly: None,
                page_type: None,
                project_id: project_id.to_string(),
                quality_score: None,
                quality_grade: None,
                quality_rated_at: None,
                publishing_ready: None,
                quality_breakdown: None,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let report = crate::content::date_policy::validate_no_future_dates(&articles);
    if report.is_valid() {
        return Ok(());
    }

    let detail = report
        .issues
        .iter()
        .take(8)
        .map(|i| format!("id {} {} ({})", i.article_id, i.description, i.current_date))
        .collect::<Vec<_>>()
        .join("; ");

    Err(Error::Other(format!(
        "Date policy check failed: {} issue(s). Future-dated articles must be corrected before export. {}",
        report.issues.len(),
        detail
    )))
}

/// Merge unknown/custom fields from an existing articles.json into a newly exported one.
/// Preserves fields not in the SQLite schema (e.g. gsc, analytics) across export rounds.
pub(crate) fn merge_unknown_fields(exported: &mut serde_json::Value, existing: &serde_json::Value) {
    let Some(exported_articles) = exported.get_mut("articles").and_then(|v| v.as_array_mut())
    else {
        return;
    };
    let Some(existing_articles) = existing.get("articles").and_then(|v| v.as_array()) else {
        return;
    };

    let existing_map: std::collections::HashMap<i64, &serde_json::Map<String, serde_json::Value>> =
        existing_articles
            .iter()
            .filter_map(|a| {
                let id = a.get("id").and_then(|v| v.as_i64())?;
                let obj = a.as_object()?;
                Some((id, obj))
            })
            .collect();

    let known_fields: std::collections::HashSet<&str> = [
        "id",
        "title",
        "url_slug",
        "file",
        "target_keyword",
        "keyword_difficulty",
        "target_volume",
        "published_date",
        "word_count",
        "status",
        "content_gaps_addressed",
        "estimated_traffic_monthly",
        "review_status",
        "review_started_at",
        "last_reviewed_at",
        "review_count",
        "page_type",
    ]
    .iter()
    .copied()
    .collect();

    for article in exported_articles.iter_mut() {
        let Some(id) = article.get("id").and_then(|v| v.as_i64()) else {
            continue;
        };
        let Some(existing_obj) = existing_map.get(&id) else {
            continue;
        };
        let Some(article_obj) = article.as_object_mut() else {
            continue;
        };

        for (key, value) in existing_obj.iter() {
            if !known_fields.contains(key.as_str()) {
                article_obj.insert(key.clone(), value.clone());
            }
        }
    }
}

/// Merge sidecar metadata from the `article_metadata` table into the exported JSON.
/// Each namespace payload (a JSON object) is flattened into the article object.
pub(crate) fn merge_sidecar_metadata(
    conn: &Connection,
    project_id: &str,
    exported: &mut serde_json::Value,
) -> Result<()> {
    let meta_rows = crate::db::list_project_metadata(conn, project_id)?;
    let Some(articles) = exported.get_mut("articles").and_then(|v| v.as_array_mut()) else {
        return Ok(());
    };

    for article in articles.iter_mut() {
        let Some(article_id) = article.get("id").and_then(|v| v.as_i64()) else {
            continue;
        };
        for (id, namespace, payload) in &meta_rows {
            if *id != article_id {
                continue;
            }
            if let Ok(obj) = serde_json::from_str::<serde_json::Value>(payload) {
                if let Some(obj_map) = obj.as_object() {
                    for (key, value) in obj_map {
                        // Namespace acts as the top-level key (e.g. "gsc")
                        if namespace == key {
                            article[key] = value.clone();
                        } else {
                            // Flatten nested keys under the namespace key
                            if !article
                                .get(namespace)
                                .map(|v| v.is_object())
                                .unwrap_or(false)
                            {
                                article[namespace] = serde_json::Value::Object(Default::default());
                            }
                            if let Some(ns_obj) =
                                article.get_mut(namespace).and_then(|v| v.as_object_mut())
                            {
                                ns_obj.insert(key.clone(), value.clone());
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Write articles.json to .github/automation/articles.json (CLI-compatible location).
pub fn write_articles_to_repo(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
) -> Result<()> {
    validate_export_date_policy(conn, project_id)?;
    let mut exported: serde_json::Value =
        serde_json::from_str(&export_articles(conn, project_id)?)?;

    let out_path = articles_json_path(project_path);
    if let Ok(existing_json) = std::fs::read_to_string(&out_path) {
        if let Ok(existing) = serde_json::from_str::<serde_json::Value>(&existing_json) {
            merge_unknown_fields(&mut exported, &existing);
        }
    }

    // Merge sidecar metadata (e.g. GSC metrics) from SQLite into the export.
    // This MUST run after merge_unknown_fields so fresh SQLite data wins
    // over stale null values from the old articles.json on disk.
    merge_sidecar_metadata(conn, project_id, &mut exported)?;

    std::fs::create_dir_all(out_path.parent().unwrap())?;
    std::fs::write(out_path, serde_json::to_string_pretty(&exported)?)?;
    Ok(())
}

/// Read articles.json from .github/automation/articles.json and import.
pub fn read_articles_from_repo(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
) -> Result<usize> {
    let articles_path = articles_json_path(project_path);
    if !articles_path.exists() {
        return Ok(0);
    }
    let json = std::fs::read_to_string(&articles_path)?;
    import_articles(conn, project_id, &json)
}

/// Try to import from existing projects.json at ~/.config/automation/projects.json.
/// Returns a list of (id, name, path) tuples for detected projects.
pub fn import_projects_config(conn: &Connection) -> Result<Vec<(String, String, String)>> {
    let config_path = dirs::home_dir()
        .ok_or_else(|| Error::Other("Cannot determine home directory".into()))?
        .join(".config")
        .join("automation")
        .join("projects.json");

    if !config_path.exists() {
        return Ok(vec![]);
    }

    let json = std::fs::read_to_string(&config_path)?;
    let parsed: Value = serde_json::from_str(&json)?;

    // Format: { "projects": [ { "website_id", "name", "repo_root", "content_dir" }, ... ] }
    let projects_arr = match parsed["projects"].as_array() {
        Some(a) => a,
        None => return Ok(vec![]),
    };

    let mut imported = vec![];
    for project in projects_arr {
        // id comes from website_id field
        let id = match project["website_id"].as_str() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };
        let name = project["name"].as_str().unwrap_or(&id).to_string();
        // path comes from repo_root
        let path = match project["repo_root"].as_str() {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => continue,
        };
        // content_dir: treat empty string as NULL
        let content_dir = project["content_dir"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(String::from);

        conn.execute(
            "INSERT INTO projects (id, name, path, content_dir, site_id, active)
             VALUES (?1, ?2, ?3, ?4, ?5, 1)
             ON CONFLICT(id) DO UPDATE SET
                name        = excluded.name,
                path        = excluded.path,
                content_dir = COALESCE(excluded.content_dir, projects.content_dir),
                site_id     = excluded.site_id",
            rusqlite::params![id, name, path, content_dir, id],
        )?;
        imported.push((id, name, path));
    }
    Ok(imported)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
    }

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn insert_test_project(conn: &Connection, id: &str, path: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES (?1, ?2, ?3, 1, 'workspace')",
            [id, "Test Project", path],
        )
        .unwrap();
    }

    #[test]
    fn import_export_round_trip_preserves_known_fields() {
        let conn = in_memory_db();
        insert_test_project(&conn, "p1", "/tmp/test");

        let json = r#"{
            "nextArticleId": 3,
            "articles": [
                {
                    "id": 1,
                    "title": "Hello",
                    "url_slug": "hello",
                    "file": "./content/001_hello.mdx",
                    "target_keyword": "hello world",
                    "keyword_difficulty": "15",
                    "target_volume": 1200,
                    "published_date": "2026-01-15",
                    "word_count": 450,
                    "status": "published",
                    "review_status": "reviewed",
                    "review_started_at": "2026-01-10T00:00:00Z",
                    "last_reviewed_at": "2026-01-12T00:00:00Z",
                    "review_count": 2,
                    "content_gaps_addressed": ["gap1"],
                    "estimated_traffic_monthly": "100"
                }
            ]
        }"#;

        let imported = import_articles(&conn, "p1", json).unwrap();
        assert_eq!(imported, 1);

        let exported = export_articles(&conn, "p1").unwrap();
        let doc: serde_json::Value = serde_json::from_str(&exported).unwrap();
        assert_eq!(doc["nextArticleId"], 3);

        let articles = doc["articles"].as_array().unwrap();
        assert_eq!(articles.len(), 1);

        let a = &articles[0];
        assert_eq!(a["id"], 1);
        assert_eq!(a["title"], "Hello");
        assert_eq!(a["url_slug"], "hello");
        assert_eq!(a["file"], "./content/001_hello.mdx");
        assert_eq!(a["target_keyword"], "hello world");
        assert_eq!(a["keyword_difficulty"], "15");
        assert_eq!(a["target_volume"], 1200);
        assert_eq!(a["published_date"], "2026-01-15");
        assert_eq!(a["word_count"], 450);
        assert_eq!(a["status"], "published");
        assert_eq!(a["review_status"], "reviewed");
        assert_eq!(a["review_started_at"], "2026-01-10T00:00:00Z");
        assert_eq!(a["last_reviewed_at"], "2026-01-12T00:00:00Z");
        assert_eq!(a["review_count"], 2);
        assert_eq!(a["estimated_traffic_monthly"], "100");
    }

    #[test]
    fn export_preserves_unknown_fields_via_merge() {
        let conn = in_memory_db();
        insert_test_project(&conn, "p1", "/tmp/test");

        let json = r#"{
            "nextArticleId": 2,
            "articles": [
                {
                    "id": 1,
                    "title": "Hello",
                    "url_slug": "hello",
                    "file": "./content/001_hello.mdx",
                    "status": "draft",
                    "gsc": { "clicks": 42, "impressions": 1000 }
                }
            ]
        }"#;

        import_articles(&conn, "p1", json).unwrap();

        // Simulate an export round-trip by writing to disk and reading back
        let dir = unique_temp_dir("ps_export_test");
        let auto_dir = dir.join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();
        std::fs::write(auto_dir.join("articles.json"), json).unwrap();

        write_articles_to_repo(&conn, "p1", &dir).unwrap();

        let on_disk = std::fs::read_to_string(auto_dir.join("articles.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
        let a = &doc["articles"].as_array().unwrap()[0];

        // Known fields come from SQLite
        assert_eq!(a["title"], "Hello");
        // Unknown field preserved from existing JSON
        assert_eq!(a["gsc"]["clicks"], 42);
        assert_eq!(a["gsc"]["impressions"], 1000);
    }

    #[test]
    fn import_does_not_destroy_unknown_fields_in_existing_json() {
        let conn = in_memory_db();
        insert_test_project(&conn, "p1", "/tmp/test");

        let initial_json = r#"{
            "nextArticleId": 2,
            "articles": [
                {
                    "id": 1,
                    "title": "Hello",
                    "url_slug": "hello",
                    "file": "./content/001_hello.mdx",
                    "status": "draft",
                    "custom_metric": 99
                }
            ]
        }"#;

        import_articles(&conn, "p1", initial_json).unwrap();

        // After export, the unknown field should still be present if we merge
        let exported = export_articles(&conn, "p1").unwrap();
        let mut exported_doc: serde_json::Value = serde_json::from_str(&exported).unwrap();
        let existing_doc: serde_json::Value = serde_json::from_str(initial_json).unwrap();

        merge_unknown_fields(&mut exported_doc, &existing_doc);

        let a = &exported_doc["articles"].as_array().unwrap()[0];
        assert_eq!(a["custom_metric"], 99);
    }

    #[test]
    fn read_write_articles_from_repo_round_trip() {
        let conn = in_memory_db();
        let dir = unique_temp_dir("ps_repo_roundtrip");
        let auto_dir = dir.join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();

        insert_test_project(&conn, "p1", dir.to_str().unwrap());

        let json = r#"{
            "nextArticleId": 2,
            "articles": [
                {
                    "id": 1,
                    "title": "Round Trip",
                    "url_slug": "round-trip",
                    "file": "./content/001_round_trip.mdx",
                    "status": "published",
                    "published_date": "2026-02-01"
                }
            ]
        }"#;

        std::fs::write(auto_dir.join("articles.json"), json).unwrap();

        let imported = read_articles_from_repo(&conn, "p1", &dir).unwrap();
        assert_eq!(imported, 1);

        write_articles_to_repo(&conn, "p1", &dir).unwrap();

        let on_disk = std::fs::read_to_string(auto_dir.join("articles.json")).unwrap();
        let doc: serde_json::Value = serde_json::from_str(&on_disk).unwrap();
        assert_eq!(doc["nextArticleId"], 2);
        let a = &doc["articles"].as_array().unwrap()[0];
        assert_eq!(a["title"], "Round Trip");
        assert_eq!(a["published_date"], "2026-02-01");
    }

    #[test]
    fn import_articles_updates_meta_next_article_id() {
        let conn = in_memory_db();
        insert_test_project(&conn, "p1", "/tmp/test");

        let json = r#"{"nextArticleId": 42, "articles": []}"#;
        import_articles(&conn, "p1", json).unwrap();

        let next: i64 = conn
            .query_row(
                "SELECT next_article_id FROM articles_meta WHERE project_id = 'p1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(next, 42);
    }
}
