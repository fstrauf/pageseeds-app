use crate::engine::spawner::{TaskSpawner, TaskSpec};
use crate::models::task::{AgentPolicy, Priority, TaskArtifact};
use std::collections::HashSet;

/// Auto-create `write_article` tasks from keyword research results.
///
/// This function extracts keywords from the research task's artifacts and creates
/// corresponding write_article tasks. It mirrors the behavior of the frontend
/// KeywordPicker + createArticleTasksFromKeywords, but runs automatically on the backend.
///
/// # Arguments
/// * `conn` - SQLite connection
/// * `research_task` - The completed research_keywords task
/// * `max_tasks` - Maximum number of article tasks to create (default: 5)
///
/// # Returns
/// The number of article tasks created, or 0 if no suitable keywords were found.
pub fn auto_create_article_tasks_from_research(
    conn: &rusqlite::Connection,
    research_task: &crate::models::task::Task,
    max_tasks: Option<usize>,
) -> crate::error::Result<usize> {
    let max_tasks = max_tasks.unwrap_or(5);
    if max_tasks == 0 {
        return Ok(0);
    }

    // Extract keywords with their metrics from the research artifact
    let keyword_data = extract_keywords_with_metrics(research_task);
    if keyword_data.is_empty() {
        log::info!(
            "[auto_create_articles] No keywords found in research task {}",
            research_task.id
        );
        return Ok(0);
    }

    // Sort by opportunity score (higher = better)
    // Opportunity score: lower difficulty + higher volume/traffic
    let mut keyword_data = keyword_data;
    keyword_data.sort_by(|a, b| {
        let score_a = calculate_opportunity_score(a);
        let score_b = calculate_opportunity_score(b);
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Take top N keywords
    let selected = keyword_data.into_iter().take(max_tasks).collect::<Vec<_>>();

    let mut created_count = 0usize;
    for kw_data in &selected {
        let title = to_title_case(&kw_data.keyword);

        // Determine priority based on difficulty
        let priority_enum = match kw_data.difficulty {
            Some(kd) if kd <= 30 => Priority::High,
            Some(kd) if kd <= 45 => Priority::Medium,
            Some(_) => Priority::Low,
            None => match kw_data.volume {
                Some(v) if v >= 1000 => Priority::High,
                Some(v) if v >= 250 => Priority::Medium,
                _ => Priority::Medium,
            },
        };

        // Build description with keyword metadata
        let mut description = format!("Target keyword: {}", kw_data.keyword);
        if let Some(kd) = kw_data.difficulty {
            description.push_str(&format!("\nKD: {}", kd));
        }
        if let Some(vol) = kw_data.volume {
            description.push_str(&format!("\nVolume: {}", vol));
        }

        // Create provenance artifact linking back to research
        let provenance = TaskArtifact {
            key: "keyword_research".to_string(),
            path: None,
            artifact_type: Some("json".to_string()),
            source: Some(research_task.id.clone()),
            content: Some(format!(
                "{{\"keyword\":\"{}\"}}",
                kw_data.keyword.replace('"', "\\\"")
            )),
        };

        let idempotency_key = format!(
            "auto_article:{}:{}",
            research_task.id,
            kw_data.keyword.to_lowercase().replace(' ', "_")
        );

        let spec = TaskSpec {
            project_id: research_task.project_id.clone(),
            task_type: "write_article".to_string(),
            title: Some(title),
            description: Some(description),
            priority: priority_enum,
            agent_policy: AgentPolicy::None,
            depends_on: vec![research_task.id.clone()],
            artifacts: vec![provenance],
            idempotency_key: Some(idempotency_key),
            ..Default::default()
        };

        match TaskSpawner::spawn(conn, spec) {
            Ok(task) => {
                log::info!(
                    "[auto_create_articles] Created write_article task {} for keyword '{}'",
                    task.id,
                    kw_data.keyword
                );
                created_count += 1;
            }
            Err(e) => {
                log::warn!(
                    "[auto_create_articles] Failed to create task for keyword '{}': {}",
                    kw_data.keyword,
                    e
                );
            }
        }
    }

    log::info!(
        "[auto_create_articles] Created {} article tasks from research task {}",
        created_count,
        research_task.id
    );

    Ok(created_count)
}

/// Helper struct for keyword data with metrics
struct KeywordData {
    keyword: String,
    difficulty: Option<i64>,
    volume: Option<i64>,
    traffic: Option<i64>,
    has_data: bool,
}

/// Extract keywords with their metrics from a research task's artifacts.
fn extract_keywords_with_metrics(task: &crate::models::task::Task) -> Vec<KeywordData> {
    use serde_json::Value;

    // Find the research artifact - prefer normalized/deterministic output
    let artifact = task
        .artifacts
        .iter()
        .find(|a| a.key == "research_normalize_stage")
        .or_else(|| {
            task.artifacts
                .iter()
                .find(|a| a.key == "research_keywords_cli")
        })
        .or_else(|| {
            task.artifacts
                .iter()
                .find(|a| a.key == "research_agent_stage")
        });

    let Some(raw) = artifact.and_then(|a| a.content.as_ref()) else {
        return Vec::new();
    };

    // Try to parse as JSON
    let v: Value = match serde_json::from_str(raw) {
        Ok(v) => v,
        Err(_) => {
            // If JSON parsing fails, try markdown table fallback
            return extract_keywords_from_markdown_table_auto(raw);
        }
    };

    let mut results = Vec::new();
    let mut seen = HashSet::new();

    // Try difficulty.results first (preferred format from CLI research)
    if let Some(arr) = v
        .get("difficulty")
        .and_then(|x| x.get("results"))
        .and_then(|x| x.as_array())
    {
        for item in arr {
            if let Some(kw) = item.get("keyword").and_then(|x| x.as_str()) {
                if seen.insert(kw.to_lowercase()) {
                    results.push(KeywordData {
                        keyword: kw.to_string(),
                        difficulty: item.get("difficulty").and_then(|x| x.as_i64()),
                        volume: item.get("volume").and_then(|x| x.as_i64()),
                        traffic: item.get("traffic").and_then(|x| x.as_i64()),
                        has_data: item
                            .get("has_data")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(true),
                    });
                }
            }
        }
        if !results.is_empty() {
            return results;
        }
    }

    // Fallback to difficulty as direct array
    if let Some(arr) = v.get("difficulty").and_then(|x| x.as_array()) {
        for item in arr {
            if let Some(kw) = item.get("keyword").and_then(|x| x.as_str()) {
                if seen.insert(kw.to_lowercase()) {
                    results.push(KeywordData {
                        keyword: kw.to_string(),
                        difficulty: item.get("difficulty").and_then(|x| x.as_i64()),
                        volume: item.get("volume").and_then(|x| x.as_i64()),
                        traffic: item.get("traffic").and_then(|x| x.as_i64()),
                        has_data: item
                            .get("has_data")
                            .and_then(|x| x.as_bool())
                            .unwrap_or(true),
                    });
                }
            }
        }
        if !results.is_empty() {
            return results;
        }
    }

    // Fallback to new_keywords (no metrics available)
    if let Some(arr) = v.get("new_keywords").and_then(|x| x.as_array()) {
        for item in arr.iter().take(10) {
            if let Some(kw) = item.as_str() {
                if seen.insert(kw.to_lowercase()) {
                    results.push(KeywordData {
                        keyword: kw.to_string(),
                        difficulty: None,
                        volume: None,
                        traffic: None,
                        has_data: false,
                    });
                }
            }
        }
    }

    results
}

/// Parse a range like "100-1,000" or "1,000" and return the midpoint or single value.
fn parse_range_midpoint(raw: &str) -> Option<i64> {
    let nums: Vec<i64> = raw
        .split(|c: char| !c.is_ascii_digit() && c != ',')
        .filter(|s| !s.trim().is_empty())
        .filter_map(|s| s.replace(',', "").parse::<i64>().ok())
        .collect();

    match nums.len() {
        0 => None,
        1 => Some(nums[0]),
        _ => Some((nums[0] + nums[1]) / 2),
    }
}

/// Extract keywords from markdown table (fallback for agent output).
fn extract_keywords_from_markdown_table_auto(raw: &str) -> Vec<KeywordData> {
    let mut results = Vec::new();
    let mut seen = HashSet::new();

    for line in raw.lines() {
        if !line.contains('|') || line.contains("---") {
            continue;
        }

        let cols: Vec<String> = line
            .split('|')
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();

        // Expected shape: Priority | Keyword | Vol | KD
        if cols.len() < 4 {
            continue;
        }
        if cols[0].eq_ignore_ascii_case("priority") || cols[1].eq_ignore_ascii_case("keyword") {
            continue;
        }

        let keyword = &cols[1];
        if seen.insert(keyword.to_lowercase()) {
            let volume = parse_range_midpoint(&cols[2]);
            let difficulty = parse_range_midpoint(&cols[3]);
            results.push(KeywordData {
                keyword: keyword.clone(),
                difficulty,
                volume,
                traffic: None,
                has_data: difficulty.is_some(),
            });
        }
    }

    results
}

/// Calculate opportunity score for a keyword.
/// Higher score = better opportunity (lower difficulty, higher volume).
fn calculate_opportunity_score(kw: &KeywordData) -> f64 {
    let kd_score = match kw.difficulty {
        None => 40.0, // Default when no data
        Some(kd) => (100.0 - kd as f64).max(0.0),
    };

    let traffic_signal = kw.traffic.unwrap_or(0).max(kw.volume.unwrap_or(0)) as f64;
    let traffic_score = (traffic_signal + 1.0).log10() * 25.0;
    let traffic_score = traffic_score.min(100.0);

    kd_score * 0.6 + traffic_score * 0.4
}

/// Simple title-case: capitalise the first letter of each word.
fn to_title_case(s: &str) -> String {
    s.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
