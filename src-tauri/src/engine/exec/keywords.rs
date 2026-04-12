/// Keyword research execution module.
///
/// Native Rust pipeline:
///   1. `get_keyword_ideas` per theme → keywords WITH volume
///   2. Dedupe against articles.json + coverage analysis
///   3. Filter/prioritize by coverage gaps (skip well-covered topics)
///   4. `get_keyword_difficulty` per top-N keyword → KD scores
///   5. Merge into the standard output schema so KeywordPicker shows both volume and KD.

use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

// ─── Coverage-Aware Filtering ─────────────────────────────────────────────────

/// Coverage cluster data loaded from keyword_coverage.json
#[derive(Debug, Clone)]
struct CoverageCluster {
    id: String,
    name: String,
    primary_keywords: Vec<String>,
    article_count: i64,
}

/// Load coverage clusters from keyword_coverage.json if available
fn load_coverage_clusters(project_path: &str) -> Vec<CoverageCluster> {
    let coverage = match crate::engine::exec::coverage::read_keyword_coverage(project_path) {
        Some(c) => c,
        None => return Vec::new(),
    };
    
    coverage.get("clusters")
        .and_then(|c| c.as_array())
        .map(|clusters| {
            clusters.iter().filter_map(|c| {
                let id = c.get("cluster_id")?.as_str()?.to_string();
                let name = c.get("cluster_name")?.as_str()?.to_string();
                let article_count = c.get("article_count")?.as_i64()?;
                let primary_keywords = c.get("primary_keywords")
                    .and_then(|k| k.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|k| k.as_str().map(|s| s.to_lowercase()))
                            .collect()
                    })
                    .unwrap_or_default();
                
                Some(CoverageCluster {
                    id,
                    name,
                    primary_keywords,
                    article_count,
                })
            }).collect()
        })
        .unwrap_or_default()
}

/// Score how well a keyword fills a coverage gap.
/// 
/// Returns (score, match_type, cluster_name):
/// - score: 0-100, higher = better gap fill
/// - match_type: "exact", "semantic", "new_topic"
/// - cluster_name: which cluster it relates to (if any)
/// 
/// Scoring logic:
/// - Keywords not matching any cluster: 100 (new topic, highest priority)
/// - Keywords matching a cluster with < 3 articles: 80 (thin cluster, needs content)
/// - Keywords matching a cluster with 3-5 articles: 50 (moderate coverage)
/// - Keywords matching a cluster with > 5 articles: 20 (well covered, low priority)
fn score_coverage_gap(
    keyword: &str,
    clusters: &[CoverageCluster],
    existing_keywords: &HashSet<String>,
) -> (u8, &'static str, Option<String>) {
    let kw_lower = keyword.to_lowercase();
    
    // Exact duplicate check
    if existing_keywords.contains(&kw_lower) {
        return (0, "exact_duplicate", None);
    }
    
    // Check for semantic match against cluster keywords
    for cluster in clusters {
        let is_related = cluster.primary_keywords.iter().any(|pk| {
            // Keyword contains cluster keyword OR cluster keyword contains keyword
            kw_lower.contains(pk) || pk.contains(&kw_lower)
        });
        
        if is_related {
            let score = match cluster.article_count {
                0..=2 => 80,   // Thin cluster - high priority
                3..=5 => 50,   // Moderate coverage
                6..=10 => 30,  // Good coverage
                _ => 20,       // Well covered - low priority
            };
            return (score, "semantic", Some(cluster.name.clone()));
        }
    }
    
    // No cluster match = new topic, highest priority
    (100, "new_topic", None)
}

/// Filter and sort candidates by coverage gap score.
/// 
/// Removes exact duplicates and low-value keywords, prioritizes gap-filling keywords.
fn filter_by_coverage_gap(
    candidates: Vec<Candidate>,
    clusters: &[CoverageCluster],
    existing_keywords: &HashSet<String>,
) -> Vec<Candidate> {
    let mut scored: Vec<(Candidate, u8, &'static str)> = candidates
        .into_iter()
        .filter_map(|c| {
            let (score, match_type, _) = score_coverage_gap(&c.keyword, clusters, existing_keywords);
            
            // Filter out exact duplicates entirely
            if score == 0 {
                return None;
            }
            
            Some((c, score, match_type))
        })
        .collect();
    
    // Sort by gap score desc, then by volume desc
    scored.sort_by(|a, b| {
        let score_cmp = b.1.cmp(&a.1); // Higher gap score first
        if score_cmp != std::cmp::Ordering::Equal {
            return score_cmp;
        }
        let vol_a = a.0.volume.unwrap_or(0);
        let vol_b = b.0.volume.unwrap_or(0);
        vol_b.cmp(&vol_a) // Higher volume first
    });
    
    // Log the distribution
    let new_topic_count = scored.iter().filter(|(_, _, t)| *t == "new_topic").count();
    let semantic_count = scored.iter().filter(|(_, _, t)| *t == "semantic").count();
    log::info!(
        "[coverage_filter] {} new topics, {} semantic matches after gap filtering",
        new_topic_count,
        semantic_count
    );
    
    scored.into_iter().map(|(c, _, _)| c).collect()
}

// ─── Research Mode ────────────────────────────────────────────────────────────

/// Research mode for the keyword pipeline.
#[derive(Debug, Clone, Copy)]
pub enum ResearchMode {
    /// Informational content research (blog articles)
    Informational,
    /// Commercial content research (landing pages)
    Commercial,
}

impl ResearchMode {
    /// Determine research mode from task type string.
    pub fn from_task_type(task_type: &str) -> Self {
        match task_type {
            "research_landing_pages" => ResearchMode::Commercial,
            _ => ResearchMode::Informational,
        }
    }
}

/// Parse themes from the `research_seed_extraction` step artifact.
/// Parsed result from the research_seed_extraction artifact.
#[derive(Debug, Clone, Default)]
struct SeedArtifact {
    themes: Vec<String>,
    competitors: Vec<String>,
}

/// Parse the output of Step 1 in the hybrid research workflow.
/// Expects JSON with {"themes": [...], "competitors": [...]}.
fn parse_seed_extraction_artifact(task: &Task) -> SeedArtifact {
    let content = task
        .artifacts
        .iter()
        .rev()
        .find(|a| a.key == "research_seed_extraction")
        .and_then(|a| a.content.as_deref());

    let Some(raw) = content else {
        return SeedArtifact::default();
    };

    // Try to parse as JSON first
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(raw) {
        let themes = themes_from_json(&json);
        let competitors = competitors_from_json(&json);
        if !themes.is_empty() || !competitors.is_empty() {
            return SeedArtifact { themes, competitors };
        }
    }

    // Fallback: try normalizer output format
    let normalized = crate::engine::normalizer::normalize_agent_output(raw);
    if let Some(json) = normalized.json_artifact {
        let themes = themes_from_json(&json);
        let competitors = competitors_from_json(&json);
        if !themes.is_empty() || !competitors.is_empty() {
            return SeedArtifact { themes, competitors };
        }
    }

    SeedArtifact::default()
}

fn themes_from_json(v: &serde_json::Value) -> Vec<String> {
    let from_array = |arr: &[serde_json::Value]| {
        arr.iter()
            .filter_map(|x| x.as_str())
            .filter_map(clean_theme_str)
            .collect::<Vec<String>>()
    };

    // Accept either object-based or array-based contracts.
    if let Some(arr) = v.as_array() {
        return from_array(arr);
    }

    for key in ["themes", "selected_themes", "keyword_themes"] {
        if let Some(arr) = v.get(key).and_then(|x| x.as_array()) {
            return from_array(arr);
        }
    }

    vec![]
}

fn competitors_from_json(v: &serde_json::Value) -> Vec<String> {
    let extract = |arr: &[serde_json::Value]| {
        arr.iter()
            .filter_map(|x| x.as_str())
            .map(|s| s.trim().trim_start_matches("https://").trim_start_matches("http://").split('/').next().unwrap_or(s).to_string())
            .filter(|s| !s.is_empty() && s.contains('.'))
            .collect::<Vec<String>>()
    };

    if let Some(arr) = v.get("competitors").and_then(|x| x.as_array()) {
        return extract(arr);
    }

    vec![]
}

pub(crate) fn estimate_volume(raw: &str) -> Option<i64> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }

    // Ahrefs free tools often return enum-like labels instead of numeric ranges.
    match s {
        "MoreThanTenThousand" => return Some(10000),
        "MoreThanOneThousand" => return Some(1000),
        "MoreThanOneHundred" => return Some(100),
        "LessThanOneHundred" => return Some(50),
        _ => {}
    }

    let mut raw_chunks: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() || ch == ',' {
            current.push(ch);
        } else if !current.is_empty() {
            raw_chunks.push(current.clone());
            current.clear();
        }
    }
    if !current.is_empty() {
        raw_chunks.push(current);
    }

    let nums: Vec<i64> = raw_chunks
        .into_iter()
        .map(|c| c.replace(',', ""))
        .filter_map(|p| p.parse::<i64>().ok())
        .collect();

    match nums.as_slice() {
        [] => None,
        [single] => Some(*single),
        [a, b, ..] => Some((a + b) / 2),
    }
}

