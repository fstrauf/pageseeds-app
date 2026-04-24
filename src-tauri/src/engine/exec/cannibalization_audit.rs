/// Keyword cannibalization audit execution module.
///
/// Covers:
///   - exec_can_build_context   (deterministic similarity analysis + keyword grouping)
///   - exec_can_analyze         (agentic analysis with cannibalization-strategy skill)
///   - create_can_fix_tasks     (spawn follow-up fix tasks)

use std::collections::{HashMap, HashSet};
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

/// Build the cannibalization audit context by reading articles.json, computing
/// Jaccard similarity between article content fingerprints, and grouping by
/// identical target keywords.
pub(crate) fn exec_can_build_context(task: &Task, project_path: &str) -> StepResult {
    let _ = task;
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

    // Collect article records with word sets for similarity
    #[derive(Debug)]
    struct ArticleRecord {
        id: i64,
        url_slug: String,
        title: String,
        h1: String,
        target_keyword: String,
        first_200_words: String,
        file: String,
        gsc: serde_json::Value,
        word_set: HashSet<String>,
    }

    let mut records: Vec<ArticleRecord> = Vec::new();

    for article in articles.iter() {
        let id = article["id"].as_i64().unwrap_or(0);
        let url_slug = article["url_slug"].as_str().unwrap_or("").to_string();
        let title = article["title"].as_str().unwrap_or("").to_string();
        let target_keyword = article["target_keyword"].as_str().unwrap_or("").to_string();
        let file_ref = article["file"].as_str().unwrap_or("").to_string();
        let gsc = article["gsc"].clone();

        let (h1, first_200_words) = read_article_head_and_words(project_path, &file_ref);

        // Build word set from title + h1 + target_keyword + first_200_words
        let combined_text = format!("{} {} {} {}", title, h1, target_keyword, first_200_words);
        let word_set = extract_word_set(&combined_text);

        records.push(ArticleRecord {
            id,
            url_slug,
            title,
            h1,
            target_keyword,
            first_200_words,
            file: file_ref,
            gsc,
            word_set,
        });
    }

    // Compute Jaccard similarity for all pairs
    let mut similarity_pairs: Vec<serde_json::Value> = Vec::new();
    for i in 0..records.len() {
        for j in (i + 1)..records.len() {
            let a = &records[i];
            let b = &records[j];
            let similarity = jaccard_similarity(&a.word_set, &b.word_set);
            if similarity > 0.3 {
                similarity_pairs.push(serde_json::json!({
                    "article_a_id": a.id,
                    "article_b_id": b.id,
                    "article_a_title": a.title,
                    "article_b_title": b.title,
                    "similarity": similarity,
                }));
            }
        }
    }

    // Sort similarity pairs descending
    similarity_pairs.sort_by(|a, b| {
        let sa = a["similarity"].as_f64().unwrap_or(0.0);
        let sb = b["similarity"].as_f64().unwrap_or(0.0);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Group articles by identical target_keyword
    let mut keyword_groups: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for r in &records {
        let kw = r.target_keyword.trim().to_lowercase();
        if kw.is_empty() {
            continue;
        }
        let entry = serde_json::json!({
            "id": r.id,
            "title": r.title,
            "url_slug": r.url_slug,
            "file": r.file,
        });
        keyword_groups.entry(kw).or_default().push(entry);
    }

    // Only keep groups with 2+ articles
    let keyword_groups_json: HashMap<String, Vec<serde_json::Value>> = keyword_groups
        .into_iter()
        .filter(|(_, v)| v.len() >= 2)
        .collect();

    // Build serializable article list
    let articles_json: Vec<serde_json::Value> = records
        .iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "url_slug": r.url_slug,
                "title": r.title,
                "h1": r.h1,
                "target_keyword": r.target_keyword,
                "first_200_words": r.first_200_words,
                "file": r.file,
                "gsc": r.gsc,
            })
        })
        .collect();

    let now_iso = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let output_doc = serde_json::json!({
        "generated_at": now_iso,
        "total_articles": articles_json.len(),
        "articles": articles_json,
        "similarity_pairs": similarity_pairs,
        "keyword_groups": keyword_groups_json,
    });

    // Write context to automation dir for reference
    let out_path = paths.automation_dir.join("cannibalization_audit_context.json");
    let out_str = serde_json::to_string_pretty(&output_doc).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&out_path, &out_str) {
        log::warn!(
            "[cannibalization_audit] Failed to write cannibalization_audit_context.json: {}",
            e
        );
    }

    StepResult {
        success: true,
        message: format!(
            "Cannibalization context built: {} articles, {} similar pairs, {} keyword groups",
            articles_json.len(),
            similarity_pairs.len(),
            keyword_groups_json.len()
        ),
        output: Some(out_str),
    }
}

