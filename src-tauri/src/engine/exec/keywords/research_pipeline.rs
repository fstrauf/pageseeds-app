use crate::engine::project_paths::ProjectPaths;
use crate::models::project::ProjectMode;
use crate::models::task::Task;
use std::collections::{HashMap, HashSet};

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
pub(crate) struct SeedArtifact {
    pub(crate) themes: Vec<String>,
    pub(crate) competitors: Vec<String>,
}

/// Parse the output of Step 1 in the hybrid research workflow.
/// Expects JSON with {"themes": [...], "competitors": [...]}.
pub(crate) fn parse_seed_extraction_artifact(task: &Task) -> SeedArtifact {
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
            return SeedArtifact {
                themes,
                competitors,
            };
        }
    }

    // Fallback: extract JSON from fenced blocks or bare JSON
    if let Some(json) = crate::engine::text::extract_json(raw) {
        let themes = themes_from_json(&json);
        let competitors = competitors_from_json(&json);
        if !themes.is_empty() || !competitors.is_empty() {
            return SeedArtifact {
                themes,
                competitors,
            };
        }
    }

    SeedArtifact::default()
}

fn themes_from_json(v: &serde_json::Value) -> Vec<String> {
    let from_array = |arr: &[serde_json::Value]| {
        arr.iter()
            .filter_map(|x| x.as_str())
            .filter_map(super::clean_theme_str)
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
            .map(|s| {
                s.trim()
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .split('/')
                    .next()
                    .unwrap_or(s)
                    .to_string()
            })
            .filter(|s| !s.is_empty() && s.contains('.'))
            .collect::<Vec<String>>()
    };

    if let Some(arr) = v.get("competitors").and_then(|x| x.as_array()) {
        return extract(arr);
    }

    vec![]
}

