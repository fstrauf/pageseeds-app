/// Keyword coverage analysis execution module.
///
/// Analyzes existing articles to build a semantic cluster map of keyword coverage.

use crate::engine::project_paths::ProjectPaths;
use crate::engine::task_store;
use crate::engine::workflows::StepResult;
use crate::models::project::ProjectMode;
use crate::models::task::Task;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Execute the `coverage_load_articles` step.
///
/// Reads articles.json or live-site inventory and returns normalized metadata
/// for clustering.
pub(crate) fn exec_coverage_load_articles(
    task: &Task,
    project_path: &str,
) -> StepResult {
    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(conn) => conn,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to open app database: {}", e),
                output: None,
            }
        }
    };

    let project = match task_store::get_project(&db, &task.project_id) {
        Ok(project) => project,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to load project '{}': {}", task.project_id, e),
                output: None,
            }
        }
    };

    if project.project_mode == ProjectMode::LiveSite {
        let pages = match crate::live_site::list_live_site_pages(&db, &task.project_id) {
            Ok(pages) => pages,
            Err(e) => {
                return StepResult {
                    success: false,
                    message: format!("Failed to load live-site inventory: {}", e),
                    output: None,
                }
            }
        };

        if pages.is_empty() {
            return StepResult {
                success: false,
                message: "No live-site pages imported yet. Import the site before running coverage analysis.".to_string(),
                output: None,
            };
        }

        let page_summaries: Vec<serde_json::Value> = pages
            .iter()
            .enumerate()
            .map(|(index, page)| {
                serde_json::json!({
                    "id": (index + 1) as i64,
                    "title": if page.title.trim().is_empty() { page.path.clone() } else { page.title.clone() },
                    "slug": slug_from_live_site_path(&page.path),
                    "target_keyword": "",
                    "file": page.path,
                    "status": "live",
                    "url": page.url,
                    "source_type": "live_site_page",
                })
            })
            .collect();

        let output = serde_json::json!({
            "article_count": page_summaries.len(),
            "articles": page_summaries,
        });

        return StepResult {
            success: true,
            message: format!(
                "Loaded {} live-site pages for coverage analysis",
                output["article_count"].as_i64().unwrap_or(0)
            ),
            output: Some(output.to_string()),
        };
    }

    // Load articles from SQLite (canonical runtime store) instead of articles.json.
    let articles = match crate::content::article_index::list_articles(&db, &task.project_id) {
        Ok(a) => a,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to load articles from DB: {}", e),
                output: None,
            }
        }
    };

    let article_summaries: Vec<serde_json::Value> = articles
        .iter()
        .map(|a| {
            serde_json::json!({
                "id": a.id,
                "title": &a.title,
                "slug": &a.url_slug,
                "target_keyword": a.target_keyword.as_deref().unwrap_or(""),
                "file": &a.file,
                "status": &a.status,
            })
        })
        .collect();

    let output = serde_json::json!({
        "article_count": article_summaries.len(),
        "articles": article_summaries,
    });

    StepResult {
        success: true,
        message: format!("Loaded {} articles for coverage analysis", article_summaries.len()),
        output: Some(output.to_string()),
    }
}

fn slug_from_live_site_path(path: &str) -> String {
    let trimmed = path.trim().trim_matches('/');
    if trimmed.is_empty() {
        return "home".to_string();
    }

    trimmed
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Authority level classification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthorityLevel {
    Strong,
    Moderate,
    Weak,
    Minimal,
}

impl AuthorityLevel {
    pub fn from_score(score: u8) -> Self {
        match score {
            75..=100 => AuthorityLevel::Strong,
            50..=74 => AuthorityLevel::Moderate,
            25..=49 => AuthorityLevel::Weak,
            _ => AuthorityLevel::Minimal,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AuthorityLevel::Strong => "Strong",
            AuthorityLevel::Moderate => "Moderate",
            AuthorityLevel::Weak => "Weak",
            AuthorityLevel::Minimal => "Minimal",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            AuthorityLevel::Strong => "Maintain and expand",
            AuthorityLevel::Moderate => "Strengthen coverage",
            AuthorityLevel::Weak => "Build comprehensive cluster",
            AuthorityLevel::Minimal => "Major opportunity",
        }
    }

    pub fn color_hint(&self) -> &'static str {
        match self {
            AuthorityLevel::Strong => "emerald",
            AuthorityLevel::Moderate => "sky",
            AuthorityLevel::Weak => "amber",
            AuthorityLevel::Minimal => "red",
        }
    }
}

