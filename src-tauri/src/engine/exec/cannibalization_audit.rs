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
use crate::models::task::{ExecutionMode, Task, TaskArtifact};

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

    let doc: serde_json::Value = match crate::engine::exec::common::read_json(&articles_path, "articles.json") {
        Ok(v) => v,
        Err(e) => return e,
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
            if similarity >= 0.3 {
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
    let full_doc = serde_json::json!({
        "generated_at": now_iso,
        "total_articles": articles_json.len(),
        "articles": articles_json,
        "similarity_pairs": similarity_pairs,
        "keyword_groups": keyword_groups_json,
    });

    // Write full context to automation dir for reference
    let out_path = paths.automation_dir.join("cannibalization_audit_context.json");
    let full_str = serde_json::to_string_pretty(&full_doc).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&out_path, &full_str) {
        log::warn!(
            "[cannibalization_audit] Failed to write cannibalization_audit_context.json: {}",
            e
        );
    }

    // Return only findings as step output to keep the agentic prompt small
    let summary_doc = serde_json::json!({
        "generated_at": now_iso,
        "total_articles": articles_json.len(),
        "similarity_pairs": similarity_pairs,
        "keyword_groups": keyword_groups_json,
    });
    let summary_str = serde_json::to_string_pretty(&summary_doc).unwrap_or_default() + "\n";

    StepResult {
        success: true,
        message: format!(
            "Cannibalization context built: {} articles, {} similar pairs, {} keyword groups",
            articles_json.len(),
            similarity_pairs.len(),
            keyword_groups_json.len()
        ),
        output: Some(summary_str),
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
                message: "Skill 'cannibalization-strategy' not found in .github/skills/ or app defaults".to_string(),
                output: None,
            };
        }
    };

    // Use string concatenation to avoid format! panics if skill content contains { or }
    let prompt = skill.content
        + "\n\n---\n\n## Cannibalization Audit Context\n\n"
        + context_json
        + "\n\nPlease analyze the above context and provide a cannibalization resolution strategy."
        + "\n\nCRITICAL: Return ONLY a single JSON object matching the Output Contract above."
        + " Do not include markdown prose, summaries, tables, or explanations outside the JSON."
        + " Do not write files. Output the JSON directly in your response.";

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
        ("fix_content_merge", format!("can_fix:merge:{}:{}", parent_task.project_id, parent_task.id)),
        ("fix_hub_page", format!("can_fix:hub:{}:{}", parent_task.project_id, parent_task.id)),
        ("research_territory", format!("can_fix:territory:{}:{}", parent_task.project_id, parent_task.id)),
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
            execution_mode: Some(ExecutionMode::Automatic),
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

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_dir() -> String {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir()
            .join(format!("can_audit_test_{}_{}", std::process::id(), n))
            .to_string_lossy()
            .to_string()
    }

    fn setup_project(path: &str) {
        let _ = std::fs::remove_dir_all(path);
        let auto_dir = Path::new(path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();
        let content_dir = Path::new(path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        let articles = serde_json::json!({
            "articles": [
                {
                    "id": 1,
                    "url_slug": "best-stocks-csp",
                    "title": "Best Stocks for Cash-Secured Puts",
                    "target_keyword": "cash secured puts",
                    "file": "content/001_best_stocks_csp.mdx",
                    "gsc": { "impressions": 45000.0, "clicks": 120.0, "ctr": 0.0027 }
                },
                {
                    "id": 2,
                    "url_slug": "csp-strategy-explained",
                    "title": "Cash-Secured Puts Strategy Explained",
                    "target_keyword": "cash secured puts",
                    "file": "content/002_csp_strategy.mdx",
                    "gsc": { "impressions": 1200.0, "clicks": 5.0, "ctr": 0.0042 }
                },
                {
                    "id": 3,
                    "url_slug": "covered-calls-guide",
                    "title": "Covered Calls Complete Guide",
                    "target_keyword": "covered calls",
                    "file": "content/003_covered_calls.mdx",
                    "gsc": { "impressions": 8000.0, "clicks": 30.0, "ctr": 0.0038 }
                }
            ]
        });
        std::fs::write(auto_dir.join("articles.json"), serde_json::to_string_pretty(&articles).unwrap()).unwrap();

        let mdx1 = r#"---
title: "Best Stocks for Cash-Secured Puts"
date: "2024-01-01"
---

# Best Stocks for Cash-Secured Puts

This article covers the best stocks for cash secured puts strategy in 2024.

## Criteria

We look for stable blue chip stocks with weekly options.
"#;
        std::fs::write(content_dir.join("001_best_stocks_csp.mdx"), mdx1).unwrap();

        let mdx2 = r#"---
title: "Cash-Secured Puts Strategy Explained"
date: "2024-01-02"
---

# Cash-Secured Puts Strategy Explained

This article covers the cash secured puts strategy for beginners looking for the best stocks.

## How It Works

You sell put options while holding cash to buy the stock if assigned.
"#;
        std::fs::write(content_dir.join("002_csp_strategy.mdx"), mdx2).unwrap();

        let mdx3 = r#"---
title: "Covered Calls Complete Guide"
date: "2024-01-03"
---

# Covered Calls Complete Guide

This guide covers covered calls strategy for income generation.

## Basics

You sell call options against stock you already own.
"#;
        std::fs::write(content_dir.join("003_covered_calls.mdx"), mdx3).unwrap();
    }

    fn cleanup(path: &str) {
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn test_jaccard_similarity() {
        let a: HashSet<String> = ["apple".to_string(), "banana".to_string(), "cherry".to_string()].into_iter().collect();
        let b: HashSet<String> = ["apple".to_string(), "banana".to_string(), "date".to_string()].into_iter().collect();
        let sim = jaccard_similarity(&a, &b);
        assert!((sim - 0.5).abs() < 0.01, "Expected ~0.5, got {}", sim);
    }

    #[test]
    fn test_exec_can_build_context() {
        let path = test_dir();
        setup_project(&path);
        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "cannibalization_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Automatic,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test Cannibalization Audit".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_can_build_context(&task, &path);
        assert!(result.success, "build_context failed: {}", result.message);

        let output: serde_json::Value = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(output["total_articles"].as_i64().unwrap(), 3);

        let groups = output["keyword_groups"].as_object().unwrap();
        let csp_group = groups.get("cash secured puts");
        assert!(csp_group.is_some(), "Should find 'cash secured puts' keyword group");
        assert_eq!(csp_group.unwrap().as_array().unwrap().len(), 2);

        let pairs = output["similarity_pairs"].as_array().unwrap();
        assert!(!pairs.is_empty(), "Should find at least one similarity pair");
        cleanup(&path);
    }
}