fn best_serp_metric(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    values.flatten().fold(None, |acc, v| match acc {
        Some(current) if current >= v => Some(current),
        _ => Some(v),
    })
}

/// A keyword candidate discovered from a seed theme.
#[derive(Debug, Clone)]
struct Candidate {
    keyword: String,
    source_theme: String,
    is_question: bool,
    volume: Option<i64>,
    kd: Option<f64>,
    intent: Option<String>,
}

/// Smart sampling: select a diverse subset of candidates for KD checking.
/// Ensures coverage across themes and reserves slots for question keywords.
fn smart_sample_candidates(candidates: Vec<Candidate>, budget: usize) -> Vec<Candidate> {
    if candidates.len() <= budget {
        return candidates;
    }

    use std::collections::HashMap;

    // Group by source theme.
    let mut by_theme: HashMap<String, Vec<Candidate>> = HashMap::new();
    for c in candidates {
        by_theme.entry(c.source_theme.clone()).or_default().push(c);
    }

    let theme_count = by_theme.len().max(1);
    let base_per_theme = budget / theme_count;
    let mut extra = budget % theme_count;

    let mut result: Vec<Candidate> = vec![];
    let mut remaining: Vec<Candidate> = vec![];

    for (_theme, mut group) in by_theme {
        // Sort each theme's group by volume descending.
        group.sort_by(|a, b| {
            let va = a.volume.unwrap_or(0);
            let vb = b.volume.unwrap_or(0);
            vb.cmp(&va)
        });

        let quota = base_per_theme + if extra > 0 { extra -= 1; 1 } else { 0 };

        // Reserve at least 1 slot for a question keyword if available.
        let question_idx = group.iter().position(|c| c.is_question);
        let mut picked = 0usize;

        if let Some(qidx) = question_idx {
            if quota > 0 {
                result.push(group.remove(qidx));
                picked += 1;
            }
        }

        // Fill remainder of this theme's quota with highest-volume keywords.
        while picked < quota && !group.is_empty() {
            result.push(group.remove(0));
            picked += 1;
        }

        // Anything left goes into the global remaining pool.
        remaining.extend(group);
    }

    // If we still have budget left, fill with highest-volume remaining candidates.
    remaining.sort_by(|a, b| {
        let va = a.volume.unwrap_or(0);
        let vb = b.volume.unwrap_or(0);
        vb.cmp(&va)
    });

    while result.len() < budget && !remaining.is_empty() {
        result.push(remaining.remove(0));
    }

    result
}

