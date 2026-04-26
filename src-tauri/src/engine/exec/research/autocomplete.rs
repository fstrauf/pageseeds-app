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
            message: "No themes found in research_seed_extraction artifact. Step 1 must run first.".to_string(),
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

                let suggestions = match crate::seo::google_autocomplete::fetch_suggestions(theme, "us", "en").await {
                    Ok(s) => s,
                    Err(e) => {
                        log::warn!("[research_autocomplete] Autocomplete failed for '{}': {}", theme, e);
                        vec![]
                    }
                };

                let suggestion_list: Vec<String> = suggestions
                    .iter()
                    .take(6)
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
            let json = serde_json::to_string_pretty(&results)
                .unwrap_or_else(|_| "[]".to_string());
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
/// This replaces the agentic step with pure Rust logic:
/// - Filters to keywords with data and KD <= target (default 10)
/// - Sorts by volume (desc), then difficulty (asc)
/// - Takes top N (default 10)
/// - Generates recommended titles based on keyword
pub fn select_keywords_deterministic(
    pipeline_json: &str,
    is_landing_page: bool,
) -> Result<KeywordPickerOutput, String> {
    // Parse pipeline output
    let pipeline: KeywordPipelineOutput = serde_json::from_str(pipeline_json)
        .map_err(|e| format!("Failed to parse pipeline output: {}", e))?;

    let target_kd = 30i64; // 0-100 scale (DataForSEO/Ahrefs unified)
    let max_results = 10usize;
    let total_candidates = pipeline.keywords.len();

    // Filter to keywords with data, acceptable KD, and non-navigational intent
    let mut candidates: Vec<_> = pipeline
        .keywords
        .into_iter()
        .filter(|k| {
            let has_data = k.has_data.unwrap_or(false);
            let kd_ok = k.kd.map(|d| d as i64 <= target_kd).unwrap_or(false);
            // Reject navigational intent (brand searches like "nike air force 1")
            let intent_ok = k.intent.as_deref()
                .map(|i| !i.eq_ignore_ascii_case("navigational"))
                .unwrap_or(true);
            has_data && kd_ok && intent_ok
        })
        .collect();

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
        Ok(KeywordPickerOutput {
            landing_page_candidates: selected
                .into_iter()
                .map(|k| LandingPageCandidate {
                    keyword: k.keyword.clone(),
                    estimated_volume: k.volume.unwrap_or(0),
                    estimated_kd: k.kd.unwrap_or(0.0) as i64,
                    intent: k.intent.clone().unwrap_or_else(|| "informational".to_string()),
                    landing_page_type: infer_landing_page_type(&k.keyword),
                    opportunity_score: "high".to_string(),
                    opportunity_reason: format!(
                        "KD {} with {} monthly searches",
                        k.kd.map(|d| d as i64).unwrap_or(0),
                        k.volume.unwrap_or(0)
                    ),
                    proposed_title: generate_title(&k.keyword),
                })
                .collect(),
            difficulty: None,
            total_candidates,
            filtered_out,
        })
    } else {
        let results: Vec<_> = selected
            .into_iter()
            .map(|k| SelectedKeyword {
                keyword: k.keyword.clone(),
                volume: k.volume.unwrap_or(0),
                difficulty: k.kd.unwrap_or(0.0) as i64,
                traffic: k.traffic.map(|t| t as i64),
                selection_reason: format!(
                    "Low difficulty (KD {}) with {} monthly searches",
                    k.kd.map(|d| d as i64).unwrap_or(0),
                    k.volume.unwrap_or(0)
                ),
                recommended_title: generate_title(&k.keyword),
            })
            .collect();

        let successful = results.len();
        Ok(KeywordPickerOutput {
            landing_page_candidates: Vec::new(),
            difficulty: Some(DifficultyWrapper {
                total: successful,
                successful,
                results,
            }),
            total_candidates,
            filtered_out,
        })
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
    } else if lower.contains("software") || lower.contains("tool") || lower.contains("app") {
        "feature".to_string()
    } else {
        "category".to_string()
    }
}

/// Generate a readable title from a keyword
fn generate_title(keyword: &str) -> String {
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
    
    // Add suffix based on keyword type
    let lower = keyword.to_lowercase();
    if lower.contains("how to") {
        format!("{}: A Step-by-Step Guide", title)
    } else if lower.contains("best") || lower.contains("top") {
        format!("{} for 2025", title)
    } else if lower.contains("vs") {
        format!("{}: Which is Right for You?", title)
    } else {
        format!("{}: Complete Guide", title)
    }
}

fn is_stop_word(word: &str) -> bool {
    matches!(
        word.to_lowercase().as_str(),
        "a" | "an" | "the" | "and" | "or" | "but" | "in" | "on" | "at" | "to" | "for" | "of" | "with"
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
                message: "No previous step output found — expected keyword pipeline results".to_string(),
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
        Ok(output) => {
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
                output.difficulty.as_ref().map(|d| d.results.len()).unwrap_or(0)
            };

            StepResult {
                success: true,
                message: format!(
                    "Selected {} keywords deterministically (KD <= 10, sorted by volume)",
                    count
                ),
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