/// Parse the `research_seed_validation` artifact.
///
/// Returns a flat list of `(theme, seed)` pairs ready for DataForSEO calls.
/// Expected artifact format:
/// `{validated_seeds: [{theme: string, seeds: [string]}]}`
fn parse_validated_seeds_artifact(task: &Task) -> Vec<(String, String)> {
    let content = task
        .artifacts
        .iter()
        .rev()
        .find(|a| a.key == "research_seed_validation")
        .and_then(|a| a.content.as_deref());

    let Some(raw) = content else {
        return vec![];
    };

    // Try direct JSON parse first, then extract_json helper
    let json = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .or_else(|| crate::engine::text::extract_json(raw));

    let Some(json) = json else {
        return vec![];
    };

    let validated = json.get("validated_seeds").and_then(|v| v.as_array());

    let Some(validated) = validated else {
        return vec![];
    };

    let mut pairs: Vec<(String, String)> = vec![];
    for entry in validated {
        let theme = entry
            .get("theme")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        if theme.is_empty() {
            continue;
        }
        let seeds = entry.get("seeds").and_then(|s| s.as_array());
        if let Some(seeds) = seeds {
            for seed in seeds {
                if let Some(s) = seed.as_str() {
                    let s = s.trim();
                    if !s.is_empty() {
                        pairs.push((theme.clone(), s.to_string()));
                    }
                }
            }
        }
    }
    pairs
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
pub(crate) struct Candidate {
    pub(crate) keyword: String,
    pub(crate) source_theme: String,
    pub(crate) is_question: bool,
    pub(crate) volume: Option<i64>,
    pub(crate) kd: Option<f64>,
    pub(crate) intent: Option<String>,
}

/// Smart sampling: select a diverse subset of candidates for KD checking.
/// Ensures coverage across themes and reserves slots for question keywords.
pub(crate) fn smart_sample_candidates(candidates: Vec<Candidate>, budget: usize) -> Vec<Candidate> {
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

        let quota = base_per_theme
            + if extra > 0 {
                extra -= 1;
                1
            } else {
                0
            };

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
    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(conn) => conn,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to open app database: {}", e),
                output: None,
            }
        }
    };
    let project = match crate::engine::task_store::get_project(&db, &task.project_id) {
        Ok(project) => project,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to load project '{}': {}", task.project_id, e),
                output: None,
            }
        }
    };
    let is_live_site_project = project.project_mode == ProjectMode::LiveSite;

    // ── Re-use cached results if this step already ran ────────────────────────
    // Prevents burning paid API credits on accidental re-runs.
    if let Some(existing) = task
        .artifacts
        .iter()
        .find(|a| a.key == "research_ahrefs_pipeline")
    {
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

    // ── Extract themes from Step 1 (agentic seed extraction) ────────────────
    // Theme extraction is always agentic — deterministic parsing of free-form
    // descriptions produces garbage (sentence fragments → bad API queries).
    // Step 1 (research_seed_extraction) must run first and produce themes.
    let SeedArtifact {
        mut themes,
        competitors: agent_competitors,
    } = parse_seed_extraction_artifact(task);

    // Fallback for custom_keyword_research: read themes from task description
    // when seed extraction artifact is missing. This lets users run streamlined
    // research on a small set of known keywords without the full agentic pipeline.
    if themes.is_empty() && task.task_type == "custom_keyword_research" {
        if let Some(desc) = &task.description {
            let desc_themes: Vec<String> = desc
                .lines()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if !desc_themes.is_empty() {
                log::info!(
                    "[keyword_research_native] custom_keyword_research: using {} themes from description",
                    desc_themes.len()
                );
                themes = desc_themes;
            }
        }
    }

    if themes.is_empty() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "No keyword themes found in seed extraction artifact. \
                 Step 1 (research_seed_extraction) must run first. \
                 Expected artifact key: research_seed_extraction. Workspace: {}.",
                paths.automation_dir.display()
            ),
            output: None,
        };
    }

    log::info!(
        "[keyword_research_native] {} themes: {:?}",
        themes.len(),
        themes
    );
    log::info!(
        "[keyword_research_native] {} competitors: {:?}",
        agent_competitors.len(),
        agent_competitors
    );

    // ── Cost estimate (DataForSEO) ────────────────────────────────────────────
    // Two-phase: Phase 1 (Google Autocomplete) is free.
    // Phase 2 tries seeds in order, stopping at first hit (~1-2 calls per theme on average).
    if is_dataforseo {
        let est_cost = themes.len() as f64 * 0.012; // ~$0.01/task + $0.0001 × ~20 keywords
        log::info!(
            "[keyword_research_native] DataForSEO estimated cost: ~${:.3} ({} themes × ~$0.012/theme, stops at first hit)",
            est_cost, themes.len()
        );
    }

    // ── Pre-flight: articles must exist in SQLite ────────────────────────────
    let has_articles = if is_live_site_project {
        match crate::live_site::list_live_site_pages(&db, &task.project_id) {
            Ok(pages) => !pages.is_empty(),
            Err(_) => false,
        }
    } else {
        match crate::content::article_index::list_articles(&db, &task.project_id) {
            Ok(a) => !a.is_empty(),
            Err(_) => false,
        }
    };
    if !has_articles {
        return crate::engine::workflows::StepResult {
            success: false,
            message: "Workspace not initialised: no articles found in the app index. \
                 Run 'Init Workspace' from Project Settings first."
                .into(),
            output: None,
        };
    }

    // Load existing keywords from SQLite so we can skip already-covered ones.
    let existing_keywords: HashSet<String> = if is_live_site_project {
        match crate::live_site::list_live_site_pages(&db, &task.project_id) {
            Ok(pages) => super::collect_existing_keywords_from_live_site(&pages),
            Err(e) => {
                return crate::engine::workflows::StepResult {
                    success: false,
                    message: format!(
                        "Failed to load live-site pages for keyword filtering: {}",
                        e
                    ),
                    output: None,
                }
            }
        }
    } else {
        crate::content::article_index::existing_keywords(&db, &task.project_id).unwrap_or_default()
    };

    log::info!(
        "[keyword_research_native] {} existing keywords to filter against",
        existing_keywords.len()
    );

    // ── Load coverage analysis for gap filtering ──────────────────────────────
    let coverage_clusters = super::load_coverage_clusters(project_path);
    let has_coverage = !coverage_clusters.is_empty();
    if has_coverage {
        log::info!(
            "[keyword_research_native] loaded {} coverage clusters for gap analysis",
            coverage_clusters.len()
        );
    } else {
        log::info!("[keyword_research_native] no coverage analysis found, skipping gap filtering");
    }

    // ── Pre-parse validated seeds (needs task borrow, must happen before thread spawn) ──
    let validated_seeds = parse_validated_seeds_artifact(task);

    // ── Read pending territory shortlist entries ──────────────────────────────
    let pending_shortlist = read_pending_shortlist(task);
    let pending_shortlist_ids: Vec<i64> = pending_shortlist.iter().filter_map(|e| e.id).collect();
    let shortlist_seeds: Vec<(String, String)> = pending_shortlist
        .into_iter()
        .flat_map(|entry| {
            let theme = entry.theme.clone();
            entry.seeds.into_iter().map(move |seed| (theme.clone(), seed))
        })
        .collect();
    if !shortlist_seeds.is_empty() {
        log::info!(
            "[keyword_research_native] {} pending shortlist entries ({} seeds) to research",
            pending_shortlist_ids.len(),
            shortlist_seeds.len()
        );
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
    let validated_seeds_thread = validated_seeds;
    let shortlist_seeds_thread = shortlist_seeds;

    let thread_result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async move {
            let env = crate::config::env_resolver::EnvResolver::new(&project_path_thread);
            let provider = crate::seo::resolve_provider(&seo_provider_thread, &env)?;
            let is_dataforseo = provider.name() == "dataforseo";

            let mut candidates: Vec<Candidate> = vec![];
            let mut seen: HashSet<String> = HashSet::new();

            if is_dataforseo {
                // ── DataForSEO path: read validated seeds from Step 3 artifact ────────────
                // Seeds were pre-filtered for domain relevance by the agentic
                // research_seed_validation step. One DataForSEO call per seed.
                if validated_seeds_thread.is_empty() {
                    log::warn!(
                        "[keyword_research_native] research_seed_validation artifact missing or empty — \
                         falling back to raw themes as seeds"
                    );
                }

                // Use validated seeds if available, otherwise fall back to raw themes.
                // Also inject pending territory shortlist seeds so they get validated.
                let mut seeds_to_use: Vec<(String, String)> = if !validated_seeds_thread.is_empty() {
                    validated_seeds_thread
                } else {
                    themes_thread.iter().map(|t| (t.clone(), t.clone())).collect()
                };
                seeds_to_use.extend(shortlist_seeds_thread);
                // Deduplicate by seed string
                {
                    let mut seen_seeds = HashSet::new();
                    seeds_to_use.retain(|(_, seed)| seen_seeds.insert(seed.clone()));
                }

                log::info!(
                    "[keyword_research_native] {} (theme, seed) pairs to query",
                    seeds_to_use.len()
                );

                let mut dataforseo_successes = 0usize;
                let mut dataforseo_failures = 0usize;

                for (theme, seed) in &seeds_to_use {
                    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;
                    match provider.keyword_ideas(seed, "us", "google").await {
                        Ok(result) => {
                            let count = result.ideas.len() + result.question_ideas.len();
                            log::info!(
                                "[keyword_research_native] theme '{}' seed '{}' → {} ideas",
                                theme, seed, count
                            );
                            dataforseo_successes += 1;
                            for idea in result.ideas.iter().chain(result.question_ideas.iter()) {
                                let kw_lower = idea.keyword.to_lowercase();
                                if existing_keywords_thread.contains(&kw_lower) || seen.contains(&kw_lower) {
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
                        }
                        Err(e) => {
                            let err_str = e.to_string();
                            log::warn!(
                                "[keyword_research_native] DataForSEO failed for seed '{}': {}",
                                seed, err_str
                            );
                            dataforseo_failures += 1;
                            // Credit exhaustion is a hard stop — don't waste calls or mislead the user.
                            if err_str.contains("402") || err_str.contains("Payment Required") {
                                return Err(crate::error::Error::Other(
                                    "DataForSEO credits exhausted (402 Payment Required). \
                                     Please top up your DataForSEO account and retry.".to_string()
                                ));
                            }
                        }
                    }
                }

                // If every single DataForSEO call failed, the result set is unreliable.
                // Fail the step so the user knows something is wrong instead of getting empty/partial data.
                if dataforseo_successes == 0 && dataforseo_failures > 0 {
                    return Err(crate::error::Error::Other(
                        format!(
                            "DataForSEO failed for all {} seeds. No keyword data could be retrieved. \
                             Check your API credentials and account status, then retry.",
                            dataforseo_failures
                        )
                    ));
                }

                log::info!(
                    "[keyword_research_native] DataForSEO phase complete → {} total candidates ({} seeds ok, {} failed)",
                    candidates.len(),
                    dataforseo_successes,
                    dataforseo_failures
                );
            } else {
                // ── Ahrefs/Google Autocomplete path (legacy) ──────────────────────
                // Build theme list from normal themes + shortlist themes
                let mut all_themes = themes_thread.clone();
                for (theme, _) in &shortlist_seeds_thread {
                    if !all_themes.contains(theme) {
                        all_themes.push(theme.clone());
                    }
                }
                for theme in &all_themes {
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
                candidates = super::filter_by_coverage_gap(
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
                    candidates = super::filter_by_coverage_gap(candidates, &coverage_clusters_thread, &existing_keywords_thread);
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

    let (with_data_results, no_data_results, analyzed_count, total_candidates, competitor_insights) =
        match thread_result {
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

    // Mark shortlist entries as researched
    if !pending_shortlist_ids.is_empty() {
        if let Ok(conn) = rusqlite::Connection::open(crate::db::default_db_path()) {
            match crate::db::research_shortlist::mark_researched(&conn, &pending_shortlist_ids) {
                Ok(n) => log::info!("[keyword_research_native] Marked {} shortlist entries as researched", n),
                Err(e) => log::warn!("[keyword_research_native] Failed to mark shortlist entries as researched: {}", e),
            }
        }
    }

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
        difficulty_results
            .iter()
            .filter(|r| r["has_data"] == true)
            .count(),
        difficulty_results.len(),
        analyzed_count,
    );

    // total_candidates already captured from pre_filter_count
    let with_data_count = difficulty_results
        .iter()
        .filter(|r| r["has_data"] == true)
        .count();

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
            themes.len(),
            total_candidates,
            analyzed_count
        ),
        output: Some(serde_json::to_string_pretty(&output).unwrap_or_default()),
    }
}

/// Read pending shortlist entries from SQLite for this task's project.
fn read_pending_shortlist(task: &Task) -> Vec<crate::db::research_shortlist::ResearchShortlistEntry> {
    let db_path = crate::db::default_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[keyword_research_native] Failed to open DB for shortlist: {}", e);
            return Vec::new();
        }
    };
    match crate::db::research_shortlist::list_entries(&conn, &task.project_id, Some("pending")) {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!("[keyword_research_native] Failed to read shortlist: {}", e);
            Vec::new()
        }
    }
}