/// Calculate authority score for a cluster
/// 
/// Formula: Coverage (50%) + Position (30%) + Demand (20%)
pub fn calculate_authority_score(
    keyword_count: usize,
    avg_position: f64,
    total_impressions: i64,
) -> u8 {
    // Coverage score (50% weight)
    let coverage_score = match keyword_count {
        n if n >= 50 => 100,
        n if n >= 30 => 80,
        n if n >= 15 => 60,
        n if n >= 8 => 40,
        n if n >= 4 => 20,
        _ => 10,
    };

    // Position score (30% weight)
    let position_score = if avg_position <= 5.0 {
        100
    } else if avg_position <= 10.0 {
        80
    } else if avg_position <= 20.0 {
        60
    } else if avg_position <= 30.0 {
        40
    } else if avg_position <= 50.0 {
        20
    } else {
        10
    };

    // Demand score (20% weight)
    let demand_score = if total_impressions >= 10000 {
        100
    } else if total_impressions >= 5000 {
        80
    } else if total_impressions >= 2000 {
        60
    } else if total_impressions >= 1000 {
        40
    } else if total_impressions >= 500 {
        20
    } else {
        10
    };

    // Weighted total
    let final_score = (
        coverage_score as f64 * 0.50 +
        position_score as f64 * 0.30 +
        demand_score as f64 * 0.20
    ) as u8;

    final_score.min(100)
}

/// Enhance clusters with authority scores and gap detection
pub fn enhance_clusters_with_authority(
    clusters: &mut serde_json::Value,
    articles: &serde_json::Value,
) {
    // Get the clusters array, return early if not found
    let clusters_array = match clusters.get_mut("clusters").and_then(|v| v.as_array_mut()) {
        Some(arr) => arr,
        None => return,
    };

    for cluster in clusters_array.iter_mut() {
        // Get article IDs in this cluster
        let article_ids: Vec<i64> = cluster.get("article_ids")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter()
                .filter_map(|id| id.as_i64())
                .collect())
            .unwrap_or_default();

        // Get GSC data for articles in this cluster
        let (avg_position, total_impressions) = calculate_cluster_metrics(&article_ids, articles);

        // Calculate authority score
        let authority_score = calculate_authority_score(
            article_ids.len(),
            avg_position,
            total_impressions,
        );

        let authority_level = AuthorityLevel::from_score(authority_score);

        // Add to cluster
        if let Some(obj) = cluster.as_object_mut() {
            obj.insert("authority_score".to_string(), serde_json::json!(authority_score));
            obj.insert("authority_level".to_string(), serde_json::json!(authority_level.as_str()));
            obj.insert("authority_description".to_string(), serde_json::json!(authority_level.description()));
            obj.insert("avg_position".to_string(), serde_json::json!(avg_position));
            obj.insert("total_impressions".to_string(), serde_json::json!(total_impressions));
            obj.insert("recommended_action".to_string(), serde_json::json!(authority_level.description()));
        }
    }
}

/// Calculate cluster metrics from GSC data
fn calculate_cluster_metrics(
    article_ids: &[i64],
    articles: &serde_json::Value,
) -> (f64, i64) {
    let articles_array = match articles.get("articles").and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return (100.0, 0),
    };

    let mut total_position = 0.0;
    let mut total_impressions: i64 = 0;
    let mut count = 0;

    for article in articles_array {
        let id = article.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
        
        if article_ids.contains(&id) {
            // Try to get GSC data
            if let Some(gsc) = article.get("gsc") {
                if !gsc.is_null() {
                    let position = gsc.get("position")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(100.0);
                    let impressions = gsc.get("impressions")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);

                    total_position += position;
                    total_impressions += impressions;
                    count += 1;
                }
            }
        }
    }

    let avg_position = if count > 0 {
        total_position / count as f64
    } else {
        100.0 // Default if no GSC data
    };

    (avg_position, total_impressions)
}

