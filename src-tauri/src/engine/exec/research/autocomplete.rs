use crate::engine::workflows::StepResult;
use crate::models::research::{KeywordPipelineOutput, LandingPageCandidate, SelectedKeyword};
use crate::models::task::Task;

/// Deterministic Step 2: fetch Google Autocomplete suggestions for all themes.
///
/// Reads the `research_seed_extraction` artifact from the task, calls Google
/// Autocomplete (free, no auth) for each theme, and outputs structured JSON:
/// `[{theme: string, suggestions: [string]}]`
///
/// This output is consumed by the agentic Step 3 (research_seed_validation).
pub fn exec_research_autocomplete(task: &Task, project_path: &str) -> StepResult {
    use crate::engine::exec::keywords::parse_seed_extraction_artifact;

    let seed_artifact = parse_seed_extraction_artifact(task);

    if seed_artifact.themes.is_empty() {
        return StepResult {
            success: false,
            message: "No themes found in research_seed_extraction artifact. Step 1 must run first."
                .to_string(),
            output: None,
        };
    }

    log::info!(
        "[research_autocomplete] Fetching Google Autocomplete for {} themes",
        seed_artifact.themes.len()
    );

    let themes = seed_artifact.themes.clone();
    let _project_path = project_path.to_string();

    let thread_result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new()
            .map_err(|e| format!("Failed to create runtime: {}", e))?;

        rt.block_on(async move {
            let mut results: Vec<serde_json::Value> = Vec::new();

            for theme in &themes {
                tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

                let suggestions =
                    match crate::seo::google_autocomplete::fetch_suggestions(theme, "us", "en")
                        .await
                    {
                        Ok(s) => s,
                        Err(e) => {
                            log::warn!(
                                "[research_autocomplete] Autocomplete failed for '{}': {}",
                                theme,
                                e
                            );
                            vec![]
                        }
                    };

                let suggestion_list: Vec<String> = suggestions
                    .iter()
                    .take(4)
                    .map(|s| s.keyword.clone())
                    .collect();

                log::info!(
                    "[research_autocomplete] theme '{}' → {} suggestions: {:?}",
                    theme,
                    suggestion_list.len(),
                    suggestion_list
                );

                results.push(serde_json::json!({
                    "theme": theme,
                    "suggestions": suggestion_list,
                }));
            }

            Ok::<Vec<serde_json::Value>, String>(results)
        })
    })
    .join()
    .map_err(|_| "Autocomplete thread panicked".to_string());

    match thread_result {
        Ok(Ok(results)) => {
            let json = serde_json::to_string_pretty(&results).unwrap_or_else(|_| "[]".to_string());
            log::info!(
                "[research_autocomplete] Complete — {} theme entries ({} chars)",
                results.len(),
                json.len()
            );
            StepResult {
                success: true,
                message: format!("Google Autocomplete fetched for {} themes", results.len()),
                output: Some(json),
            }
        }
        Ok(Err(e)) => StepResult {
            success: false,
            message: format!("Autocomplete failed: {}", e),
            output: None,
        },
        Err(e) => StepResult {
            success: false,
            message: format!("Autocomplete thread error: {}", e),
            output: None,
        },
    }
}

