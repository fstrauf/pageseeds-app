use crate::engine::project_paths::ProjectPaths;
use crate::engine::skills;
use crate::engine::workflows::StepResult;
use crate::models::cannibalization::{TerritoryRecommendation, TerritoryStrategy};
use crate::models::task::{
    AgentPolicy, FollowUpPolicy, Priority, Task, TaskArtifact, TaskReviewSurface,
};
/// Territory research execution module.
///
/// Covers the 4-step territory_research pipeline:
///   1. territory_load_recommendation  — read approved territory recommendation
///   2. territory_build_context        — gather existing articles, excerpts, metrics
///   3. territory_strategy             — agentic: generate TerritoryStrategy via skill
///   4. territory_apply                — write strategy JSON to automation dir
use rusqlite::Connection;
use serde_json::Value;
use std::path::Path;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 1: Load Recommendation
// ═══════════════════════════════════════════════════════════════════════════════

pub(crate) fn exec_territory_load_recommendation(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Extract theme from task title: "Research territory: {theme}"
    let theme = task
        .title
        .as_deref()
        .and_then(|t| t.strip_prefix("Research territory:"))
        .unwrap_or("")
        .trim();

    if theme.is_empty() {
        return StepResult {
            success: false,
            message: "Cannot determine territory theme from task title".to_string(),
            output: None,
        };
    }

    let strategy_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "cannibalization_strategy")
        .and_then(|a| a.content.clone())
        .unwrap_or_default();

    let strategy_json = if strategy_json.is_empty() {
        let path = paths.automation_dir.join("cannibalization_strategy.json");
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        strategy_json
    };

    if strategy_json.is_empty() {
        return StepResult {
            success: false,
            message: "No cannibalization_strategy artifact found".to_string(),
            output: None,
        };
    }

    let strategy: serde_json::Value = match serde_json::from_str(&strategy_json) {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid strategy JSON: {}", e),
                output: None,
            };
        }
    };

    let empty: Vec<serde_json::Value> = vec![];
    let recommendations = strategy["territory_recommendations"]
        .as_array()
        .unwrap_or(&empty);
    let rec = recommendations.iter().find(|r| {
        r["theme"]
            .as_str()
            .map(|t| t.trim().eq_ignore_ascii_case(theme))
            .unwrap_or(false)
    });

    let rec = match rec {
        Some(r) => r.clone(),
        None => {
            return StepResult {
                success: false,
                message: format!("No territory recommendation found matching '{}'", theme),
                output: None,
            };
        }
    };

    let out_path = paths
        .automation_dir
        .join(format!("territory_recommendation_{}.json", task.id));
    if let Err(e) = std::fs::write(
        &out_path,
        serde_json::to_string_pretty(&rec).unwrap_or_default(),
    ) {
        log::warn!(
            "[territory_load_recommendation] failed to write {}: {}",
            out_path.display(),
            e
        );
    }

    let json = serde_json::to_string_pretty(&rec).unwrap_or_default();
    StepResult {
        success: true,
        message: format!("Loaded territory recommendation for theme: {}", theme),
        output: Some(json),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 2: Build Context
// ═══════════════════════════════════════════════════════════════════════════════

pub(crate) fn exec_territory_build_context(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let rec_path = paths
        .automation_dir
        .join(format!("territory_recommendation_{}.json", task.id));
    let rec_json = match std::fs::read_to_string(&rec_path) {
        Ok(s) => s,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Cannot read territory recommendation: {}", e),
                output: None,
            };
        }
    };

    let rec: TerritoryRecommendation = match serde_json::from_str(&rec_json) {
        Ok(r) => r,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid territory recommendation JSON: {}", e),
                output: None,
            };
        }
    };

    let db_path = crate::db::default_db_path();
    let conn = match Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to open DB: {}", e),
                output: None,
            };
        }
    };

    let theme_lower = rec.theme.to_lowercase();
    let articles = match gather_matching_articles(&conn, &task.project_id, &theme_lower) {
        Ok(a) => a,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to query articles: {}", e),
                output: None,
            };
        }
    };

    let context = serde_json::json!({
        "theme": rec.theme,
        "priority": rec.priority,
        "demand_evidence": rec.demand_evidence,
        "existing_articles": articles,
    });

    let context_json = match serde_json::to_string_pretty(&context) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize context: {}", e),
                output: None,
            };
        }
    };

    StepResult {
        success: true,
        message: format!(
            "Built territory context with {} existing articles",
            articles.as_array().map(|a| a.len()).unwrap_or(0)
        ),
        output: Some(context_json),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Agentic Strategy
// ═══════════════════════════════════════════════════════════════════════════════

/// Agentic: generate a TerritoryStrategy JSON from clustered article context.
///
/// Why not deterministic: territory strategy requires weighing trade-offs between
/// competitor gaps, existing coverage, content opportunity, and business priority.
/// A deterministic ruleset cannot judge which gaps are worth filling or how to
/// prioritize recommendations — this requires semantic understanding of the domain,
/// search intent, and content quality. The output is a structured TerritoryStrategy
/// with typed recommendations, extracted via Rig's `extract_structured`.
pub(crate) async fn exec_territory_strategy(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: &str,
) -> StepResult {
    let repo_root = Path::new(project_path);

    let skill = match skills::load_skill(repo_root, "territory-strategy") {
        Some(s) => s,
        None => {
            return StepResult {
                success: false,
                message: "Skill 'territory-strategy' not found".to_string(),
                output: None,
            };
        }
    };

    let prompt = skill.content
        + "\n\n---\n\n## Territory Context\n\n"
        + context_json
        + "\n\nGenerate a structured TerritoryStrategy by calling the submit tool.";

    let preamble = "You are an expert SEO strategist. Analyze the territory context and generate a structured strategy using the submit tool.";

    log::info!(
        "[territory_strategy] running structured extraction ({} chars prompt, provider={})",
        prompt.len(),
        agent_provider
    );

    match crate::rig::extraction::extract_structured::<TerritoryStrategy>(
        agent_provider,
        &prompt,
        Some(preamble),
        Some("direct"),
    )
    .await
    {
        Ok(strategy) => {
            let strategy_json = match serde_json::to_string_pretty(&strategy) {
                Ok(j) => j,
                Err(e) => {
                    return StepResult {
                        success: false,
                        message: format!("Failed to serialize strategy: {}", e),
                        output: None,
                    };
                }
            };
            StepResult {
                success: true,
                message: format!(
                    "Territory strategy complete: {} recommendations for '{}'",
                    strategy.content_recommendations.len(),
                    strategy.theme
                ),
                output: Some(strategy_json),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Structured extraction failed: {}", e),
            output: None,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 4: Apply
// ═══════════════════════════════════════════════════════════════════════════════

pub(crate) fn exec_territory_apply(
    task: &Task,
    project_path: &str,
    strategy_json: &str,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let (strategy, normalized_json, warnings) = match parse_territory_strategy(strategy_json) {
        Ok(parsed) => parsed,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid territory strategy JSON: {}", e),
                output: None,
            };
        }
    };

    let out_path = paths
        .automation_dir
        .join(format!("territory_strategy_{}.json", task.id));
    if let Err(e) = std::fs::write(&out_path, &normalized_json) {
        return StepResult {
            success: false,
            message: format!("Failed to write territory strategy: {}", e),
            output: None,
        };
    }

    let warning_suffix = if warnings.is_empty() {
        String::new()
    } else {
        format!(" (normalized: {})", warnings.join(", "))
    };

    StepResult {
        success: true,
        message: format!(
            "Territory strategy applied: {} recommendations for '{}'{}",
            strategy.content_recommendations.len(),
            strategy.theme,
            warning_suffix
        ),
        output: Some(normalized_json),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

fn gather_matching_articles(
    conn: &Connection,
    project_id: &str,
    theme_lower: &str,
) -> Result<serde_json::Value, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, title, url_slug, target_keyword, file FROM articles WHERE project_id = ?1",
    )?;

    let rows = stmt.query_map([project_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
        ))
    })?;

    let mut articles = Vec::new();
    for row in rows {
        let (id, title, url_slug, target_keyword, file) = row?;
        let kw = target_keyword.as_deref().unwrap_or("").to_lowercase();
        let title_lower = title.to_lowercase();
        if kw.contains(theme_lower)
            || title_lower.contains(theme_lower)
            || url_slug.to_lowercase().contains(theme_lower)
        {
            let excerpt = read_excerpt_from_file(&file);
            articles.push(serde_json::json!({
                "article_id": id,
                "title": title,
                "url_slug": url_slug,
                "target_keyword": target_keyword,
                "excerpt": excerpt,
            }));
        }
    }

    Ok(serde_json::Value::Array(articles))
}

fn parse_territory_strategy(
    strategy_json: &str,
) -> Result<(TerritoryStrategy, String, Vec<String>), String> {
    match serde_json::from_str::<TerritoryStrategy>(strategy_json) {
        Ok(strategy) => {
            let normalized_json = serde_json::to_string_pretty(&strategy)
                .map_err(|e| format!("failed to serialize typed strategy: {}", e))?;
            Ok((strategy, normalized_json, Vec::new()))
        }
        Err(initial_error) => {
            let mut value: Value = serde_json::from_str(strategy_json).map_err(|e| {
                format!(
                    "{}; also failed to parse as JSON value: {}",
                    initial_error, e
                )
            })?;
            let warnings = normalize_territory_strategy_value(&mut value);
            let strategy: TerritoryStrategy = serde_json::from_value(value).map_err(|e| {
                format!(
                    "{}; normalization could not produce TerritoryStrategy: {}",
                    initial_error, e
                )
            })?;
            let normalized_json = serde_json::to_string_pretty(&strategy)
                .map_err(|e| format!("failed to serialize normalized strategy: {}", e))?;
            Ok((strategy, normalized_json, warnings))
        }
    }
}

fn normalize_territory_strategy_value(value: &mut Value) -> Vec<String> {
    let mut warnings = Vec::new();
    let Some(object) = value.as_object_mut() else {
        return warnings;
    };

    normalize_string_array_field(object, "target_keywords", &mut warnings);
    normalize_string_array_field(object, "competitor_gaps", &mut warnings);
    normalize_object_array_field(object, "content_recommendations", &mut warnings);
    normalize_object_array_field(object, "existing_coverage", &mut warnings);

    warnings
}

fn normalize_string_array_field(
    object: &mut serde_json::Map<String, Value>,
    field: &str,
    warnings: &mut Vec<String>,
) {
    match object.get_mut(field) {
        Some(Value::Null) | None => {
            object.insert(field.to_string(), Value::Array(Vec::new()));
            warnings.push(format!("{} null/missing to []", field));
        }
        Some(Value::Array(_)) => {}
        Some(Value::Object(map)) => {
            let items = map
                .values()
                .filter_map(|value| value.as_str().map(|s| Value::String(s.to_string())))
                .collect::<Vec<_>>();
            if !items.is_empty() {
                *object.get_mut(field).expect("field exists") = Value::Array(items);
                warnings.push(format!("{} object values to array", field));
            }
        }
        Some(_) => {}
    }
}

fn normalize_object_array_field(
    object: &mut serde_json::Map<String, Value>,
    field: &str,
    warnings: &mut Vec<String>,
) {
    match object.get_mut(field) {
        Some(Value::Null) | None => {
            object.insert(field.to_string(), Value::Array(Vec::new()));
            warnings.push(format!("{} null/missing to []", field));
        }
        Some(Value::Array(_)) => {}
        Some(Value::Object(map)) => {
            let items = if looks_like_object_array_item(map) {
                vec![Value::Object(map.clone())]
            } else {
                map.values()
                    .filter_map(|value| match value {
                        Value::Object(item) if looks_like_object_array_item(item) => {
                            Some(Value::Object(item.clone()))
                        }
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };

            *object.get_mut(field).expect("field exists") = Value::Array(items);
            warnings.push(format!("{} object to array", field));
        }
        Some(_) => {}
    }
}

fn looks_like_object_array_item(map: &serde_json::Map<String, Value>) -> bool {
    map.contains_key("title") || map.contains_key("url_slug") || map.contains_key("article_id")
}

fn read_excerpt_from_file(file: &str) -> String {
    if file.is_empty() {
        return String::new();
    }
    let path = Path::new(file);
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };
    let body = crate::content::frontmatter::split_mdx(&content)
        .map(|(_, b)| b)
        .unwrap_or(&content);
    let text: String = body
        .lines()
        .filter(|l| !l.trim().starts_with("```") && !l.trim().starts_with("---"))
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned = text.replace('*', "").replace('_', "").replace('#', "");
    if cleaned.chars().count() > 200 {
        let mut excerpt = String::new();
        let mut count = 0;
        for ch in cleaned.chars() {
            if count >= 200 {
                excerpt.push('…');
                break;
            }
            excerpt.push(ch);
            count += 1;
        }
        excerpt
    } else {
        cleaned
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Post-task: Spawn write_article tasks from territory strategy
// ═══════════════════════════════════════════════════════════════════════════════

/// Spawn `write_article` tasks for each content recommendation in a completed
/// territory research task.
///
/// Reads the territory strategy from the task artifact or from disk, then
/// creates focused `write_article` tasks with `territory_brief` artifacts.
/// Uses `SkipIfAnyExists` dedup to avoid duplicates.
pub(crate) fn create_territory_write_tasks(
    conn: &Connection,
    parent_task: &Task,
    project_path: &str,
) -> Vec<String> {
    let mut spawned_ids = Vec::new();

    // 1. Try task artifact first, then fall back to disk
    let strategy_json = parent_task
        .artifacts
        .iter()
        .find(|a| a.key == "territory_strategy")
        .and_then(|a| a.content.clone())
        .or_else(|| {
            let paths = ProjectPaths::from_path(project_path);
            let path = paths
                .automation_dir
                .join(format!("territory_strategy_{}.json", parent_task.id));
            std::fs::read_to_string(&path).ok()
        });

    let strategy_json = match strategy_json {
        Some(j) => j,
        None => {
            log::warn!(
                "[territory_post_task] No territory strategy found for task {}",
                parent_task.id
            );
            return spawned_ids;
        }
    };

    let strategy: TerritoryStrategy = match serde_json::from_str(&strategy_json) {
        Ok(s) => s,
        Err(e) => {
            log::warn!(
                "[territory_post_task] Failed to parse territory strategy for task {}: {}",
                parent_task.id,
                e
            );
            return spawned_ids;
        }
    };

    for rec in &strategy.content_recommendations {
        let idempotency_key = format!(
            "territory_write:{}:{}:{}",
            parent_task.project_id, parent_task.id, rec.url_slug
        );

        let brief = serde_json::json!({
            "title": rec.title,
            "url_slug": rec.url_slug,
            "intent": rec.intent,
            "rationale": rec.rationale,
            "territory_theme": strategy.theme,
            "priority": strategy.priority,
        });

        let artifact = TaskArtifact {
            key: "territory_brief".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some("territory_research".to_string()),
            content: Some(brief.to_string()),
        };

        let spec = crate::engine::spawner::TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: "write_article".to_string(),
            title: Some(format!("Write territory article: {}", rec.title)),
            description: Some(format!(
                "Territory article for '{}'. Intent: {}. Rationale: {}",
                strategy.theme, rec.intent, rec.rationale
            )),
            priority: Priority::Medium,
            agent_policy: AgentPolicy::Required,
            depends_on: vec![parent_task.id.clone()],
            artifacts: vec![artifact],
            idempotency_key: Some(idempotency_key),
            dedup_policy: Some(crate::engine::spawner::DeduplicationPolicy::SkipIfAnyExists),
            ..Default::default()
        };

        match crate::engine::spawner::TaskSpawner::spawn(conn, spec) {
            Ok(task) => {
                log::info!(
                    "[territory_post_task] Spawned write_article {} for '{}' (territory: {})",
                    task.id,
                    rec.title,
                    strategy.theme
                );
                spawned_ids.push(task.id);
            }
            Err(e) => {
                log::warn!(
                    "[territory_post_task] Failed to spawn write_article for '{}': {}",
                    rec.title,
                    e
                );
            }
        }
    }

    spawned_ids
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    struct TempProjectDir {
        path: std::path::PathBuf,
    }

    impl TempProjectDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("pageseeds-territory-test-{}", Uuid::new_v4()));
            fs::create_dir_all(path.join(".github").join("automation")).unwrap();
            fs::create_dir_all(path.join("content")).unwrap();
            Self { path }
        }

        fn path(&self) -> &std::path::Path {
            &self.path
        }
    }

    impl Drop for TempProjectDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn file_db(path: &std::path::Path) -> Connection {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE projects (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                path TEXT NOT NULL,
                active INTEGER DEFAULT 1
            );
            CREATE TABLE articles (
                id INTEGER NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                url_slug TEXT NOT NULL DEFAULT '',
                file TEXT NOT NULL DEFAULT '',
                target_keyword TEXT,
                keyword_difficulty TEXT,
                target_volume INTEGER DEFAULT 0,
                published_date TEXT,
                word_count INTEGER DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'draft',
                review_status TEXT,
                review_started_at TEXT,
                last_reviewed_at TEXT,
                review_count INTEGER NOT NULL DEFAULT 0,
                content_gaps_addressed TEXT NOT NULL DEFAULT '[]',
                estimated_traffic_monthly TEXT,
                project_id TEXT NOT NULL,
                PRIMARY KEY (id, project_id)
            );",
        )
        .unwrap();
        conn
    }

    fn create_test_project(conn: &Connection, id: &str, path: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES (?1, ?2, ?3, 1)",
            rusqlite::params![id, "Test Project", path],
        )
        .unwrap();
    }

    fn insert_test_article(
        conn: &Connection,
        project_id: &str,
        id: i64,
        title: &str,
        slug: &str,
        keyword: &str,
        file: &str,
    ) {
        conn.execute(
            "INSERT INTO articles (id, title, url_slug, file, target_keyword, status, content_gaps_addressed, project_id)
             VALUES (?1, ?2, ?3, ?4, ?5, 'published', '[]', ?6)",
            rusqlite::params![id, title, slug, file, keyword, project_id],
        )
        .unwrap();
    }

    #[test]
    fn test_territory_load_recommendation_from_artifact() {
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();

        let strategy = serde_json::json!({
            "territory_recommendations": [
                {
                    "theme": "Dividend Investing",
                    "priority": "high",
                    "demand_evidence": ["1000 impressions"],
                    "suggested_tasks": ["Write guide"]
                }
            ]
        });

        let task = Task {
            id: "task-tr-1".to_string(),
            project_id: "proj1".to_string(),
            task_type: "territory_research".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Research territory: Dividend Investing".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "cannibalization_strategy".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("cannibalization_audit".to_string()),
                content: Some(strategy.to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_territory_load_recommendation(&task, &project_path);
        assert!(result.success, "Expected success: {}", result.message);
        assert!(result.output.is_some());
        let out: serde_json::Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert_eq!(out["theme"], "Dividend Investing");
    }

    #[test]
    fn test_territory_build_context_finds_articles() {
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();
        let db_path = dir.path().join("test.db");
        {
            let conn = file_db(&db_path);
            std::env::set_var("PAGESEEDS_DB_PATH", &db_path);
            create_test_project(&conn, "proj1", &project_path);
            insert_test_article(
                &conn,
                "proj1",
                1,
                "Dividend Basics",
                "dividend-basics",
                "dividend investing",
                "./content/001_basics.mdx",
            );
            insert_test_article(
                &conn,
                "proj1",
                2,
                "Growth Stocks",
                "growth-stocks",
                "growth investing",
                "./content/002_growth.mdx",
            );
        }

        let rec = serde_json::json!({
            "theme": "Dividend Investing",
            "priority": "high",
            "demand_evidence": [],
            "suggested_tasks": []
        });
        let rec_path = dir
            .path()
            .join(".github")
            .join("automation")
            .join("territory_recommendation_task-tr-2.json");
        fs::write(&rec_path, rec.to_string()).unwrap();

        let task = Task {
            id: "task-tr-2".to_string(),
            project_id: "proj1".to_string(),
            task_type: "territory_research".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Research territory: Dividend Investing".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_territory_build_context(&task, &project_path);
        assert!(result.success, "Expected success: {}", result.message);

        let context: serde_json::Value = serde_json::from_str(&result.output.unwrap()).unwrap();
        let articles = context["existing_articles"].as_array().unwrap();
        assert_eq!(articles.len(), 1);
        assert_eq!(articles[0]["title"], "Dividend Basics");
    }

    #[test]
    fn test_territory_apply_writes_strategy() {
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();

        let strategy = serde_json::json!({
            "theme": "Dividend Investing",
            "priority": "high",
            "target_keywords": ["dividend investing", "dividend stocks"],
            "competitor_gaps": ["tax implications"],
            "content_recommendations": [
                {
                    "title": "Dividend Tax Guide",
                    "url_slug": "dividend-tax-guide",
                    "intent": "informational",
                    "rationale": "Gap in tax coverage"
                }
            ],
            "existing_coverage": []
        });

        let task = Task {
            id: "task-tr-3".to_string(),
            project_id: "proj1".to_string(),
            task_type: "territory_research".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Research territory: Dividend Investing".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_territory_apply(&task, &project_path, &strategy.to_string());
        assert!(result.success, "Expected success: {}", result.message);

        let out_path = dir
            .path()
            .join(".github")
            .join("automation")
            .join("territory_strategy_task-tr-3.json");
        assert!(out_path.exists(), "Strategy file should be written");
    }

    #[test]
    fn test_territory_apply_normalizes_existing_coverage_summary_object() {
        let dir = TempProjectDir::new();
        let project_path = dir.path().to_string_lossy().to_string();

        let strategy = serde_json::json!({
            "theme": "coffee-health",
            "priority": "high",
            "target_keywords": ["coffee health benefits"],
            "competitor_gaps": ["No dedicated health coverage"],
            "content_recommendations": [
                {
                    "title": "The Real Health Benefits of Coffee",
                    "url_slug": "coffee-health-benefits",
                    "intent": "informational",
                    "rationale": "Fills the health gap"
                }
            ],
            "existing_coverage": {
                "summary": "No existing articles cover the coffee-health theme.",
                "overlap_risks": []
            }
        });

        let task = Task {
            id: "task-tr-4".to_string(),
            project_id: "proj1".to_string(),
            task_type: "territory_research".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::Required,
            title: Some("Research territory: coffee-health".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_territory_apply(&task, &project_path, &strategy.to_string());
        assert!(result.success, "Expected success: {}", result.message);
        assert!(result
            .message
            .contains("normalized: existing_coverage object to array"));

        let normalized: TerritoryStrategy = serde_json::from_str(&result.output.unwrap()).unwrap();
        assert!(normalized.existing_coverage.is_empty());

        let out_path = dir
            .path()
            .join(".github")
            .join("automation")
            .join("territory_strategy_task-tr-4.json");
        let written: TerritoryStrategy =
            serde_json::from_str(&fs::read_to_string(out_path).unwrap()).unwrap();
        assert!(written.existing_coverage.is_empty());
    }
}
