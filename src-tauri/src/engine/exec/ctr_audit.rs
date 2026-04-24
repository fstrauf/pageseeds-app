/// CTR (Click-Through Rate) audit execution module.
///
/// Covers:
///   - exec_ctr_build_context   (deterministic data collection + clicks_lost scoring)
///   - exec_ctr_analyze         (agentic analysis with ctr-optimization skill)
///   - create_ctr_fix_tasks     (spawn follow-up fix tasks)

use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::engine::{agent, skills};
use crate::engine::spawner::{TaskSpawner, TaskSpec};
use crate::models::task::{Task, TaskArtifact};

// ═══════════════════════════════════════════════════════════════════════════════
// Step 1: Build Context
// ═══════════════════════════════════════════════════════════════════════════════

/// Build the CTR audit context by reading articles.json, extracting excerpts,
/// computing clicks_lost per article, and returning structured JSON.
pub(crate) fn exec_ctr_build_context(_task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let articles_path = paths.automation_dir.join("articles.json");

    let raw = match std::fs::read_to_string(&articles_path) {
        Ok(s) => s,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("articles.json not found: {}", e),
                output: None,
            };
        }
    };

    let doc: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to parse articles.json: {}", e),
                output: None,
            };
        }
    };

    let empty = vec![];
    let articles = doc["articles"].as_array().unwrap_or(&empty);

    let mut article_records: Vec<serde_json::Value> = Vec::new();

    for article in articles.iter() {
        let id = article["id"].as_i64().unwrap_or(0);
        let url_slug = article["url_slug"].as_str().unwrap_or("").to_string();
        let title = article["title"].as_str().unwrap_or("").to_string();
        let target_keyword = article["target_keyword"].as_str().unwrap_or("").to_string();
        let file_ref = article["file"].as_str().unwrap_or("").to_string();

        let gsc = &article["gsc"];
        let impressions = gsc["impressions"].as_f64().unwrap_or(0.0);
        let clicks = gsc["clicks"].as_f64().unwrap_or(0.0);
        let ctr = gsc["ctr"].as_f64().unwrap_or(0.0);
        let avg_position = gsc["avg_position"].as_f64().unwrap_or(0.0);

        // Extract meta_description, first_paragraph, h1 from source file
        let (meta_description, first_paragraph, h1) =
            read_article_excerpt(project_path, &file_ref);

        // Compute clicks_lost: impressions * max(0, 0.005 - actual_ctr)
        let clicks_lost = impressions * (0.005_f64 - ctr).max(0.0);

        article_records.push(serde_json::json!({
            "id": id,
            "url_slug": url_slug,
            "title": title,
            "target_keyword": target_keyword,
            "meta_description": meta_description,
            "first_paragraph": first_paragraph,
            "h1": h1,
            "file": file_ref,
            "gsc": {
                "impressions": impressions,
                "clicks": clicks,
                "ctr": ctr,
                "avg_position": avg_position,
            },
            "clicks_lost": clicks_lost,
        }));
    }

    // Sort by clicks_lost descending
    article_records.sort_by(|a, b| {
        let ca = a["clicks_lost"].as_f64().unwrap_or(0.0);
        let cb = b["clicks_lost"].as_f64().unwrap_or(0.0);
        cb.partial_cmp(&ca).unwrap_or(std::cmp::Ordering::Equal)
    });

    let top_20: Vec<&serde_json::Value> = article_records.iter().take(20).collect();

    let now_iso = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let output_doc = serde_json::json!({
        "generated_at": now_iso,
        "total_articles": article_records.len(),
        "articles": article_records,
        "top_20_by_clicks_lost": top_20,
    });

    // Write context to automation dir for reference
    let out_path = paths.automation_dir.join("ctr_audit_context.json");
    let out_str = serde_json::to_string_pretty(&output_doc).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&out_path, &out_str) {
        log::warn!("[ctr_audit] Failed to write ctr_audit_context.json: {}", e);
    }

    StepResult {
        success: true,
        message: format!(
            "CTR context built for {} articles",
            article_records.len()
        ),
        output: Some(out_str),
    }
}