pub(crate) fn exec_keyword_research_native(
    task: &Task,
    project_path: &str,
    seo_provider: &str,
) -> crate::engine::workflows::StepResult {
    use crate::config::env_resolver::EnvResolver;

    let paths = ProjectPaths::from_path(project_path);
    let is_dataforseo = seo_provider.eq_ignore_ascii_case("dataforseo");

    // ── Re-use cached results if this step already ran ────────────────────────
    // Prevents burning paid API credits on accidental re-runs.
    if let Some(existing) = task.artifacts.iter().find(|a| a.key == "research_ahrefs_pipeline") {
        if let Some(ref content) = existing.content {
            if !content.is_empty() {
                log::info!(
                    "[keyword_research_native] reusing cached artifact ({} chars) — skipping API calls",
                    content.len()
                );
                return crate::engine::workflows::StepResult {
                    success: true,
                    message: "Keyword research (cached) — no API calls made".to_string(),
                    output: Some(content.clone()),
                };
            }
        }
    }

    // ── For Ahrefs path, also need CAPSOLVER_API_KEY ──────────────────────────
    let capsolver_key = if !is_dataforseo {
        let env = EnvResolver::new(project_path).build_env(HashMap::new());
        match env.get("CAPSOLVER_API_KEY").map(|s| s.as_str()) {
            Some(k) if !k.is_empty() => Some(k.to_string()),
            _ => {
                return crate::engine::workflows::StepResult {
                    success: false,
                    message: "CAPSOLVER_API_KEY not set. Add it in Settings → Secrets.".to_string(),
                    output: None,
                };
            }
        }
    } else {
        None
    };

    // ── Parse themes from task description ───────────────────────────────────
    let raw_desc = task.description.as_deref().unwrap_or("");
    let desc_themes = parse_desc_themes(raw_desc);

    let SeedArtifact {
        themes: agent_themes,
        competitors: agent_competitors,
    } = parse_seed_extraction_artifact(task);

    let themes = if !desc_themes.is_empty() {
        desc_themes
    } else if !agent_themes.is_empty() {
        agent_themes
    } else {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "No keyword themes available. Provide themes in task description or run agentic theme selection first. \
                 Expected artifact key: research_seed_extraction. Workspace: {}.",
                paths.automation_dir.display()
            ),
            output: None,
        };
    };

    log::info!("[keyword_research_native] {} themes: {:?}", themes.len(), themes);
    log::info!("[keyword_research_native] {} competitors: {:?}", agent_competitors.len(), agent_competitors);

    // ── Cost estimate (DataForSEO) ────────────────────────────────────────────
    if is_dataforseo {
        let est_cost = themes.len() as f64 * 0.012; // ~$0.01/task + $0.0001 × ~20 keywords
        log::info!(
            "[keyword_research_native] DataForSEO estimated cost: ${:.3} ({} themes × $0.012/theme)",
            est_cost, themes.len()
        );
    }

    // ── Pre-flight: articles.json must exist ──────────────────────────────────
    let articles_json_path = paths.automation_dir.join("articles.json");
    if !articles_json_path.exists() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "Workspace not initialised: articles.json not found at {}. \
                 Run 'Init Workspace' from Project Settings first.",
                articles_json_path.display()
            ),
            output: None,
        };
    }

    // Load existing keywords from articles.json so we can skip already-covered ones.
    // articles.json format: {"nextArticleId": N, "articles": [...]}
    let existing_keywords: HashSet<String> = std::fs::read_to_string(&articles_json_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| {
            // Support both {"articles": [...]} wrapper and bare [...] array.
            let arr = v["articles"].as_array().or_else(|| v.as_array());
            arr.map(|items| {
                items.iter()
                    .filter_map(|a| a["target_keyword"].as_str())
                    .map(|k| k.to_lowercase())
                    .collect()
            })
        })
        .unwrap_or_default();

    log::info!("[keyword_research_native] {} existing keywords to filter against", existing_keywords.len());

    // ── Load coverage analysis for gap filtering ──────────────────────────────
    let coverage_clusters = load_coverage_clusters(project_path);
    let has_coverage = !coverage_clusters.is_empty();
    if has_coverage {
        log::info!("[keyword_research_native] loaded {} coverage clusters for gap analysis", coverage_clusters.len());
    } else {
        log::info!("[keyword_research_native] no coverage analysis found, skipping gap filtering");
    }

    // ── Bridge to tokio async runtime ─────────────────────────────────────────
    // Spawn a new thread with its own runtime to avoid block_on issues when called
    // from within an async context (queue executor)
    let capsolver_key_thread = capsolver_key.clone();
    let themes_thread = themes.clone();
    let existing_keywords_thread = existing_keywords.clone();
    let agent_competitors_thread = agent_competitors.clone();
    let coverage_clusters_thread = coverage_clusters.clone();
    let seo_provider_thread = seo_provider.to_string();
    let project_path_thread = project_path.to_string();
    
    let thread_result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async move {
            let env = crate::config::env_resolver::EnvResolver::new(&project_path_thread);
            let provider = crate::seo::resolve_provider(&seo_provider_thread, &env)?;
            let is_dataforseo = provider.name() == "dataforseo";

            let mut candidates: Vec<Candidate> = vec![];
            let mut seen: HashSet<String> = HashSet::new();

            if is_dataforseo {
                // ── DataForSEO path: keyword_suggestions returns volume + KD + intent ──
                for theme in &themes_thread {
                    log::info!("[keyword_research_native] fetching DataForSEO keyword suggestions for theme '{}'", theme);
                    match provider.keyword_ideas(theme, "us", "google").await {
                        Ok(result) => {
                            for idea in result.ideas.iter().chain(result.question_ideas.iter()) {
                                let kw_lower = idea.keyword.to_lowercase();
                                if existing_keywords_thread.contains(&kw_lower) {
                                    continue;
                                }
                                if seen.contains(&kw_lower) {
                                    continue;
                                }
                                seen.insert(kw_lower);
                                candidates.push(Candidate {
                                    keyword: idea.keyword.clone(),
                                    source_theme: theme.clone(),
                                    is_question: idea.idea_type == "question",
                                    volume: idea.volume_exact,
                                    kd: idea.kd,
                                    intent: idea.intent.clone(),
                                });
                            }
                            log::info!(
                                "[keyword_research_native] theme '{}' → {} total candidates (DataForSEO)",
                                theme, candidates.len()
                            );
                        }
                        Err(e) => {
                            log::warn!("[keyword_research_native] DataForSEO keyword_ideas failed for '{}': {}", theme, e);
                        }
                    }
                }
            } else {
                // ── Ahrefs/Google Autocomplete path (legacy) ──────────────────────
                for theme in &themes_thread {
                    log::info!("[keyword_research_native] fetching Google autocomplete ideas for theme '{}'", theme);
                    match crate::seo::google_autocomplete::get_keyword_ideas_google(theme, "us", "Google").await {
                        Ok(result) => {
                            for suggestion in result.ideas.iter().chain(result.question_ideas.iter()) {
                                let kw_lower = suggestion.keyword.to_lowercase();
                                if existing_keywords_thread.contains(&kw_lower) {
                                    continue;
                                }
                                if seen.contains(&kw_lower) {
                                    continue;
                                }
                                seen.insert(kw_lower);
                                candidates.push(Candidate {
                                    keyword: suggestion.keyword.clone(),
                                    source_theme: theme.clone(),
                                    is_question: suggestion.suggestion_type == crate::seo::google_autocomplete::SuggestionType::Question,
                                    volume: None,
                                    kd: None,
                                    intent: None,
                                });
                            }
                            log::info!("[keyword_research_native] theme '{}' → {} total candidates", theme, candidates.len());
                        }
                        Err(e) => {
                            log::warn!("[keyword_research_native] Google autocomplete failed for '{}': {}", theme, e);
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
                }
            }

            let pre_filter_count = candidates.len();
            
            // Step 2 — Coverage-aware filtering
            if !coverage_clusters_thread.is_empty() {
                let pre_gap_count = candidates.len();
                candidates = filter_by_coverage_gap(
                    candidates,
                    &coverage_clusters_thread,
                    &existing_keywords_thread,
                );
                log::info!(
                    "[keyword_research_native] coverage gap filter: {} → {} candidates",
                    pre_gap_count,
                    candidates.len()
                );
            }

            // Step 3 — Volume filter (only meaningful when we have volume data)
            const MIN_VOLUME: i64 = 50;
            let pre_volume_count = candidates.len();
            candidates = candidates
                .into_iter()
                .filter(|c| {
                    c.volume
                        .map(|v| v >= MIN_VOLUME)
                        .unwrap_or(false) // reject unknown volumes
                })
                .collect();

            log::info!(
                "[keyword_research_native] volume filter: {} → {} candidates (dropped {} below {} or unknown)",
                pre_volume_count,
                candidates.len(),
                pre_volume_count - candidates.len(),
                MIN_VOLUME,
            );

            // If DataForSEO returned zero candidates after volume filter, that's the real count.
            // For Ahrefs path (no volume data), volume filter drops everything — skip it and use all candidates.
            if !is_dataforseo && candidates.is_empty() && pre_volume_count > 0 {
                log::info!("[keyword_research_native] Ahrefs path: no volume data available, using all {} candidates", pre_volume_count);
                // Re-filter without volume requirement for Ahrefs path
                candidates = Vec::new(); // will rebuild below
            }

            // Rebuild candidates for Ahrefs path if volume filter emptied the list
            if !is_dataforseo && candidates.is_empty() && pre_filter_count > 0 {
                // Re-run without volume filter
                let mut seen2: HashSet<String> = HashSet::new();
                for theme in &themes_thread {
                    match crate::seo::google_autocomplete::get_keyword_ideas_google(theme, "us", "Google").await {
                        Ok(result) => {
                            for suggestion in result.ideas.iter().chain(result.question_ideas.iter()) {
                                let kw_lower = suggestion.keyword.to_lowercase();
                                if existing_keywords_thread.contains(&kw_lower) || seen2.contains(&kw_lower) {
                                    continue;
                                }
                                seen2.insert(kw_lower);
                                candidates.push(Candidate {
                                    keyword: suggestion.keyword.clone(),
                                    source_theme: theme.clone(),
                                    is_question: suggestion.suggestion_type == crate::seo::google_autocomplete::SuggestionType::Question,
                                    volume: None,
                                    kd: None,
                                    intent: None,
                                });
                            }
                        }
                        Err(_) => {}
                    }
                }
                if !coverage_clusters_thread.is_empty() {
                    candidates = filter_by_coverage_gap(candidates, &coverage_clusters_thread, &existing_keywords_thread);
                }
            }

            // Smart sample: limit to 50 for Ahrefs KD checks; DataForSEO can take all
            let sampled = if is_dataforseo {
                candidates // already have KD data, no API calls needed
            } else {
                smart_sample_candidates(candidates, 50)
            };

            log::info!(
                "[keyword_research_native] {} keywords for analysis ({} question keywords)",
                sampled.len(),
                sampled.iter().filter(|c| c.is_question).count()
            );

            let mut with_data_results: Vec<serde_json::Value> = vec![];
            let mut no_data_results: Vec<serde_json::Value> = vec![];
            let mut analyzed_count = 0usize;

            // Extract capsolver key once for Ahrefs path
            let capsolver_key_str = capsolver_key_thread.unwrap_or_default();

            if is_dataforseo {
                // DataForSEO: KD + volume already present in candidates
                for candidate in &sampled {
                    analyzed_count += 1;
                    let has_data = candidate.kd.is_some() && candidate.volume.is_some();
                    let entry = serde_json::json!({
                        "keyword": candidate.keyword,
                        "difficulty": candidate.kd,
                        "volume": candidate.volume,
                        "intent": candidate.intent,
                        "has_data": has_data,
                    });
                    if has_data {
                        with_data_results.push(entry);
                    } else {
                        no_data_results.push(entry);
                    }
                }
            } else {
                // Ahrefs: need individual KD checks via CapSolver
                for candidate in &sampled {
                    analyzed_count += 1;
                    let kw = &candidate.keyword;

                    match crate::seo::keywords::get_keyword_difficulty(
                        &capsolver_key_str, kw, "us",
                    ).await {
                        Ok(kd) => {
                            let has_data = kd.difficulty.is_some() && !kd.last_update.is_empty();
                            let vol = candidate.volume;
                            let top_traffic = best_serp_metric(kd.serp.iter().map(|s| s.traffic));
                            let top_volume = best_serp_metric(kd.serp.iter().map(|s| s.top_volume));
                            let entry = serde_json::json!({
                                "keyword": kw,
                                "difficulty": kd.difficulty,
                                "volume": vol,
                                "traffic": top_traffic,
                                "topVolume": top_volume,
                                "shortage": kd.shortage,
                                "has_data": has_data,
                                "serp_count": kd.serp.len(),
                                "top_result": kd.serp.first().map(|s| s.url.as_str()).unwrap_or(""),
                                "last_update": kd.last_update,
                            });
                            log::info!(
                                "[keyword_research_native] '{}' kd={:?} vol={:?} has_data={}",
                                kw, kd.difficulty, vol, has_data,
                            );
                            if has_data {
                                with_data_results.push(entry);
                            } else {
                                no_data_results.push(entry);
                            }
                        }
                        Err(e) => {
                            log::warn!("[keyword_research_native] difficulty failed for '{}': {}", kw, e);
                            no_data_results.push(serde_json::json!({
                                "keyword": kw,
                                "difficulty": serde_json::Value::Null,
                                "volume": candidate.volume,
                                "has_data": false,
                            }));
                        }
                    }
                }
            }

            // Competitor insights (skip for DataForSEO — uses CapSolver/Ahrefs)
            let mut competitor_insights: Vec<crate::models::research::CompetitorInsight> = vec![];
            if !is_dataforseo {
                for domain in &agent_competitors_thread {
                    match crate::seo::traffic::check_traffic(
                        &capsolver_key_str, domain, "subdomains", "None",
                    ).await {
                        Ok(traffic) => {
                            let top_keywords = traffic.top_keywords
                                .into_iter()
                                .take(5)
                                .map(|tk| crate::models::research::CompetitorTopKeyword {
                                    keyword: tk.keyword.unwrap_or_default(),
                                    traffic: tk.traffic,
                                    position: tk.position,
                                })
                                .collect();
                            competitor_insights.push(crate::models::research::CompetitorInsight {
                                domain: traffic.domain,
                                traffic_monthly_avg: traffic.traffic.traffic_monthly_avg,
                                top_keywords,
                            });
                        }
                        Err(e) => {
                            log::warn!("[keyword_research_native] competitor traffic failed for '{}': {}", domain, e);
                        }
                    }
                }
            }

            Ok::<_, crate::error::Error>((with_data_results, no_data_results, analyzed_count, pre_filter_count, competitor_insights))
        })
    }).join();

    let (with_data_results, no_data_results, analyzed_count, total_candidates, competitor_insights) = match thread_result {
        Ok(Ok(result)) => result,
        Ok(Err(e)) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Keyword research failed: {}", e),
                output: None,
            };
        }
        Err(_) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: "Keyword research thread panicked".to_string(),
                output: None,
            };
        }
    };

    if total_candidates == 0 {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "No new keyword ideas found for themes: {}. All suggestions may already be covered.",
                themes.join(", ")
            ),
            output: None,
        };
    }

    // Present with-data results first, then append no-data results so the
    // final selection agent can see the full sampled set (up to max_api_calls).
    let mut difficulty_results = with_data_results;
    difficulty_results.extend(no_data_results);

    log::info!(
        "[keyword_research_native] {} with data, {} total shown (checked {} keywords)",
        difficulty_results.iter().filter(|r| r["has_data"] == true).count(),
        difficulty_results.len(),
        analyzed_count,
    );

    // total_candidates already captured from pre_filter_count
    let with_data_count = difficulty_results.iter().filter(|r| r["has_data"] == true).count();
    
    // Build typed output contract with intent classification
    let keywords: Vec<crate::models::research::ScoredKeyword> = difficulty_results
        .into_iter()
        .map(|r| {
            let keyword = r["keyword"].as_str().unwrap_or("").to_string();
            
            // Use DataForSEO intent if available, otherwise classify by pattern
            let (intent, confidence) = if let Some(api_intent) = r["intent"].as_str() {
                (api_intent.to_string(), 90.0)
            } else {
                let (i, c) = crate::engine::exec::intent_classifier::classify_intent(&keyword);
                (i.as_str().to_string(), c)
            };
            
            crate::models::research::ScoredKeyword {
                keyword,
                volume: r["volume"].as_i64(),
                kd: r["difficulty"].as_f64(),
                intent: Some(intent),
                intent_confidence: Some(confidence),
                traffic: r["traffic"].as_f64(),
                has_data: r["has_data"].as_bool(),
            }
        })
        .collect();
    
    let output = crate::models::research::KeywordPipelineOutput {
        keywords,
        themes: themes.clone(),
        competitors: agent_competitors,
        competitor_insights,
        total_candidates,
        with_data_count,
    };

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Keyword research complete ({} themes, {} candidates, {} analyzed)",
            themes.len(), total_candidates, analyzed_count
        ),
        output: Some(serde_json::to_string_pretty(&output).unwrap_or_default()),
    }
}