/// Execute the `coverage_cluster_analysis` step.
///
/// Sends article metadata to the agent for semantic clustering.
/// The agent groups articles by topic/theme and assigns human-readable cluster names.
#[allow(deprecated)]
pub(crate) fn exec_coverage_cluster_analysis(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
    articles_json: &str,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    log::info!("[coverage_cluster] received articles_json ({} chars)", articles_json.len());
    
    let articles: serde_json::Value = match serde_json::from_str(articles_json) {
        Ok(v) => v,
        Err(e) => {
            log::error!("[coverage_cluster] Failed to parse articles JSON: {}", e);
            return StepResult {
                success: false,
                message: format!("Failed to parse articles JSON: {}", e),
                output: None,
            }
        }
    };

    let article_count = articles["article_count"].as_i64().unwrap_or(0);
    log::info!("[coverage_cluster] article_count: {}", article_count);
    
    if article_count == 0 {
        log::warn!("[coverage_cluster] No articles to cluster");
        return StepResult {
            success: true,
            message: "No articles to cluster".to_string(),
            output: Some(articles_json.to_string()),
        };
    }

    let prompt = build_cluster_prompt(&articles);

    log::info!(
        "[coverage_cluster] running agent ({} chars prompt, {} articles, provider={})",
        prompt.len(),
        article_count,
        agent_provider
    );

    let raw_output = match crate::engine::agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(out) => out,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Agent failed: {}", e),
                output: None,
            }
        }
    };

    // Parse the agent output to extract clusters
    let mut clusters = crate::engine::text::extract_json(&raw_output).unwrap_or_else(|| {
        serde_json::json!({
            "generated_at": chrono::Utc::now().to_rfc3339(),
            "article_count": article_count,
            "clusters": [],
        })
    });

    // NEW: Enhance clusters with authority scores and gap detection
    log::info!("[coverage_cluster] Enhancing clusters with authority scores");
    enhance_clusters_with_authority(&mut clusters, &articles);

    // Persist to keyword_coverage.json for future reference
    if let Err(e) = std::fs::create_dir_all(&paths.automation_dir) {
        return StepResult {
            success: false,
            message: format!("Failed to create automation directory: {}", e),
            output: None,
        };
    }

    let coverage_path = paths.automation_dir.join("keyword_coverage.json");
    let coverage_str = serde_json::to_string_pretty(&clusters).unwrap_or_default() + "\n";
    if let Err(e) = std::fs::write(&coverage_path, &coverage_str) {
        log::warn!("[coverage_cluster] failed to write keyword_coverage.json: {}", e);
    }

    let cluster_count = clusters["clusters"].as_array().map(|a| a.len()).unwrap_or(0);
    
    // Build a summary of cluster names for the message
    let cluster_names: Vec<String> = clusters["clusters"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c["cluster_name"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    
    let summary = if cluster_names.is_empty() {
        "No clusters identified".to_string()
    } else if cluster_names.len() <= 3 {
        format!("Clusters: {}", cluster_names.join(", "))
    } else {
        format!("Clusters: {}, ... ({} total)", cluster_names[..3].join(", "), cluster_count)
    };
    
    log::info!("[coverage_cluster] Complete: {} clusters saved to {:?}", cluster_count, coverage_path);
    
    StepResult {
        success: true,
        message: format!(
            "✓ Analyzed {} articles → {} clusters. {}",
            article_count, cluster_count, summary
        ),
        output: Some(clusters.to_string()),
    }
}

/// Build the agent prompt for clustering articles.
fn build_cluster_prompt(articles: &serde_json::Value) -> String {
    let articles_json = serde_json::to_string_pretty(articles).unwrap_or_default();

    format!(
        r#"You are an SEO content strategist analyzing a website's existing content portfolio.

    Your task is to group the content items into semantic clusters based on their topics and target keywords.

## Articles to Analyze

{articles_json}

## Clustering Instructions

1. Group articles by **topic/theme similarity**, not just keyword matching.
   - Consider the title, target_keyword, and implied topic from the slug.
   - Articles about similar concepts should be in the same cluster.

2. Create **human-readable cluster names**:
   - Use clear, descriptive names like "React Hooks", "Budget Planning", "SEO Fundamentals"
   - Avoid generic names like "Cluster 1" or "Group A"

3. For each cluster, provide:
   - A unique cluster_id (snake_case, e.g., "react_hooks")
   - A human-readable cluster_name
   - The list of article IDs in that cluster
   - Primary keywords/topics that define this cluster

4. Guidelines:
   - Aim for 3-10 meaningful clusters (adjust based on article count and diversity)
   - An article should belong to exactly one cluster (no duplicates)
   - If an article doesn't fit any clear cluster, put it in a "misc" or "general" cluster
   - Clusters should be roughly balanced — avoid one cluster with 90% of articles

## Output Contract (Required)

Return ONLY one valid JSON object. No markdown fences, no commentary.

```json
{{
  "generated_at": "<ISO-8601 timestamp>",
  "article_count": <total number of articles analyzed>,
  "clusters": [
    {{
      "cluster_id": "<snake_case_id>",
      "cluster_name": "<Human Readable Name>",
      "article_ids": [<article id numbers>],
      "primary_keywords": ["keyword1", "keyword2"],
      "article_count": <number of articles in this cluster>
    }}
  ]
}}
```

Requirements:
- cluster_id: Use snake_case, no spaces (e.g., "react_hooks", "budget_planning")
- cluster_name: Use Title Case, descriptive (e.g., "React Hooks", "Budget Planning")
- article_ids: Array of integers matching the article "id" fields from input
- primary_keywords: 2-5 keywords that best represent this cluster's focus
- The sum of article_count across all clusters should equal the total article_count
"#,
        articles_json = articles_json
    )
}

/// Execute the `coverage_save` step.
///
/// Verifies the coverage JSON was written and returns the final result.
pub(crate) fn exec_coverage_save(
    _task: &Task,
    project_path: &str,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let coverage_path = paths.automation_dir.join("keyword_coverage.json");

    if !coverage_path.exists() {
        return StepResult {
            success: false,
            message: "keyword_coverage.json was not created".to_string(),
            output: None,
        };
    }

    match std::fs::read_to_string(&coverage_path) {
        Ok(content) => {
            // Validate it's valid JSON
            match serde_json::from_str::<serde_json::Value>(&content) {
                Ok(parsed) => {
                    let cluster_count = parsed["clusters"].as_array().map(|a| a.len()).unwrap_or(0);
                    let article_count = parsed["article_count"].as_i64().unwrap_or(0);
                    
                    // Get cluster names for a nice summary
                    let cluster_names: Vec<String> = parsed["clusters"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .take(5)
                                .filter_map(|c| c["cluster_name"].as_str().map(|s| s.to_string()))
                                .collect()
                        })
                        .unwrap_or_default();
                    
                    let names_str = if cluster_names.is_empty() {
                        "No clusters found".to_string()
                    } else {
                        format!("Found: {}", cluster_names.join(", "))
                    };
                    
                    StepResult {
                        success: true,
                        message: format!(
                            "✓ Coverage analysis complete!\n{} articles grouped into {} clusters\n{}\n\nResults saved to keyword_coverage.json",
                            article_count, cluster_count, names_str
                        ),
                        output: Some(content),
                    }
                }
                Err(e) => StepResult {
                    success: false,
                    message: format!("Invalid JSON in keyword_coverage.json: {}", e),
                    output: Some(content),
                },
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Failed to read keyword_coverage.json: {}", e),
            output: None,
        },
    }
}

/// Read the existing keyword_coverage.json for a project.
///
/// Returns None if the file doesn't exist or is invalid.
pub fn read_keyword_coverage(project_path: &str) -> Option<serde_json::Value> {
    let paths = ProjectPaths::from_path(project_path);
    let coverage_path = paths.automation_dir.join("keyword_coverage.json");

    let content = std::fs::read_to_string(&coverage_path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Check if keyword coverage exists and return its age.
///
/// Returns (exists, age_description) where age_description is human-readable.
pub fn get_coverage_status(project_path: &str) -> (bool, String) {
    let paths = ProjectPaths::from_path(project_path);
    let coverage_path = paths.automation_dir.join("keyword_coverage.json");

    if !coverage_path.exists() {
        return (false, "Never analyzed".to_string());
    }

    let metadata = match std::fs::metadata(&coverage_path) {
        Ok(m) => m,
        Err(_) => return (false, "Never analyzed".to_string()),
    };

    let modified = match metadata.modified() {
        Ok(m) => m,
        Err(_) => return (true, "Analyzed (unknown date)".to_string()),
    };

    let now = std::time::SystemTime::now();
    let duration = now.duration_since(modified).unwrap_or_default();

    let age_desc = if duration.as_secs() < 60 {
        "Just now".to_string()
    } else if duration.as_secs() < 3600 {
        format!("{} minutes ago", duration.as_secs() / 60)
    } else if duration.as_secs() < 86400 {
        format!("{} hours ago", duration.as_secs() / 3600)
    } else {
        format!("{} days ago", duration.as_secs() / 86400)
    };

    (true, age_desc)
}