/// Read an MDX file and extract (meta_description, first_paragraph, h1).
fn read_article_excerpt(project_path: &str, file_ref: &str) -> (String, String, String) {
    if file_ref.is_empty() {
        return (String::new(), String::new(), String::new());
    }

    let repo_root = Path::new(project_path);
    let p = Path::new(file_ref);
    let full = if p.is_absolute() {
        p.to_path_buf()
    } else {
        repo_root.join(p)
    };

    let content = match std::fs::read_to_string(&full) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[ctr_audit] Could not read {}: {}", full.display(), e);
            return (String::new(), String::new(), String::new());
        }
    };

    // Use cleaner::parse_frontmatter to split frontmatter and body
    let (frontmatter_str, body) = match crate::content::cleaner::parse_frontmatter(&content) {
        Some((fm, b)) => (fm, b),
        None => ("", content.as_str()),
    };

    // Extract meta_description from frontmatter
    let meta_description = frontmatter_str
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("description:") {
                let val = rest.trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
            None
        })
        .unwrap_or_default();

    // Extract h1: first line starting with "# " in body
    let h1 = body
        .lines()
        .find(|l| {
            let t = l.trim_start();
            t.starts_with("# ") && !t.starts_with("## ")
        })
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .unwrap_or_default();

    // Extract first_paragraph: first non-empty, non-heading line
    let first_paragraph = body
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("---"))
        .unwrap_or("")
        .to_string();

    (meta_description, first_paragraph, h1)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 2: Analyze
// ═══════════════════════════════════════════════════════════════════════════════

/// Run the CTR optimization analysis using an LLM agent.
///
/// Loads the "ctr-optimization" skill, builds a prompt with the skill content
/// and the provided context JSON, and delegates to the agent.
pub(crate) fn exec_ctr_analyze(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: &str,
) -> StepResult {
    let repo_root = Path::new(project_path);

    let skill = match skills::load_skill(repo_root, "ctr-optimization") {
        Some(s) => s,
        None => {
            return StepResult {
                success: false,
                message: "Skill 'ctr-optimization' not found in .github/skills/".to_string(),
                output: None,
            };
        }
    };

    let prompt = format!(
        "{skill_content}\n\n---\n\n## CTR Audit Context\n\n{context}\n\nPlease analyze the above context and provide actionable CTR optimization recommendations.",
        skill_content = skill.content,
        context = context_json,
    );

    match agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(output) => StepResult {
            success: true,
            message: "CTR analysis completed".to_string(),
            output: Some(output),
        },
        Err(e) => StepResult {
            success: false,
            message: format!("Agent error during CTR analysis: {}", e),
            output: None,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Create Fix Tasks
// ═══════════════════════════════════════════════════════════════════════════════

/// Spawn up to 3 CTR fix tasks based on the recommendations artifact.
///
/// Looks for a `ctr_recommendations` artifact on the parent task; falls back
/// to reading `ctr_recommendations.json` from the automation directory.
pub(crate) fn create_ctr_fix_tasks(
    conn: &Connection,
    parent_task: &Task,
    project_path: &str,
) -> Vec<String> {
    let paths = ProjectPaths::from_path(project_path);

    // Try to find the artifact on the parent task first
    let recommendation_json = parent_task
        .artifacts
        .iter()
        .find(|a| a.key == "ctr_recommendations")
        .and_then(|a| a.content.clone())
        .or_else(|| {
            // Fallback: read from automation dir
            let fallback_path = paths.automation_dir.join("ctr_recommendations.json");
            std::fs::read_to_string(&fallback_path).ok()
        })
        .unwrap_or_default();

    if recommendation_json.is_empty() {
        log::warn!(
            "[ctr_audit] No ctr_recommendations artifact found for task {}",
            parent_task.id
        );
        return Vec::new();
    }

    let artifact = TaskArtifact {
        key: "ctr_recommendations".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some("ctr_audit".to_string()),
        content: Some(recommendation_json),
    };

    let fix_task_types = [
        ("fix_title_meta", format!("ctr_fix:title_meta:{}", parent_task.project_id)),
        ("fix_faq_schema", format!("ctr_fix:faq:{}", parent_task.project_id)),
        ("fix_snippet_bait", format!("ctr_fix:snippet:{}", parent_task.project_id)),
    ];

    let mut created_ids = Vec::new();

    for (task_type, idempotency_key) in &fix_task_types {
        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: task_type.to_string(),
            title: Some(format!("CTR fix: {}", task_type)),
            description: Some(format!(
                "Follow-up CTR fix task from {} (parent: {})",
                task_type, parent_task.id
            )),
            priority: crate::models::task::Priority::Medium,
            agent_policy: crate::models::task::AgentPolicy::Optional,
            depends_on: vec![parent_task.id.clone()],
            artifacts: vec![artifact.clone()],
            idempotency_key: Some(idempotency_key.clone()),
            ..Default::default()
        };

        match TaskSpawner::spawn(conn, spec) {
            Ok(task) => {
                log::info!("[ctr_audit] Created fix task {} (type: {})", task.id, task_type);
                created_ids.push(task.id);
            }
            Err(e) => {
                log::warn!("[ctr_audit] Failed to create fix task {}: {}", task_type, e);
            }
        }
    }

    created_ids
}