// ─── Theme string cleaning ──────────────────────────────────────────────────

/// Strip markdown heading markers (`###`), resolve `Cluster N: Topic` → `"Topic"`,
/// and return `None` for bare cluster labels like `"Cluster 7"` or `"### Cluster 9"`
/// that carry no real search topic.
fn clean_theme_str(raw: &str) -> Option<String> {
    let s = raw.trim().trim_start_matches('#').trim();
    if s.is_empty() {
        return None;
    }

    let s: String = if let Some(colon_pos) = s.find(':') {
        // "Cluster 4: SEO Tools (PLANNED)" → "SEO Tools"
        let after = s[colon_pos + 1..].trim();
        after.split('(').next().unwrap_or(after).trim().to_string()
    } else {
        // Strip trailing parenthetical FIRST, then check for bare cluster label.
        // "Cluster 7 (PLANNED)" → "Cluster 7" → bare → None.
        let s_no_paren = s.split('(').next().unwrap_or(s).trim();
        let words: Vec<&str> = s_no_paren.split_whitespace().collect();
        let is_bare_cluster = words.len() <= 2
            && words
                .first()
                .map(|w| w.eq_ignore_ascii_case("cluster"))
                .unwrap_or(false);
        if is_bare_cluster {
            return None;
        }
        s_no_paren.to_string()
    };

    if s.is_empty() || s.len() <= 2 {
        None
    } else {
        Some(s)
    }
}

/// Parse comma-/newline-separated themes from a task description string,
/// applying the same cleaning rules as brief parsing.
///
/// Returns an empty vec when the description contains only junk cluster labels.
pub(crate) fn parse_desc_themes(raw: &str) -> Vec<String> {
    raw.lines()
        .flat_map(|line| line.split(','))
        .filter_map(clean_theme_str)
        .collect()
}

// ─── Theme auto-derivation ────────────────────────────────────────────────────

/// Try to derive keyword themes from existing project configuration files.
///
/// Priority order:
///   1. `project.md` — consolidated project config (PLANNED clusters, Identity)
///   2. `*seo_content_brief*.md` — legacy: PLANNED cluster topics (🎯) and gap cluster names
///   3. `*project_summary*.md`   — legacy: Content Pillar names
///   4. `articles.json`          — unique existing target_keywords (as baseline coverage)
pub(crate) fn derive_themes_from_project(automation_dir: &std::path::Path) -> Vec<String> {
    // Primary: consolidated project.md
    let project_md = automation_dir.join("project.md");
    if project_md.exists() {
        log::info!("[keyword_research] using project.md: {:?}", project_md);
        let themes = extract_from_brief(&project_md);
        if !themes.is_empty() {
            return themes;
        }
        // Also try summary extraction (for Content Clusters & Identity sections)
        let themes = extract_from_summary(&project_md);
        if !themes.is_empty() {
            return themes;
        }
    }

    // Legacy fallbacks
    if let Some(brief) = find_file_by_suffix(automation_dir, "seo_content_brief.md") {
        log::info!("[keyword_research] using brief: {:?}", brief);
        let themes = extract_from_brief(&brief);
        if !themes.is_empty() {
            return themes;
        }
    }

    if let Some(summary) = find_file_by_suffix(automation_dir, "project_summary.md") {
        log::info!("[keyword_research] using summary: {:?}", summary);
        let themes = extract_from_summary(&summary);
        if !themes.is_empty() {
            return themes;
        }
    }

    let articles_json = automation_dir.join("articles.json");
    if articles_json.exists() {
        let themes = extract_from_articles(&articles_json);
        if !themes.is_empty() {
            return themes;
        }
    }

    vec![]
}

/// Find the first file in `dir` whose name contains `suffix` (case-insensitive).
fn find_file_by_suffix(dir: &std::path::Path, suffix: &str) -> Option<std::path::PathBuf> {
    let exact = dir.join(suffix);
    if exact.exists() {
        return Some(exact);
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return None };
    let suffix_lower = suffix.to_lowercase();
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.to_lowercase().contains(&suffix_lower))
                .unwrap_or(false)
        })
}