/// Output format matching what the frontend KeywordPicker expects.
///
/// The frontend expects either:
/// - `landing_page_candidates` for landing page research
/// - `difficulty.results` for keyword research (wrapped in difficulty object)
#[derive(Debug, Clone, serde::Serialize)]
pub struct KeywordPickerOutput {
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub landing_page_candidates: Vec<LandingPageCandidate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<DifficultyWrapper>,
    pub total_candidates: usize,
    pub filtered_out: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DifficultyWrapper {
    pub total: usize,
    pub successful: usize,
    pub results: Vec<SelectedKeyword>,
}

/// Deterministic final selection of keywords from pipeline output.
///
/// This replaces the agentic step with pure Rust logic, but remains
/// workflow-aware: `research_keywords` surfaces informational content ideas,
/// while `research_landing_pages` surfaces commercial/transactional ones.
///
/// Selection logic:
/// - Filter to keywords with data, acceptable KD, non-navigational intent, and
///   intent aligned with the workflow (informational for blog, commercial for
///   landing pages).
/// - Sort by volume (desc), then difficulty (asc).
/// - Take top N (default 10).
/// - Generate recommended titles based on keyword type.
pub fn select_keywords_deterministic(
    pipeline_json: &str,
    is_landing_page: bool,
) -> Result<(KeywordPickerOutput, bool), String> {
    // Parse pipeline output
    let pipeline: KeywordPipelineOutput = serde_json::from_str(pipeline_json)
        .map_err(|e| format!("Failed to parse pipeline output: {}", e))?;

    let target_kd = 30i64; // 0-100 scale (DataForSEO/Ahrefs unified)
    let max_results = 10usize;
    let total_candidates = pipeline.keywords.len();

    // Primary filter: data + KD + non-navigational + workflow-aligned intent.
    let mut candidates: Vec<_> = pipeline
        .keywords
        .clone()
        .into_iter()
        .filter(|k| matches_workflow_intent(k, is_landing_page))
        .filter(|k| {
            let has_data = k.has_data.unwrap_or(false);
            let kd_ok = k.kd.map(|d| d as i64 <= target_kd).unwrap_or(false);
            let intent_ok = k
                .intent
                .as_deref()
                .map(|i| !i.eq_ignore_ascii_case("navigational"))
                .unwrap_or(true);
            has_data && kd_ok && intent_ok
        })
        .collect();

    // No fallback. If strict filtering yields nothing, the task fails with an
    // actionable message rather than silently relaxing the quality bar. The
    // user iterates on seed keywords rather than accepting low-quality
    // candidates that would become dead-weight articles.
    if candidates.is_empty() {
        return Err(format!(
            "No keywords met the quality bar after filtering {} candidates. \
             Criteria: KD ≤ {}, non-navigational intent, with verified search data. \
             Try different seed keywords, broaden the territory, or lower the \
             difficulty expectation for this workflow.",
            total_candidates, target_kd
        ));
    }

    let used_fallback = false;

    // Sort by volume desc, then KD asc
    candidates.sort_by(|a, b| {
        let vol_cmp = b.volume.unwrap_or(0).cmp(&a.volume.unwrap_or(0));
        if vol_cmp != std::cmp::Ordering::Equal {
            return vol_cmp;
        }
        let kd_a = a.kd.unwrap_or(100.0) as i64;
        let kd_b = b.kd.unwrap_or(100.0) as i64;
        kd_a.cmp(&kd_b)
    });

    // Take top N
    let selected: Vec<_> = candidates.into_iter().take(max_results).collect();
    let filtered_out = total_candidates.saturating_sub(selected.len());

    if is_landing_page {
        Ok((KeywordPickerOutput {
            landing_page_candidates: selected
                .into_iter()
                .map(|k| LandingPageCandidate {
                    keyword: k.keyword.clone(),
                    estimated_volume: k.volume.unwrap_or(0),
                    estimated_kd: k.kd.unwrap_or(0.0) as i64,
                    intent: k
                        .intent
                        .clone()
                        .unwrap_or_else(|| "informational".to_string()),
                    landing_page_type: infer_landing_page_type(&k.keyword),
                    opportunity_score: "high".to_string(),
                    opportunity_reason: format!(
                        "KD {} with {} monthly searches",
                        k.kd.map(|d| d as i64).unwrap_or(0),
                        k.volume.unwrap_or(0)
                    ),
                    proposed_title: generate_title(&k.keyword, true),
                })
                .collect(),
            difficulty: None,
            total_candidates,
            filtered_out,
        }, used_fallback))
    } else {
        let results: Vec<_> = selected
            .into_iter()
            .map(|k| SelectedKeyword {
                keyword: k.keyword.clone(),
                volume: k.volume.unwrap_or(0),
                difficulty: k.kd.unwrap_or(0.0) as i64,
                traffic: k.traffic.map(|t| t as i64),
                selection_reason: format!(
                    "KD {} with {} monthly searches",
                    k.kd.map(|d| d as i64).unwrap_or(0),
                    k.volume.unwrap_or(0)
                ),
                recommended_title: generate_title(&k.keyword, false),
                // Populated by enrich_with_winnability() after selection.
                winnability: None,
                winnability_reason: None,
            })
            .collect();

        let successful = results.len();
        Ok((KeywordPickerOutput {
            landing_page_candidates: Vec::new(),
            difficulty: Some(DifficultyWrapper {
                total: successful,
                successful,
                results,
            }),
            total_candidates,
            filtered_out,
        }, used_fallback))
    }
}

/// Returns true when a keyword's intent matches the workflow goal.
///
/// Blog research wants informational/educational keywords. Landing page
/// research wants commercial/transactional keywords. Unknown intent is allowed
/// because pattern matching is conservative (especially for SaaS keywords that
/// default to informational despite being commercial).
fn matches_workflow_intent(k: &crate::models::research::ScoredKeyword, is_landing_page: bool) -> bool {
    let intent = k.intent.as_deref().map(|i| i.to_lowercase());
    match intent.as_deref() {
        None | Some("unknown") => true,
        Some("navigational") => false,
        Some(i) if is_landing_page => {
            matches!(i, "commercial" | "transactional")
        }
        Some(i) => {
            matches!(i, "informational")
        }
    }
}

/// Infer landing page type from keyword patterns
fn infer_landing_page_type(keyword: &str) -> String {
    let lower = keyword.to_lowercase();
    if lower.contains("vs") || lower.contains("compare") || lower.contains("alternative") {
        "comparison".to_string()
    } else if lower.contains("best") || lower.contains("top") || lower.contains("review") {
        "category".to_string()
    } else if lower.contains("how to") || lower.contains("guide") || lower.contains("tutorial") {
        "use_case".to_string()
    } else if lower.contains("software")
        || lower.contains("tool")
        || lower.contains("app")
        || lower.contains("tracker")
        || lower.contains("screener")
        || lower.contains("calculator")
        || lower.contains("dashboard")
        || lower.contains("scanner")
        || lower.contains("platform")
    {
        "feature".to_string()
    } else {
        "category".to_string()
    }
}

/// Generate a readable title from a keyword.
///
/// Landing page titles are conversion-focused; blog titles are guide-focused.
fn generate_title(keyword: &str, is_landing_page: bool) -> String {
    // Capitalize first letter of each word
    let words: Vec<String> = keyword
        .split_whitespace()
        .enumerate()
        .map(|(i, word)| {
            if i == 0 || !is_stop_word(word) {
                capitalize_first(word)
            } else {
                word.to_lowercase()
            }
        })
        .collect();

    let title = words.join(" ");
    let lower = keyword.to_lowercase();

    if is_landing_page {
        if lower.contains("vs") {
            format!("{}: Which is Right for You?", title)
        } else if lower.contains("best") || lower.contains("top") {
            format!("{} for 2025", title)
        } else if lower.contains("alternative") || lower.contains("alternatives") {
            format!("The Best {} Alternative", title)
        } else if lower.contains("software")
            || lower.contains("tool")
            || lower.contains("app")
            || lower.contains("tracker")
            || lower.contains("screener")
            || lower.contains("calculator")
            || lower.contains("dashboard")
            || lower.contains("platform")
        {
            format!("{} for Options Traders", title)
        } else {
            format!("{} — DaysToExpiry", title)
        }
    } else {
        if lower.contains("how to") {
            format!("{}: A Step-by-Step Guide", title)
        } else if lower.contains("what is") || lower.contains("what are") {
            format!("{} Explained", title)
        } else if lower.contains("best") || lower.contains("top") {
            format!("{} for 2025", title)
        } else if lower.contains("vs") {
            format!("{}: Which is Right for You?", title)
        } else if lower.contains("tips") {
            format!("{} That Actually Work", title)
        } else {
            format!("{}: Complete Guide", title)
        }
    }
}

fn is_stop_word(word: &str) -> bool {
    matches!(
        word.to_lowercase().as_str(),
        "a" | "an"
            | "the"
            | "and"
            | "or"
            | "but"
            | "in"
            | "on"
            | "at"
            | "to"
            | "for"
            | "of"
            | "with"
            | "vs"
            | "versus"
    )
}

fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

/// Execute the deterministic final selection step.
///
/// This is called by the executor when it encounters a step with kind "research_final_selection".
/// It reads the previous step's output (keyword pipeline results) and applies deterministic
/// filtering/sorting to select the best candidates.
pub fn exec_research_final_selection(
    task: &Task,
    _project_path: &str,
    previous_output: Option<&str>,
) -> StepResult {
    let pipeline_json = match previous_output {
        Some(out) => out,
        None => {
            return StepResult {
                success: false,
                message: "No previous step output found — expected keyword pipeline results"
                    .to_string(),
                output: None,
            };
        }
    };

    let is_landing_page = task.task_type == "research_landing_pages";

    log::info!(
        "[research_final_selection] Running deterministic selection for {} (landing_page={})",
        task.task_type,
        is_landing_page
    );

    match select_keywords_deterministic(pipeline_json, is_landing_page) {
        Ok((mut output, used_fallback)) => {
            // Enrich the selected keywords with winnability scores (AIO risk,
            // competitor authority, KD) before writing the artifact.
            enrich_with_winnability(&mut output, &task.project_id);

            let json = match serde_json::to_string_pretty(&output) {
                Ok(j) => j,
                Err(e) => {
                    return StepResult {
                        success: false,
                        message: format!("Failed to serialize output: {}", e),
                        output: None,
                    };
                }
            };

            let count = if is_landing_page {
                output.landing_page_candidates.len()
            } else {
                output
                    .difficulty
                    .as_ref()
                    .map(|d| d.results.len())
                    .unwrap_or(0)
            };

            let msg = if used_fallback {
                format!(
                    "Selected {} keywords (API data unavailable; showing best candidates without KD/volume filters)",
                    count
                )
            } else {
                format!(
                    "Selected {} keywords deterministically (KD <= 30, sorted by volume)",
                    count
                )
            };

            StepResult {
                success: true,
                message: msg,
                output: Some(json),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Keyword selection failed: {}", e),
            output: None,
        },
    }
}

/// Enrich selected keywords with winnability scores using SERP feature data.
///
/// Calls the DataForSEO SERP API for each keyword and scores it using the
/// winnability classifier (Target / Differentiate / Avoid). Non-fatal: if the
/// provider is unavailable or a SERP lookup fails, the keyword keeps its
/// existing fields without a winnability score.
fn enrich_with_winnability(output: &mut KeywordPickerOutput, project_id: &str) {
    let keywords = match &mut output.difficulty {
        Some(d) => &mut d.results,
        None => return,
    };
    if keywords.is_empty() {
        return;
    }

    // SERP feature enrichment requires an async runtime (HTTP calls to
    // DataForSEO). Run it in a dedicated runtime like the cannibalization
    // batch step does.
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[winnability] Failed to create runtime: {}", e);
            return;
        }
    };