/// Read an MDX file and extract (h1, first_200_words).
fn read_article_head_and_words(project_path: &str, file_ref: &str) -> (String, String) {
    if file_ref.is_empty() {
        return (String::new(), String::new());
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
            log::warn!(
                "[cannibalization_audit] Could not read {}: {}",
                full.display(),
                e
            );
            return (String::new(), String::new());
        }
    };

    let (_, body) = match crate::content::cleaner::parse_frontmatter(&content) {
        Some((_, b)) => ("", b),
        None => ("", content.as_str()),
    };

    // Extract h1
    let h1 = body
        .lines()
        .find(|l| {
            let t = l.trim_start();
            t.starts_with("# ") && !t.starts_with("## ")
        })
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .unwrap_or_default();

    // Extract first 200 words from body (strip markdown syntax roughly)
    let plain = body
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#') && !t.starts_with("---")
        })
        .collect::<Vec<_>>()
        .join(" ");

    let words: Vec<&str> = plain.split_whitespace().collect();
    let first_200_words = words.into_iter().take(200).collect::<Vec<_>>().join(" ");

    (h1, first_200_words)
}

/// Extract a set of normalized words from text.
fn extract_word_set(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty() && s.len() > 2)
        .map(|s| s.to_string())
        .collect()
}

/// Compute Jaccard similarity between two word sets.
fn jaccard_similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let intersection_size = a.intersection(b).count() as f64;
    let union_size = a.union(b).count() as f64;
    if union_size == 0.0 {
        return 0.0;
    }
    intersection_size / union_size
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 2: Analyze
// ═══════════════════════════════════════════════════════════════════════════════

/// Run the cannibalization strategy analysis using an LLM agent.
///
/// Loads the "cannibalization-strategy" skill, builds a prompt with the skill
/// content and the provided context JSON, and delegates to the agent.
pub(crate) fn exec_can_analyze(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: &str,
) -> StepResult {
    let repo_root = Path::new(project_path);

    let skill = match skills::load_skill(repo_root, "cannibalization-strategy") {
        Some(s) => s,
        None => {
            return StepResult {
                success: false,
                message: "Skill 'cannibalization-strategy' not found in .github/skills/".to_string(),
                output: None,
            };
        }
    };

    let prompt = format!(
        "{skill_content}\n\n---\n\n## Cannibalization Audit Context\n\n{context}\n\nPlease analyze the above context and provide a cannibalization resolution strategy.",
        skill_content = skill.content,
        context = context_json,
    );

    match agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(output) => StepResult {
            success: true,
            message: "Cannibalization analysis completed".to_string(),
            output: Some(output),
        },
        Err(e) => StepResult {
            success: false,
            message: format!("Agent error during cannibalization analysis: {}", e),
            output: None,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Create Fix Tasks
// ═══════════════════════════════════════════════════════════════════════════════

/// Spawn up to 3 cannibalization fix tasks based on the strategy artifact.
///
/// Looks for a `cannibalization_strategy` artifact on the parent task; falls
/// back to reading `cannibalization_strategy.json` from the automation directory.
pub(crate) fn create_can_fix_tasks(
    conn: &Connection,
    parent_task: &Task,
    project_path: &str,
) -> Vec<String> {
    let paths = ProjectPaths::from_path(project_path);

    // Try to find the artifact on the parent task first
    let strategy_json = parent_task
        .artifacts
        .iter()
        .find(|a| a.key == "cannibalization_strategy")
        .and_then(|a| a.content.clone())
        .or_else(|| {
            // Fallback: read from automation dir
            let fallback_path = paths.automation_dir.join("cannibalization_strategy.json");
            std::fs::read_to_string(&fallback_path).ok()
        })
        .unwrap_or_default();

    if strategy_json.is_empty() {
        log::warn!(
            "[cannibalization_audit] No cannibalization_strategy artifact found for task {}",
            parent_task.id
        );
        return Vec::new();
    }

    let artifact = TaskArtifact {
        key: "cannibalization_strategy".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some("cannibalization_audit".to_string()),
        content: Some(strategy_json),
    };

    let fix_task_types = [
        ("fix_content_merge", format!("can_fix:merge:{}", parent_task.project_id)),
        ("fix_hub_page", format!("can_fix:hub:{}", parent_task.project_id)),
        ("research_territory", format!("can_fix:territory:{}", parent_task.project_id)),
    ];

    let mut created_ids = Vec::new();

    for (task_type, idempotency_key) in &fix_task_types {
        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: task_type.to_string(),
            title: Some(format!("Cannibalization fix: {}", task_type)),
            description: Some(format!(
                "Follow-up cannibalization fix task from {} (parent: {})",
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
                log::info!(
                    "[cannibalization_audit] Created fix task {} (type: {})",
                    task.id, task_type
                );
                created_ids.push(task.id);
            }
            Err(e) => {
                log::warn!(
                    "[cannibalization_audit] Failed to create fix task {}: {}",
                    task_type, e
                );
            }
        }
    }

    created_ids
}