/// Extract themes from `seo_content_brief.md`.
fn extract_from_brief(path: &std::path::Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else { return vec![] };

    let planned_items: Vec<String> = content
        .lines()
        .filter(|l| l.contains('🎯'))
        .filter_map(|l| {
            // Strip all decorators first, then delegate to clean_theme_str which:
            // - strips '#' heading markers
            // - resolves "Cluster N: Topic (annotation)" → "Topic"
            // - rejects bare "Cluster N" / "### Cluster N" labels
            let stripped = l.trim()
                .trim_start_matches("- [ ] ")
                .trim_start_matches("- [x] ")
                .trim_start_matches("- ")
                .trim_start_matches("* ")
                .replace('🎯', "")
                .replace("**", "")
                .trim()
                .to_string();
            clean_theme_str(&stripped)
        })
        .take(8)
        .collect();

    if !planned_items.is_empty() {
        return planned_items;
    }

    let planned_clusters: Vec<String> = content
        .lines()
        .filter(|l| l.contains("PLANNED") && l.starts_with("###"))
        .filter_map(clean_theme_str)
        .take(8)
        .collect();

    if !planned_clusters.is_empty() {
        return planned_clusters;
    }

    content
        .lines()
        .filter(|l| l.starts_with("### Cluster"))
        .filter_map(clean_theme_str)
        .take(6)
        .collect()
}

