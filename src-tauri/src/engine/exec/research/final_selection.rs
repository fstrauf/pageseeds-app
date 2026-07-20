use crate::engine::workflows::StepResult;
use crate::models::research::{KeywordPipelineOutput, LandingPageCandidate, SelectedKeyword};
use crate::models::task::Task;

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
/// - Sort by volume (desc), then difficulty (asc), then coverage-gap score
///   (desc, `None` last among equals).
/// - Take top `max_results` (callers may overshoot to leave room for the
///   downstream relevance check to drop off-domain candidates).
/// - Generate recommended titles based on keyword type.
pub fn select_keywords_deterministic(
    pipeline_json: &str,
    is_landing_page: bool,
    max_results: usize,
) -> Result<(KeywordPickerOutput, bool), String> {
    // Parse pipeline output
    let pipeline: KeywordPipelineOutput = serde_json::from_str(pipeline_json)
        .map_err(|e| format!("Failed to parse pipeline output: {}", e))?;

    let target_kd = 30i64; // 0-100 scale (DataForSEO/Ahrefs unified)
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

    // Sort: landing page candidates rank by commercial value (volume × CPC) —
    // the standard proxy for conversion-page value — falling back to plain
    // volume when CPC is unavailable. Blog candidates rank by volume. Ties
    // break by KD asc, then coverage-gap score desc, preserving the
    // "prioritize thin clusters" intent from the coverage filter.
    candidates.sort_by(|a, b| {
        if is_landing_page {
            let val_cmp = commercial_value(b)
                .partial_cmp(&commercial_value(a))
                .unwrap_or(std::cmp::Ordering::Equal);
            if val_cmp != std::cmp::Ordering::Equal {
                return val_cmp;
            }
        }
        let vol_cmp = b.volume.unwrap_or(0).cmp(&a.volume.unwrap_or(0));
        if vol_cmp != std::cmp::Ordering::Equal {
            return vol_cmp;
        }
        let kd_a = a.kd.unwrap_or(100.0) as i64;
        let kd_b = b.kd.unwrap_or(100.0) as i64;
        let kd_cmp = kd_a.cmp(&kd_b);
        if kd_cmp != std::cmp::Ordering::Equal {
            return kd_cmp;
        }
        cmp_gap_desc(a.gap_score, b.gap_score)
    });

    // Take top N
    let selected: Vec<_> = candidates.into_iter().take(max_results).collect();
    let filtered_out = total_candidates.saturating_sub(selected.len());

    if is_landing_page {
        // Opportunity tiers derive from the same commercial-value score used
        // for ranking: the top candidate sets the scale, others are bucketed
        // relative to it. When no CPC data exists at all, volume is the score.
        let max_value = selected
            .iter()
            .map(commercial_value)
            .fold(0.0f64, f64::max);
        let max_volume = selected
            .iter()
            .map(|k| k.volume.unwrap_or(0))
            .max()
            .unwrap_or(0) as f64;
        let (score_of, max_score): (Box<dyn Fn(&crate::models::research::ScoredKeyword) -> f64>, f64) =
            if max_value > 0.0 {
                (Box::new(commercial_value), max_value)
            } else {
                (Box::new(|k| k.volume.unwrap_or(0) as f64), max_volume)
            };
        Ok((KeywordPickerOutput {
            landing_page_candidates: selected
                .into_iter()
                .map(|k| {
                    let kd = k.kd.map(|d| d as i64).unwrap_or(0);
                    let volume = k.volume.unwrap_or(0);
                    LandingPageCandidate {
                        keyword: k.keyword.clone(),
                        estimated_volume: volume,
                        estimated_kd: kd,
                        intent: k
                            .intent
                            .clone()
                            .unwrap_or_else(|| "informational".to_string()),
                        landing_page_type: infer_landing_page_type(&k.keyword),
                        opportunity_score: opportunity_tier(score_of(&k), max_score).to_string(),
                        opportunity_reason: match k.cpc {
                            Some(cpc) => format!(
                                "KD {} with {} monthly searches, ${:.2} CPC",
                                kd, volume, cpc
                            ),
                            None => format!("KD {} with {} monthly searches", kd, volume),
                        },
                        proposed_title: generate_title(&k.keyword, true),
                        cpc: k.cpc,
                        // Populated by enrich_with_winnability() after selection,
                        // before the final sort and trim.
                        winnability: None,
                        winnability_reason: None,
                    }
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
                intent: k.intent.clone(),
                // Populated by enrich_with_winnability() after selection,
                // before the final sort and trim.
                winnability: None,
                winnability_reason: None,
                gap_score: k.gap_score,
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

/// Commercial value proxy for a landing page candidate: expected monthly
/// organic visits worth their equivalent paid cost. Zero when CPC is unknown,
/// in which case callers fall back to volume.
fn commercial_value(k: &crate::models::research::ScoredKeyword) -> f64 {
    k.volume.unwrap_or(0) as f64 * k.cpc.unwrap_or(0.0)
}

/// Bucket a candidate's commercial-value score relative to the shortlist's
/// best. Deterministic replacement for the previously hardcoded "high".
fn opportunity_tier(score: f64, max_score: f64) -> &'static str {
    if max_score <= 0.0 {
        return "medium";
    }
    let ratio = score / max_score;
    if ratio >= 0.66 {
        "high"
    } else if ratio >= 0.33 {
        "medium"
    } else {
        "low"
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
/// Titles must stay site-agnostic — no product names or audience hardcoding.
pub(crate) fn generate_title(keyword: &str, is_landing_page: bool) -> String {
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
    let year = chrono::Datelike::year(&chrono::Utc::now());

    if is_landing_page {
        if lower.contains("vs") {
            format!("{}: Which is Right for You?", title)
        } else if lower.contains("best") || lower.contains("top") {
            format!("{} ({})", title, year)
        } else if lower.contains("alternative") || lower.contains("alternatives") {
            format!("The Best {} Alternative", title)
        } else {
            title
        }
    } else {
        if lower.contains("how to") {
            format!("{}: A Step-by-Step Guide", title)
        } else if lower.contains("what is") || lower.contains("what are") {
            format!("{} Explained", title)
        } else if lower.contains("best") || lower.contains("top") {
            format!("{} ({})", title, year)
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

/// Final shortlist size after relevance filtering.
pub(crate) const FINAL_RESULTS: usize = 10;
/// How many candidates to select before the relevance check, so off-domain
/// removals don't shrink the final shortlist below FINAL_RESULTS.
const RELEVANCE_OVERSHOOT: usize = 15;

/// Execute the final selection step.
///
/// This is called by the executor when it encounters a step with kind "research_final_selection".
/// It reads the previous step's output (keyword pipeline results), applies deterministic
/// filtering/sorting, then runs one batched agentic relevance check to drop
/// off-domain candidates before writing the artifact.
pub fn exec_research_final_selection(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
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

    match select_keywords_deterministic(pipeline_json, is_landing_page, RELEVANCE_OVERSHOOT) {
        Ok((mut output, used_fallback)) => {
            // Agentic relevance check: DataForSEO expansion can return
            // same-vocabulary but off-domain candidates (e.g. "assignment risk
            // ao3" from an options-trading seed). Cannot be deterministic:
            // telling "ao3" (off-domain) apart from "61-day" (on-domain new
            // term) requires domain judgment. Non-fatal — on failure the
            // deterministic shortlist stands and the human reviewer decides.
            let themes: Vec<String> = serde_json::from_str::<KeywordPipelineOutput>(pipeline_json)
                .map(|p| p.themes)
                .unwrap_or_default();
            let removed = filter_off_domain_candidates(
                &mut output,
                &themes,
                project_path,
                agent_provider,
            );

            // Enrich the overshoot with winnability scores (AIO risk,
            // competitor authority) BEFORE trimming, so an `Avoid` verdict can
            // demote a keyword below the cut line instead of being computed
            // and discarded. Non-fatal per keyword: a failed SERP lookup
            // leaves the keyword unscored and it sorts as Target-equivalent.
            enrich_with_winnability(&mut output, &task.project_id);

            // Re-sort by the combined key (winnability bucket, then volume,
            // KD, gap score) and only then trim: `Avoid` keywords drop out of
            // the picker whenever enough better candidates exist, and remain
            // (badged, at the bottom) when they don't.
            sort_by_winnability(&mut output);
            trim_to_final(&mut output, FINAL_RESULTS);
            let final_count = selected_count(&output);
            output.filtered_out = output.total_candidates.saturating_sub(final_count);

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

            let relevance_note = if removed > 0 {
                format!(", {} off-domain removed", removed)
            } else {
                String::new()
            };
            let msg = if used_fallback {
                format!(
                    "Selected {} keywords (API data unavailable; showing best candidates without KD/volume filters{})",
                    final_count, relevance_note
                )
            } else {
                format!(
                    "Selected {} keywords deterministically (KD <= 30, winnability-aware ranking{})",
                    final_count, relevance_note
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

pub(crate) fn selected_count(output: &KeywordPickerOutput) -> usize {
    if !output.landing_page_candidates.is_empty() {
        output.landing_page_candidates.len()
    } else {
        output
            .difficulty
            .as_ref()
            .map(|d| d.results.len())
            .unwrap_or(0)
    }
}

/// Truncate both output shapes to `max` entries (post-relevance-check).
pub(crate) fn trim_to_final(output: &mut KeywordPickerOutput, max: usize) {
    output.landing_page_candidates.truncate(max);
    if let Some(d) = &mut output.difficulty {
        d.results.truncate(max);
        d.total = d.results.len();
        d.successful = d.results.len();
    }
}

/// Winnability bucket sort rank: `target` and unknown/missing buckets rank 0
/// (keywords whose enrichment failed keep pre-enrichment behavior),
/// `differentiate` ranks 1, `avoid` ranks last. Values are the lowercase
/// strings written by `WinnabilityBucket::as_str()`.
fn winnability_rank(winnability: Option<&str>) -> u8 {
    match winnability {
        Some("differentiate") => 1,
        Some("avoid") => 2,
        _ => 0,
    }
}

/// Gap-score tiebreak: higher score first; `None` (no coverage analysis was
/// available) sorts last among equals. `total_cmp` keeps f64 ordering total
/// and deterministic.
fn cmp_gap_desc(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(x), Some(y)) => y.total_cmp(&x),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// Final selection sort, applied after winnability enrichment and before the
/// trim to `FINAL_RESULTS`. Combined key, in priority order:
///   1. Winnability bucket rank — target/unknown, then differentiate, avoid last.
///   2. Volume descending (landing pages: commercial value — volume × CPC —
///      descending, then volume).
///   3. KD ascending.
///   4. Coverage-gap score descending (`None` last among equals).
/// The sort is stable, so fully-equal keys keep their prior (deterministic)
/// order.
pub(crate) fn sort_by_winnability(output: &mut KeywordPickerOutput) {
    if !output.landing_page_candidates.is_empty() {
        output.landing_page_candidates.sort_by(|a, b| {
            winnability_rank(a.winnability.as_deref())
                .cmp(&winnability_rank(b.winnability.as_deref()))
                .then_with(|| {
                    lp_commercial_value(b)
                        .partial_cmp(&lp_commercial_value(a))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .then_with(|| b.estimated_volume.cmp(&a.estimated_volume))
                .then_with(|| a.estimated_kd.cmp(&b.estimated_kd))
        });
        return;
    }
    if let Some(d) = &mut output.difficulty {
        d.results.sort_by(|a, b| {
            winnability_rank(a.winnability.as_deref())
                .cmp(&winnability_rank(b.winnability.as_deref()))
                .then_with(|| b.volume.cmp(&a.volume))
                .then_with(|| a.difficulty.cmp(&b.difficulty))
                .then_with(|| cmp_gap_desc(a.gap_score, b.gap_score))
        });
    }
}

/// Commercial value of a landing candidate after selection (volume × CPC).
fn lp_commercial_value(c: &LandingPageCandidate) -> f64 {
    c.estimated_volume as f64 * c.cpc.unwrap_or(0.0)
}

/// Apply an off-domain list to the shortlist (case-insensitive, trimmed).
/// Pure — unit-tested without an LLM. Returns the number removed.
pub(crate) fn apply_off_domain_filter(
    output: &mut KeywordPickerOutput,
    off_domain: &std::collections::HashSet<String>,
) -> usize {
    if off_domain.is_empty() {
        return 0;
    }
    let before = selected_count(output);
    output
        .landing_page_candidates
        .retain(|c| !off_domain.contains(&c.keyword.trim().to_lowercase()));
    if let Some(d) = &mut output.difficulty {
        d.results
            .retain(|k| !off_domain.contains(&k.keyword.trim().to_lowercase()));
    }
    before - selected_count(output)
}

/// One batched LLM call flagging off-domain candidates in the shortlist.
/// Non-fatal: returns 0 (keeps everything) when the check is unavailable.
fn filter_off_domain_candidates(
    output: &mut KeywordPickerOutput,
    themes: &[String],
    project_path: &str,
    agent_provider: &str,
) -> usize {
    let keywords: Vec<String> = if !output.landing_page_candidates.is_empty() {
        output
            .landing_page_candidates
            .iter()
            .map(|c| c.keyword.clone())
            .collect()
    } else {
        output
            .difficulty
            .as_ref()
            .map(|d| d.results.iter().map(|k| k.keyword.clone()).collect())
            .unwrap_or_default()
    };
    if keywords.is_empty() {
        return 0;
    }

    let brief = std::fs::read_to_string(
        crate::engine::project_paths::ProjectPaths::from_path(project_path)
            .automation_dir
            .join("project.md"),
    )
    .unwrap_or_else(|_| "(no brief found)".to_string());
    const MAX_BRIEF_LEN: usize = 8_000;
    let brief_trimmed = if brief.len() > MAX_BRIEF_LEN {
        format!("{}… [truncated]", &brief[..MAX_BRIEF_LEN])
    } else {
        brief
    };

    let system = include_str!("../../../prompts/candidate_relevance.md");
    let user = format!(
        "## Project Context\n\n{}\n\n## Research Themes\n\n{}\n\n## Candidate Keywords\n\n{}",
        brief_trimmed,
        themes.join(", "),
        keywords.join("\n")
    );
    let prompt = format!("{}\n\n---\n\n{}", system, user);

    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[relevance_check] Failed to create runtime: {}", e);
            return 0;
        }
    };

    let result = rt.block_on(async {
        crate::rig::extraction::extract_structured::<
            crate::models::research::CandidateRelevanceOutput,
        >(agent_provider, &prompt, Some(system), Some("direct"), None)
        .await
    });

    match result {
        Ok(check) => {
            let off_domain: std::collections::HashSet<String> = check
                .off_domain_keywords
                .iter()
                .map(|k| k.trim().to_lowercase())
                .filter(|k| !k.is_empty())
                .collect();
            let removed = apply_off_domain_filter(output, &off_domain);
            if removed > 0 {
                log::info!(
                    "[relevance_check] removed {} off-domain candidates: {:?}",
                    removed,
                    off_domain
                );
            } else {
                log::info!("[relevance_check] all {} candidates on-domain", keywords.len());
            }
            removed
        }
        Err(e) => {
            log::warn!(
                "[relevance_check] extraction failed ({}); keeping deterministic shortlist",
                e
            );
            0
        }
    }
}

/// Enrich shortlisted keywords with winnability scores using SERP feature data.
///
/// Runs on the pre-trim overshoot (up to `RELEVANCE_OVERSHOOT` keywords), so
/// the paid SERP verdict feeds back into selection via `sort_by_winnability`
/// instead of being computed and discarded. Covers both blog selections
/// (`difficulty.results`) and landing page candidates — authority-dominated
/// commercial SERPs must be flagged before selection too. Calls the DataForSEO
/// SERP API for each keyword and scores it using the winnability classifier
/// (Target / Differentiate / Avoid). Non-fatal: if the provider is unavailable
/// or a SERP lookup fails, the keyword keeps its existing fields without a
/// winnability score.
fn enrich_with_winnability(output: &mut KeywordPickerOutput, project_id: &str) {
    let landing_count = output.landing_page_candidates.len();
    let blog_count = output
        .difficulty
        .as_ref()
        .map(|d| d.results.len())
        .unwrap_or(0);
    if landing_count == 0 && blog_count == 0 {
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
            landing_count + blog_count,
            provider_name
        );

        if !output.landing_page_candidates.is_empty() {
            for kw in output.landing_page_candidates.iter_mut() {
                match provider.serp_features(&kw.keyword, "us").await {
                    Ok(serp) => {
                        let assessment = crate::seo::winnability::assess(
                            &kw.keyword,
                            &serp,
                            Some(kw.estimated_kd as f64),
                            Some(kw.intent.as_str()),
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
        } else if let Some(d) = &mut output.difficulty {
            for kw in d.results.iter_mut() {
                match provider.serp_features(&kw.keyword, "us").await {
                    Ok(serp) => {
                        let assessment = crate::seo::winnability::assess(
                            &kw.keyword,
                            &serp,
                            Some(kw.difficulty as f64),
                            kw.intent.as_deref(),
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
        }
    });
}