    rt.block_on(async {
        let conn = match rusqlite::Connection::open(crate::db::default_db_path()) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("[winnability] DB error: {}", e);
                return;
            }
        };
        let project = match crate::engine::task_store::get_project(&conn, project_id) {
            Ok(p) => p,
            Err(e) => {
                log::warn!("[winnability] Project error: {}", e);
                return;
            }
        };
        let provider_name = project.seo_provider.as_deref().unwrap_or("dataforseo");
        let env = crate::config::env_resolver::EnvResolver::new(&project.path);
        let provider = match crate::seo::resolve_provider(provider_name, &env) {
            Ok(p) => p,
            Err(e) => {
                log::warn!(
                    "[winnability] Could not resolve SEO provider '{}': {}. \
                     Keywords will lack winnability scores.",
                    provider_name,
                    e
                );
                return;
            }
        };

        log::info!(
            "[winnability] Enriching {} keywords with SERP features via {}",
            keywords.len(),
            provider_name
        );

        for kw in keywords.iter_mut() {
            match provider.serp_features(&kw.keyword, "us").await {
                Ok(serp) => {
                    let assessment = crate::seo::winnability::assess(
                        &kw.keyword,
                        &serp,
                        Some(kw.difficulty as f64),
                        Some("informational"),
                    );
                    log::info!(
                        "[winnability] '{}' → {} (risk={})",
                        kw.keyword,
                        assessment.bucket,
                        assessment.risk_score
                    );
                    kw.winnability = Some(assessment.bucket.as_str().to_string());
                    kw.winnability_reason = Some(assessment.reason);
                }
                Err(e) => {
                    log::warn!(
                        "[winnability] SERP lookup failed for '{}': {}",
                        kw.keyword,
                        e
                    );
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::research::ScoredKeyword;

    fn kw(
        keyword: &str,
        volume: i64,
        kd: f64,
        intent: &str,
    ) -> ScoredKeyword {
        ScoredKeyword {
            keyword: keyword.to_string(),
            volume: Some(volume),
            kd: Some(kd),
            intent: Some(intent.to_string()),
            traffic: None,
            has_data: Some(true),
            intent_confidence: None,
        }
    }

    fn build_pipeline(keywords: Vec<ScoredKeyword>) -> KeywordPipelineOutput {
        KeywordPipelineOutput {
            keywords,
            themes: vec!["covered calls".to_string()],
            competitors: vec![],
            competitor_insights: vec![],
            total_candidates: 0,
            with_data_count: 0,
        }
    }

    #[test]
    fn blog_selection_prefers_informational_intent() {
        let pipeline = build_pipeline(vec![
            kw("how to sell covered calls", 1200, 25.0, "informational"),
            kw("covered call tracker", 800, 25.0, "commercial"),
            kw("what is a covered call", 3000, 20.0, "informational"),
        ]);
        let json = serde_json::to_string(&pipeline).unwrap();
        let (output, _) = select_keywords_deterministic(&json, false).unwrap();
        let results = output.difficulty.unwrap().results;
        assert!(
            results.iter().any(|r| r.keyword == "how to sell covered calls"),
            "informational keyword should be selected for blog"
        );
        assert!(
            !results.iter().any(|r| r.keyword == "covered call tracker"),
            "commercial keyword should not be selected for blog"
        );
    }

    #[test]
    fn landing_page_selection_prefers_commercial_intent() {
        let pipeline = build_pipeline(vec![
            kw("how to sell covered calls", 1200, 25.0, "informational"),
            kw("covered call tracker", 800, 25.0, "commercial"),
            kw("best covered call screener", 600, 30.0, "commercial"),
        ]);
        let json = serde_json::to_string(&pipeline).unwrap();
        let (output, _) = select_keywords_deterministic(&json, true).unwrap();
        let candidates = output.landing_page_candidates;
        assert!(
            candidates.iter().any(|c| c.keyword == "covered call tracker"),
            "commercial keyword should be selected for landing page"
        );
        assert!(
            !candidates.iter().any(|c| c.keyword == "how to sell covered calls"),
            "informational keyword should not be selected for landing page"
        );
    }

    #[test]
    fn selection_fails_when_nothing_matches_filters() {
        // All keywords exceed KD 30 — no fallback, the function should fail
        // with an actionable error rather than silently relaxing the bar.
        let pipeline = build_pipeline(vec![
            kw("how to sell covered calls", 1200, 55.0, "informational"),
            kw("covered call strike selection", 400, 50.0, "informational"),
        ]);
        let json = serde_json::to_string(&pipeline).unwrap();
        let result = select_keywords_deterministic(&json, false);
        assert!(
            result.is_err(),
            "should fail (not fallback) when no keywords meet the KD bar"
        );
        let err = result.unwrap_err();
        assert!(
            err.contains("No keywords met the quality bar"),
            "error should explain the failure: {}",
            err
        );
    }

    #[test]
    fn title_generation_matches_workflow() {
        assert_eq!(
            generate_title("how to sell covered calls", false),
            "How to Sell Covered Calls: A Step-by-Step Guide"
        );
        assert_eq!(
            generate_title("covered call tracker", true),
            "Covered Call Tracker for Options Traders"
        );
        assert_eq!(
            generate_title("optionstrat vs tastytrade", true),
            "Optionstrat vs Tastytrade: Which is Right for You?"
        );
    }
}