/// Extract content pillar topics from `project_summary.md`.
fn extract_from_summary(path: &std::path::Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else { return vec![] };

    let mut themes: Vec<String> = content
        .lines()
        .filter(|l| {
            let lower = l.to_lowercase();
            lower.contains("pillar") && l.starts_with("###")
        })
        .map(|l| {
            let s = l.trim_start_matches('#').trim();
            let s = s.split(':').nth(1).unwrap_or(s).trim();
            s.split('(').next().unwrap_or(s).trim().to_string()
        })
        .filter(|s| !s.is_empty())
        .take(6)
        .collect();

    if themes.is_empty() {
        let mut in_keywords = false;
        for line in content.lines() {
            if line.contains("Search Keywords") {
                in_keywords = true;
                continue;
            }
            if in_keywords {
                if line.trim().starts_with('-') || line.trim().starts_with('*') {
                    let kw = line.trim()
                        .trim_start_matches('-')
                        .trim_start_matches('*')
                        .trim()
                        .trim_matches('"')
                        .to_string();
                    if !kw.is_empty() {
                        themes.push(kw);
                    }
                    if themes.len() >= 8 { break; }
                } else if line.trim().is_empty() || line.starts_with('#') {
                    in_keywords = false;
                }
            }
        }
    }

    themes
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// Write `content` to `<tmp>/ps_kw_test_<name>.md` and return the path.
    fn write_tmp(name: &str, content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("ps_kw_test_{name}.md"));
        fs::write(&path, content).unwrap();
        path
    }

    // ── extract_from_brief: 🎯 items ─────────────────────────────────────────

    #[test]
    fn brief_goal_markers_extract_topic_names() {
        let path = write_tmp("brief_goals", "\
## Gap Analysis\n\
- [ ] 🎯 SEO Tools for Beginners (PLANNED)\n\
- [ ] 🎯 Content Marketing Strategy\n\
- No marker here\n");
        let themes = extract_from_brief(&path);
        assert!(
            themes.contains(&"SEO Tools for Beginners".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Content Marketing Strategy".to_string()),
            "got: {themes:?}"
        );
        // Non-goal lines must not appear.
        assert!(!themes.iter().any(|t| t.contains("No marker")));
    }

    #[test]
    fn brief_goal_heading_cluster_style_extracts_topic() {
        // Exact format from the failing brief: "### Cluster N: Topic (annotation) 🎯"
        // Old code returned ["### Cluster 7", "### Cluster 8"] — sending markdown
        // heading tokens straight to Ahrefs.
        let path = write_tmp("brief_goals_heading", "\
### Cluster 7: Risk Management (EMERGING) 🎯\n\
### Cluster 8: Advanced Topics (EMERGING) 🎯\n\
**Cluster 9: IRA / Retirement Account Options (NEW) 🎯**\n\
**Cluster 10: Protective Put / Portfolio Hedging (NEW) 🎯**\n");
        let themes = extract_from_brief(&path);
        assert!(!themes.iter().any(|t| t.contains('#')), "no # markers: {themes:?}");
        assert!(!themes.iter().any(|t| t.starts_with("Cluster ")), "no bare cluster labels: {themes:?}");
        assert!(themes.contains(&"Risk Management".to_string()), "got: {themes:?}");
        assert!(themes.contains(&"Advanced Topics".to_string()), "got: {themes:?}");
        assert!(themes.contains(&"IRA / Retirement Account Options".to_string()), "got: {themes:?}");
        assert!(themes.contains(&"Protective Put / Portfolio Hedging".to_string()), "got: {themes:?}");
    }

    // ── extract_from_brief: PLANNED clusters ──────────────────────────────────

    #[test]
    fn brief_planned_cluster_with_colon_extracts_topic() {
        let path = write_tmp("brief_planned", "\
### Cluster 4: Advanced SEO Tactics (PLANNED)\n\
### Cluster 5: Link Building (PLANNED)\n");
        let themes = extract_from_brief(&path);
        assert!(
            themes.contains(&"Advanced SEO Tactics".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Link Building".to_string()),
            "got: {themes:?}"
        );
    }

    #[test]
    fn brief_planned_heading_without_colon_is_filtered_out() {
        // "### Cluster 7 (PLANNED)" has no colon → no real topic → must be dropped.
        let path = write_tmp("brief_planned_no_colon", "### Cluster 7 (PLANNED)\n");
        let themes = extract_from_brief(&path);
        assert!(themes.is_empty(), "bare cluster label should be filtered: {themes:?}");
    }

    // ── extract_from_brief: all-clusters fallback ─────────────────────────────

    #[test]
    fn brief_cluster_headings_without_planned_uses_last_resort() {
        let path = write_tmp("brief_clusters", "\
### Cluster 1: On-Page SEO\n\
### Cluster 2: Technical SEO\n");
        let themes = extract_from_brief(&path);
        assert!(
            themes.contains(&"On-Page SEO".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Technical SEO".to_string()),
            "got: {themes:?}"
        );
    }

    #[test]
    fn brief_empty_file_returns_empty() {
        let path = write_tmp("brief_empty", "");
        assert!(extract_from_brief(&path).is_empty());
    }

    #[test]
    fn brief_missing_file_returns_empty() {
        assert!(extract_from_brief(std::path::Path::new("/nonexistent/ps_kw_missing.md")).is_empty());
    }

    // ── extract_from_summary ──────────────────────────────────────────────────

    #[test]
    fn summary_pillar_headings_extract_names() {
        let path = write_tmp("summary_pillars", "\
### Pillar 1: Keyword Research\n\
### Pillar 2: Content Creation\n\
## Other section\n");
        let themes = extract_from_summary(&path);
        assert!(
            themes.contains(&"Keyword Research".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"Content Creation".to_string()),
            "got: {themes:?}"
        );
    }

    #[test]
    fn summary_search_keywords_list_fallback() {
        let path = write_tmp("summary_keywords", "\
## Search Keywords\n\
- seo tips\n\
- content strategy\n\
## Other\n");
        let themes = extract_from_summary(&path);
        assert!(
            themes.contains(&"seo tips".to_string()),
            "got: {themes:?}"
        );
        assert!(
            themes.contains(&"content strategy".to_string()),
            "got: {themes:?}"
        );
    }

    #[test]
    fn summary_empty_file_returns_empty() {
        let path = write_tmp("summary_empty", "");
        assert!(extract_from_summary(&path).is_empty());
    }

    // ── find_file_by_suffix ───────────────────────────────────────────────────

    #[test]
    fn find_file_locates_by_partial_name() {
        let dir = std::env::temp_dir().join("ps_kw_find_test");
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("my_seo_content_brief_v2.md");
        fs::write(&file, "content").unwrap();

        let found = find_file_by_suffix(&dir, "seo_content_brief");
        assert!(found.is_some(), "expected to find file");

        fs::remove_dir_all(&dir).ok();
    }

    // ── clean_theme_str ──────────────────────────────────────────────────────────────

    #[test]
    fn clean_theme_markdown_heading_no_colon_rejected() {
        // Exact inputs from the log: ["### Cluster 7", "### Cluster 8", ...]
        assert_eq!(clean_theme_str("### Cluster 7"), None);
        assert_eq!(clean_theme_str("### Cluster 8"), None);
    }

    #[test]
    fn clean_theme_bare_cluster_label_rejected() {
        assert_eq!(clean_theme_str("Cluster 9"), None);
        assert_eq!(clean_theme_str("Cluster 10"), None);
    }

    #[test]
    fn clean_theme_heading_with_colon_extracts_topic() {
        assert_eq!(
            clean_theme_str("### Cluster 4: SEO Tools"),
            Some("SEO Tools".to_string())
        );
    }

    #[test]
    fn clean_theme_strips_planned_annotation() {
        assert_eq!(
            clean_theme_str("### Cluster 5: Link Building (PLANNED)"),
            Some("Link Building".to_string())
        );
    }

    #[test]
    fn clean_theme_plain_topic_passes_through() {
        assert_eq!(
            clean_theme_str("content marketing"),
            Some("content marketing".to_string())
        );
    }

    #[test]
    fn clean_theme_empty_returns_none() {
        assert_eq!(clean_theme_str(""), None);
        assert_eq!(clean_theme_str("  "), None);
    }

    // ── parse_desc_themes ──────────────────────────────────────────────────────────

    #[test]
    fn parse_desc_exact_failing_log_payload_returns_empty() {
        // This is the exact string that caused the CapSolver failure.
        // After the fix it must produce zero themes so the fallback kicks in.
        let raw = "### Cluster 7, ### Cluster 8, Cluster 9, Cluster 10";
        assert!(
            parse_desc_themes(raw).is_empty(),
            "bare cluster labels must all be filtered out"
        );
    }

    #[test]
    fn parse_desc_topics_with_colon_extracted() {
        let raw = "### Cluster 4: SEO Tools, ### Cluster 5: Link Building";
        let themes = parse_desc_themes(raw);
        assert_eq!(themes, vec!["SEO Tools", "Link Building"]);
    }

    #[test]
    fn parse_desc_plain_comma_list_passes_through() {
        let raw = "seo tools, content marketing, link building";
        let themes = parse_desc_themes(raw);
        assert_eq!(themes, vec!["seo tools", "content marketing", "link building"]);
    }

    #[test]
    fn parse_desc_newline_separated_works() {
        let raw = "seo tools\ncontent marketing\n";
        assert_eq!(parse_desc_themes(raw), vec!["seo tools", "content marketing"]);
    }

    #[test]
    fn parse_desc_mixed_valid_and_bare_clusters() {
        // If a description has some good themes AND some bare cluster junk,
        // only the good ones should survive.
        let raw = "### Cluster 7, SEO Automation, Cluster 9";
        let themes = parse_desc_themes(raw);
        assert_eq!(themes, vec!["SEO Automation"]);
    }
    // ── derive_themes_from_project integration ────────────────────────────────

    #[test]
    fn derive_themes_real_brief_format_returns_clean_topics() {
        // Exact content structure from the brief that caused the CapSolver failure.
        // Verifies the full stack: find_file → extract_from_brief → clean_theme_str.
        let dir = std::env::temp_dir().join("ps_kw_derive_real");
        fs::create_dir_all(&dir).unwrap();

        let brief = "\
## Existing Clusters\n\
### Cluster 7: Risk Management (EMERGING) \u{1f3af}\n\
**Pillar Content:** Risk management principles\n\
\n\
### Cluster 8: Advanced Topics (EMERGING) \u{1f3af}\n\
**Pillar Content:** Advanced strategies\n\
\n\
### New Clusters Discovered\n\
**Cluster 9: IRA / Retirement Account Options (NEW) \u{1f3af}**\n\
\n\
**Cluster 10: Protective Put / Portfolio Hedging (NEW) \u{1f3af}**\n";

        fs::write(dir.join("seo_content_brief.md"), brief).unwrap();

        let themes = derive_themes_from_project(&dir);

        assert!(!themes.is_empty(), "should derive themes, got none");
        assert!(
            !themes.iter().any(|t| t.contains('#')),
            "no markdown heading markers in themes: {themes:?}"
        );
        assert!(
            !themes.iter().any(|t| {
                let w: Vec<_> = t.split_whitespace().collect();
                w.len() <= 2 && w.first().map(|s| s.eq_ignore_ascii_case("cluster")).unwrap_or(false)
            }),
            "no bare 'Cluster N' labels in themes: {themes:?}"
        );
        assert!(themes.contains(&"Risk Management".to_string()), "got: {themes:?}");
        assert!(themes.contains(&"Advanced Topics".to_string()), "got: {themes:?}");
        assert!(themes.contains(&"IRA / Retirement Account Options".to_string()), "got: {themes:?}");
        assert!(themes.contains(&"Protective Put / Portfolio Hedging".to_string()), "got: {themes:?}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn derive_themes_bare_cluster_only_brief_returns_empty() {
        // If a brief has ONLY bare "### Cluster N" headings (no colon → no topic),
        // derive_themes should return empty so the executor fails with a clear
        // "No themes found" message instead of sending junk strings to Ahrefs.
        let dir = std::env::temp_dir().join("ps_kw_derive_bare");
        fs::create_dir_all(&dir).unwrap();

        let brief = "### Cluster 7 (PLANNED)\n### Cluster 8 (PLANNED)\nCluster 9\nCluster 10\n";
        fs::write(dir.join("seo_content_brief.md"), brief).unwrap();

        let themes = derive_themes_from_project(&dir);

        assert!(
            themes.is_empty(),
            "bare cluster labels must produce empty themes (not sent to Ahrefs): {themes:?}"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn derive_themes_without_project_summary_uses_brief_only() {
        // Regression: missing project_summary.md must not crash or block theme derivation.
        let dir = std::env::temp_dir().join("ps_kw_derive_no_summary");
        fs::create_dir_all(&dir).unwrap();

        fs::write(
            dir.join("seo_content_brief.md"),
            "### Cluster 1: Protective Put (PLANNED)\n",
        )
        .unwrap();

        let themes = derive_themes_from_project(&dir);
        assert_eq!(themes, vec!["Protective Put".to_string()]);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn derive_themes_missing_brief_and_summary_returns_empty() {
        // Regression: no brief + no summary should fail gracefully with empty themes.
        let dir = std::env::temp_dir().join("ps_kw_derive_missing_all");
        fs::create_dir_all(&dir).unwrap();

        let themes = derive_themes_from_project(&dir);
        assert!(themes.is_empty(), "expected empty themes, got {themes:?}");

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parse_agent_themes_handles_fenced_json_with_tool_logs() {
        let raw = r#"● Read seo_content_brief.md
 │ .github/automation/seo_content_brief.md
 └ 1 line read

```json
{
  "themes": ["Protective Put", "IRA Options", "Portfolio Hedging"]
}
```
"#;

        let task = crate::models::task::Task {
            id: "t1".to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Manual,
            agent_policy: crate::models::task::AgentPolicy::Optional,
            title: None,
            description: None,
            project_id: "p1".to_string(),
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "research_seed_extraction".to_string(),
                path: None,
                artifact_type: Some("agentic".to_string()),
                source: Some("agentic".to_string()),
                content: Some(raw.to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let parsed = parse_seed_extraction_artifact(&task);
        assert_eq!(parsed.themes, vec!["Protective Put", "IRA Options", "Portfolio Hedging"]);
    }

    // Note: List fallback ("1. Theme") removed - we now require JSON output contract.
    // The deterministic step expects {"themes": [...]} format from Step 1.

    #[test]
    fn parse_agent_themes_supports_array_json_contract() {
        let task = crate::models::task::Task {
            id: "t3".to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Manual,
            agent_policy: crate::models::task::AgentPolicy::Optional,
            title: None,
            description: None,
            project_id: "p1".to_string(),
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "research_seed_extraction".to_string(),
                path: None,
                artifact_type: Some("agentic".to_string()),
                source: Some("agentic".to_string()),
                content: Some("[\"Protective Put\", \"IRA Options\"]".to_string()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let parsed = parse_seed_extraction_artifact(&task);
        assert_eq!(parsed.themes, vec!["Protective Put", "IRA Options"]);
    }
    // ── find_file_by_suffix ──────────────────────────────────────────────────────

    #[test]
    fn find_file_exact_match_returned_first() {
        let dir = std::env::temp_dir().join("ps_kw_find_exact");
        fs::create_dir_all(&dir).unwrap();
        let exact = dir.join("seo_content_brief.md");
        fs::write(&exact, "exact").unwrap();

        let found = find_file_by_suffix(&dir, "seo_content_brief.md");
        assert_eq!(found.unwrap(), exact);

        fs::remove_dir_all(&dir).ok();
    }
}

/// Extract unique target_keywords from `articles.json` as theme seeds.
fn extract_from_articles(path: &std::path::Path) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else { return vec![] };
    let Ok(articles) = serde_json::from_str::<Vec<serde_json::Value>>(&content) else { return vec![] };

    let mut seen = std::collections::HashSet::new();
    let mut themes = Vec::new();

    for article in &articles {
        if let Some(kw) = article.get("target_keyword").and_then(|v| v.as_str()) {
            if kw.is_empty() { continue; }
            let short: String = kw.split_whitespace().take(3).collect::<Vec<_>>().join(" ");
            let lower = short.to_lowercase();
            if seen.insert(lower.clone()) {
                themes.push(short);
            }
        }
        if themes.len() >= 6 { break; }
    }

    themes
}

// ─── Integration tests (require live credentials) ─────────────────────────────
//
// These tests call real external APIs (CapSolver → Ahrefs).
// They are marked `#[ignore]` so normal `cargo test` skips them.
//
// Run with:
//   CAPSOLVER_API_KEY=<key> cargo test --lib keyword_research_integration -- --ignored --nocapture
//
// Requirements:
//   - CAPSOLVER_API_KEY must be set (in env or ~/.config/automation/secrets.env)
//   - Network access to CapSolver and Ahrefs must be available

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::engine::workflows::StepResult;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Build a unique temp directory for a test run.
    fn unique_temp_project_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{prefix}_{nanos}"))
    }

    /// Helper: build a minimal fake repo at `dir` with:
    ///   - `.github/automation/seo_content_brief.md` containing `theme`
    ///   - `.github/automation/articles.json` (empty array)
    fn setup_dummy_project(dir: &std::path::Path, theme: &str) {
        let automation = dir.join(".github").join("automation");
        fs::create_dir_all(&automation).unwrap();

        let brief = format!("## Clusters\n\n### Cluster 1: {theme} (PLANNED)\n");
        fs::write(automation.join("seo_content_brief.md"), brief).unwrap();
        fs::write(automation.join("articles.json"), "[]").unwrap();
    }

    /// Run the full native keyword research flow against a temp dummy project.
    fn run_dummy_project_flow(theme: &str) -> StepResult {
        let dir = unique_temp_project_dir("ps_kw_integration_test");
        setup_dummy_project(&dir, theme);

        let project_path = dir.to_string_lossy().to_string();

        let task = crate::models::task::Task {
            id: "integration-test".to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: crate::models::task::TaskStatus::Todo,
            priority: crate::models::task::Priority::Medium,
            execution_mode: crate::models::task::ExecutionMode::Manual,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Integration test".to_string()),
            description: None,
            project_id: "test".to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        // Need a tokio runtime because exec_keyword_research_native uses block_on.
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(async {
            tokio::task::spawn_blocking(move || exec_keyword_research_native(&task, &project_path, "ahrefs"))
                .await
                .unwrap()
        });

        fs::remove_dir_all(&dir).ok();
        result
    }

    /// Full end-to-end: brief → theme extraction → CapSolver → Ahrefs keyword ideas
    /// → difficulty analysis → structured JSON output.
    ///
    /// This is what the "Run" button triggers. If it fails here, it will fail in the app.
    #[test]
    #[ignore = "calls live CapSolver + Ahrefs APIs; run with --ignored"]
    fn full_keyword_research_pipeline_single_theme() {
        // Resolve CAPSOLVER_API_KEY the same way the app does.
        let capsolver_key = {
            use crate::config::env_resolver::EnvResolver;
            // Use a throwaway project path — we only need the secrets resolution.
            let env = EnvResolver::new("/tmp").build_env(std::collections::HashMap::new());
            env.get("CAPSOLVER_API_KEY")
                .cloned()
                .unwrap_or_default()
        };

        if capsolver_key.is_empty() {
            eprintln!("SKIP: CAPSOLVER_API_KEY not set — set it in ~/.config/automation/secrets.env");
            return;
        }

        // Build and run against a minimal throwaway dummy project.
        let result = run_dummy_project_flow("options risk management");

        eprintln!("=== StepResult ===");
        eprintln!("success: {}", result.success);
        eprintln!("message: {}", result.message);
        if let Some(output) = &result.output {
            let v: serde_json::Value = serde_json::from_str(output).unwrap_or_default();
            eprintln!("themes:   {:?}", v["themes"]);
            eprintln!("candidates: {}", v["total_candidates"]);
            eprintln!("analyzed:   {}", v["difficulty"]["total"]);
            eprintln!("results:    {}", v["difficulty"]["results"]);
        }

        if !result.success {
            assert!(
                result.message.contains("No new keyword ideas found")
                    || result.message.contains("Failed to fetch keyword ideas")
                    || result.message.contains("No themes found")
                    || result.message.contains("CAPSOLVER"),
                "unexpected pipeline failure: {}",
                result.message
            );
            return;
        }

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap_or("{}")).unwrap();

        // Themes must be clean (no # markers, no bare "Cluster N").
        let themes = output["themes"].as_array().unwrap();
        assert!(!themes.is_empty(), "no themes derived");
        for t in themes {
            let s = t.as_str().unwrap();
            assert!(!s.contains('#'), "theme contains # marker: {s}");
            assert!(
                !(s.split_whitespace().count() <= 2
                    && s.split_whitespace().next().map(|w| w.eq_ignore_ascii_case("cluster")).unwrap_or(false)),
                "bare cluster label sent to API: {s}"
            );
        }

        // Must have analysed at least one keyword with KD data.
        let results = output["difficulty"]["results"].as_array().unwrap();
        assert!(!results.is_empty(), "no difficulty results returned");

    }

    /// Lightweight dummy-project smoke flow that still exercises the full live pipeline.
    #[test]
    #[ignore = "calls live CapSolver + Ahrefs APIs; run with --ignored"]
    fn keyword_research_dummy_project_smoke_flow() {
        let capsolver_key = {
            use crate::config::env_resolver::EnvResolver;
            let env = EnvResolver::new("/tmp").build_env(std::collections::HashMap::new());
            env.get("CAPSOLVER_API_KEY")
                .cloned()
                .unwrap_or_default()
        };

        if capsolver_key.is_empty() {
            eprintln!("SKIP: CAPSOLVER_API_KEY not set — set it in ~/.config/automation/secrets.env");
            return;
        }

        let result = run_dummy_project_flow("coffee roasting profiles");
        eprintln!("smoke flow success: {}", result.success);
        eprintln!("smoke flow message: {}", result.message);

        if !result.success {
            assert!(
                result.message.contains("No new keyword ideas found")
                    || result.message.contains("Failed to fetch keyword ideas")
                    || result.message.contains("CAPSOLVER"),
                "unexpected smoke-flow failure: {}",
                result.message
            );
            return;
        }

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap_or("{}")).unwrap_or_default();
        assert!(output.is_object(), "expected JSON output object when successful");
    }
}

#[cfg(test)]
mod volume_tests {
    use super::estimate_volume;

    #[test]
    fn estimate_volume_maps_ahrefs_labels() {
        assert_eq!(estimate_volume("MoreThanOneHundred"), Some(100));
        assert_eq!(estimate_volume("MoreThanOneThousand"), Some(1000));
        assert_eq!(estimate_volume("LessThanOneHundred"), Some(50));
    }

    #[test]
    fn estimate_volume_parses_ranges_and_numbers() {
        assert_eq!(estimate_volume("100-1,000"), Some(550));
        assert_eq!(estimate_volume("2,400"), Some(2400));
    }
}

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
    use crate::config::{default_execution_mode, default_phase};
    use crate::engine::task_store;
    use crate::models::task::{AgentPolicy, Priority, Task, TaskArtifact, TaskRun, TaskStatus};
    use std::collections::HashSet;

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
        score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Take top N keywords
    let selected = keyword_data.into_iter().take(max_tasks).collect::<Vec<_>>();

    let mut created_count = 0usize;
    for (idx, kw_data) in selected.iter().enumerate() {
        let now = chrono::Utc::now().to_rfc3339();
        let id = format!("task-{}-{}", chrono::Utc::now().timestamp_millis(), idx);
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

        let task = Task {
            id,
            phase: default_phase("write_article").to_string(),
            execution_mode: default_execution_mode("write_article"),
            task_type: "write_article".to_string(),
            status: TaskStatus::Todo,
            priority: priority_enum,
            agent_policy: AgentPolicy::None,
            title: Some(title),
            description: Some(description),
            project_id: research_task.project_id.clone(),
            depends_on: vec![research_task.id.clone()],
            artifacts: vec![provenance],
            run: TaskRun::default(),
            created_at: now.clone(),
            updated_at: now,
        };

        match task_store::create_task(conn, &task) {
            Ok(_) => {
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
        .or_else(|| task.artifacts.iter().find(|a| a.key == "research_keywords_cli"))
        .or_else(|| task.artifacts.iter().find(|a| a.key == "research_agent_stage"));

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
                        has_data: item.get("has_data").and_then(|x| x.as_bool()).unwrap_or(true),
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
                        has_data: item.get("has_data").and_then(|x| x.as_bool()).unwrap_or(true),
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

// Integration tests for keyword research workflow
#[cfg(test)]
mod keyword_workflow_tests {
    use super::*;
    use crate::engine::workflows::handlers::default_handlers;
    use crate::models::task::{Task, TaskRun, TaskStatus, Priority, ExecutionMode, AgentPolicy};
    use chrono::Utc;

    fn in_memory_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS projects (
                id TEXT PRIMARY KEY, name TEXT NOT NULL,
                path TEXT NOT NULL,
                content_dir TEXT,
                site_url TEXT,
                site_id TEXT,
                active INTEGER NOT NULL DEFAULT 1,
                agent_provider TEXT
             );
             CREATE TABLE IF NOT EXISTS tasks (
                id TEXT PRIMARY KEY, type TEXT NOT NULL, phase TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'todo',
                priority TEXT NOT NULL DEFAULT 'medium',
                execution_mode TEXT NOT NULL DEFAULT 'manual',
                agent_policy TEXT NOT NULL DEFAULT 'none',
                title TEXT, description TEXT,
                project_id TEXT NOT NULL,
                depends_on TEXT NOT NULL DEFAULT '[]',
                artifacts TEXT NOT NULL DEFAULT '[]',
                run_attempts INTEGER NOT NULL DEFAULT 0,
                run_last_error TEXT, run_provider TEXT,
                created_at TEXT NOT NULL, updated_at TEXT NOT NULL
             );",
        ).unwrap();
        conn
    }

    fn create_test_project(conn: &rusqlite::Connection, path: &str) -> String {
        let id = format!("proj-{}", Utc::now().timestamp_millis());
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES (?1, 'Test', ?2, 1)",
            [&id, path],
        ).unwrap();
        id
    }

    fn create_keyword_research_task(project_id: &str, themes: &[&str]) -> Task {
        Task {
            id: format!("task-{}", Utc::now().timestamp_millis()),
            project_id: project_id.to_string(),
            task_type: "research_keywords".to_string(),
            phase: "research".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            execution_mode: ExecutionMode::Automatic,
            agent_policy: AgentPolicy::Optional,
            title: Some("Keyword Research".to_string()),
            description: if themes.is_empty() {
                None // No themes provided - should trigger agentic mode
            } else {
                Some(format!("Themes: {}", themes.join(", ")))
            },
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun { attempts: 0, last_error: None, provider: None },
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        }
    }

    /// Test workflow planning - now uses 3-step agentic workflow for all research tasks.
    #[test]
    fn workflow_uses_four_step_hybrid_workflow() {
        let conn = in_memory_db();
        let temp_dir = std::env::temp_dir().join(format!("ps_kw_test_{}", Utc::now().timestamp_millis()));
        std::fs::create_dir_all(&temp_dir.join(".github").join("automation")).unwrap();
        
        std::fs::write(
            temp_dir.join(".github").join("automation").join("articles.json"),
            r#"{"nextArticleId":1,"articles":[]}"#
        ).unwrap();

        let project_id = create_test_project(&conn, &temp_dir.to_string_lossy());
        let task = create_keyword_research_task(&project_id, &["personal finance", "budgeting"]);

        let handlers = default_handlers();
        let handler = handlers.iter().find(|h| h.supports(&task)).expect("Should find handler");
        let steps = handler.plan(&task);
        
        // New 4-step hybrid workflow: 
        //   1. seed extraction (agentic)
        //   2. ahrefs pipeline (deterministic) 
        //   3. final selection (agentic)
        //   4. normalizer (normalizer)
        assert_eq!(steps.len(), 4, "Should have 4 steps: agentic → deterministic → agentic → normalizer");
        assert_eq!(steps[0].name, "research_seed_extraction");
        assert_eq!(steps[0].kind, "agentic");
        assert_eq!(steps[1].name, "research_ahrefs_pipeline");
        assert_eq!(steps[1].kind, "keyword_research_native");
        assert_eq!(steps[2].name, "research_final_selection");
        assert_eq!(steps[2].kind, "research_final_selection");
        assert_eq!(steps[3].name, "research_normalize");
        assert_eq!(steps[3].kind, "normalizer");
        
        std::fs::remove_dir_all(&temp_dir).ok();
    }
}

#[cfg(test)]
mod sampling_tests {
    use super::{Candidate, smart_sample_candidates};

    fn make(kw: &str, theme: &str, is_question: bool, volume: Option<i64>) -> Candidate {
        Candidate {
            keyword: kw.to_string(),
            source_theme: theme.to_string(),
            is_question,
            volume,
        }
    }

    #[test]
    fn sampling_returns_all_when_below_budget() {
        let candidates = vec![
            make("a", "t1", false, Some(100)),
            make("b", "t1", false, Some(200)),
        ];
        let result = smart_sample_candidates(candidates.clone(), 10);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn sampling_stratifies_across_themes() {
        let candidates = vec![
            make("a1", "t1", false, Some(1000)),
            make("a2", "t1", false, Some(900)),
            make("a3", "t1", false, Some(800)),
            make("b1", "t2", false, Some(700)),
            make("b2", "t2", false, Some(600)),
            make("b3", "t2", false, Some(500)),
        ];
        let result = smart_sample_candidates(candidates, 4);
        let t1_count = result.iter().filter(|c| c.source_theme == "t1").count();
        let t2_count = result.iter().filter(|c| c.source_theme == "t2").count();
        assert!(t1_count >= 1, "t1 should have at least 1 sample");
        assert!(t2_count >= 1, "t2 should have at least 1 sample");
        assert_eq!(result.len(), 4);
    }

    #[test]
    fn sampling_prioritizes_question_keywords() {
        let candidates = vec![
            make("a1", "t1", false, Some(1000)),
            make("a2", "t1", true, Some(100)), // question
            make("a3", "t1", false, Some(800)),
        ];
        let result = smart_sample_candidates(candidates, 2);
        assert!(result.iter().any(|c| c.keyword == "a2" && c.is_question), "question keyword should be sampled");
    }

    #[test]
    fn sampling_fills_remaining_with_highest_volume() {
        let candidates = vec![
            make("a1", "t1", false, Some(100)),
            make("b1", "t2", false, Some(1000)),
            make("b2", "t2", false, Some(900)),
        ];
        let result = smart_sample_candidates(candidates, 3);
        assert_eq!(result.len(), 3);
        // The highest-volume keyword should definitely be included.
        assert!(result.iter().any(|c| c.keyword == "b1" && c.volume == Some(1000)));
    }
}
