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
        let execution_mode = t["execution_mode"].as_str().unwrap_or("manual").to_string();
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
                title, description, project_id, depends_on, artifacts,
                run_attempts, run_last_error, run_provider, created_at, updated_at
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
             ON CONFLICT(id) DO UPDATE SET
                status          = excluded.status,
                priority        = excluded.priority,
                title           = excluded.title,
                description     = excluded.description,
                depends_on      = excluded.depends_on,
                artifacts       = excluded.artifacts,
                run_attempts    = excluded.run_attempts,
                run_last_error  = excluded.run_last_error,
                run_provider    = excluded.run_provider,
                updated_at      = excluded.updated_at",
            rusqlite::params![
                id,
                task_type,
                phase,
                status,
                priority,
                execution_mode,
                agent_policy,
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
        "SELECT id, type, phase, status, priority, execution_mode, agent_policy,
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
            let execution_mode: String = row.get(5)?;
            let agent_policy: String = row.get(6)?;
            let title: Option<String> = row.get(7)?;
            let description: Option<String> = row.get(8)?;
            let depends_on_str: String = row.get(9)?;
            let artifacts_str: String = row.get(10)?;
            let run_attempts: i64 = row.get(11)?;
            let run_last_error: Option<String> = row.get(12)?;
            let run_provider: Option<String> = row.get(13)?;
            Ok((
                id,
                task_type,
                phase,
                status,
                priority,
                execution_mode,
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
                execution_mode,
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
                    "execution_mode": execution_mode,
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
        let gaps =
            serde_json::to_string(&a["content_gaps_addressed"]).unwrap_or_else(|_| "[]".into());
        let estimated_traffic = a["estimated_traffic_monthly"].as_str().map(String::from);

        conn.execute(
            "INSERT INTO articles (
                id, title, url_slug, file, target_keyword, keyword_difficulty,
                target_volume, published_date, word_count, status,
                content_gaps_addressed, estimated_traffic_monthly, project_id
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
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
                content_gaps_addressed      = excluded.content_gaps_addressed,
                estimated_traffic_monthly   = excluded.estimated_traffic_monthly",
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
                gaps,
                estimated_traffic,
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
                content_gaps_addressed, estimated_traffic_monthly
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
            let gaps_str: String = row.get(10)?;
            let estimated_traffic: Option<String> = row.get(11)?;
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
                gaps_str,
                estimated_traffic,
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
                gaps_str,
                estimated_traffic,
            )| {
                let gaps: Value =
                    serde_json::from_str(&gaps_str).unwrap_or(Value::Array(vec![]));
                serde_json::json!({
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
                })
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
    project_path.join(".github").join("automation").join("articles.json")
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
                content_gaps_addressed: vec![],
                estimated_traffic_monthly: None,
                project_id: project_id.to_string(),
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

/// Write articles.json to .github/automation/articles.json (CLI-compatible location).
pub fn write_articles_to_repo(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
) -> Result<()> {
    validate_export_date_policy(conn, project_id)?;
    let json = export_articles(conn, project_id)?;
    let out_path = articles_json_path(project_path);
    std::fs::create_dir_all(out_path.parent().unwrap())?;
    std::fs::write(out_path, json)?;
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
