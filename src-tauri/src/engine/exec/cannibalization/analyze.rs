//! Step 6: Agentic candidate analysis.

use super::*;

use std::collections::HashSet;
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

const MAX_CANDIDATE_PAGES: usize = 4;
const MIN_CANDIDATE_PAGES: usize = 2;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Analyze Candidates
// ═══════════════════════════════════════════════════════════════════════════════

/// Agentic analysis of individual merge candidates with byte-budgeted prompts.
///
/// Why not deterministic: each candidate is a cluster of 2–8 pages competing for the
/// same keyword(s). Deciding which page to keep, which to redirect, and how to merge
/// unique valuable content requires judgment about content quality, user intent,
/// URL authority, and GSC performance. No finite rule set can correctly resolve all
/// valid inputs because the "best" keeper depends on nuanced semantic comparison.
/// The output is a structured `CandidateAnalysisOutput` per candidate, extracted
/// via Rig's `extract_structured`.
///
/// Reads `cannibalization_candidates.json`, enriches pages with article-evidence
/// packages (outline / real word_count / top queries), applies product guards
/// beyond ID resolution, calls the agent once per candidate, and writes
/// `cannibalization_batch_outputs.json`.
pub(crate) fn exec_can_analyze_candidates(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let repo_root = Path::new(project_path);

    // Load candidates from DB (primary) or JSON fallback
    let candidates_doc: serde_json::Value = {
        let db_doc = rusqlite::Connection::open(crate::db::default_db_path())
            .ok()
            .and_then(|conn| {
                crate::db::content_audit::get_latest_audit_artifact(
                    &conn,
                    &task.project_id,
                    "cannibalization_candidates",
                )
                .ok()
                .flatten()
            });
        match db_doc {
            Some(v) => v,
            None => {
                let candidates_path = paths.automation_dir.join("cannibalization_candidates.json");
                match crate::engine::exec::common::read_json(
                    &candidates_path,
                    "cannibalization_candidates.json",
                ) {
                    Ok(v) => v,
                    Err(e) => return e,
                }
            }
        }
    };

    let candidates = candidates_doc["candidates"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let candidates_len = candidates.len();
    if candidates.is_empty() {
        return StepResult {
            success: true,
            message: "No candidates to analyze.".to_string(),
            output: None,
            artifact_key: None,
        };
    }

    const TARGET_PROMPT_BYTES: usize = 15_000;
    const HARD_PROMPT_BYTES: usize = 20_000;

    let skill = match crate::engine::skills::load_skill_or_fail(repo_root, "cannibalization-strategy")
    {
        Ok(s) => s,
        Err(msg) => {
            return StepResult::fail(msg);
        }
    };

    // Output-contract assertion: a stale URL-based skill copy (keep_url/redirect_urls)
    // deserializes into `CandidateAnalysisOutput` with keep_id=0, and the
    // id-resolution guard below then silently converts every recommendation to
    // no_action. Fail loudly instead of producing an empty strategy.
    if !skill.content.contains("keep_id") {
        return StepResult::fail(
            "Skill 'cannibalization-strategy' does not use the keep_id/redirect_ids output contract — it appears to be a stale URL-based copy. Delete .github/skills/cannibalization-strategy/SKILL.md from the project repo to use the embedded app default.".to_string(),
        );
    }

    let db_conn = rusqlite::Connection::open(crate::db::default_db_path()).ok();

    let mut batch_outputs: Vec<serde_json::Value> = Vec::new();
    let mut failed_candidates: Vec<String> = Vec::new();
    let mut guard_degraded_count: usize = 0;

    for candidate in &candidates {
        let candidate_id = candidate["candidate_id"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        // Enrich pages with article-evidence packages before prompting.
        let enriched = enrich_candidate_with_packages(
            db_conn.as_ref(),
            &task.project_id,
            candidate,
        );

        // Pre-LLM product guards (lane / page count). Multi-intent is applied
        // after the model returns so we can override a bad merge recommendation.
        if let Some(reason) = pre_llm_product_guard_reason(&enriched) {
            guard_degraded_count += 1;
            log::warn!(
                "[cannibalization_audit] Candidate {}: pre-LLM product guard — {}",
                candidate_id,
                reason
            );
            batch_outputs.push(serde_json::json!({
                "candidate_id": candidate_id,
                "success": true,
                "message": "Degraded by product guard (pre-LLM)",
                "merge_recommendation": {
                    "cluster_id": candidate_id,
                    "cluster_theme": enriched["theme"].as_str().unwrap_or(""),
                    "keep_id": 0,
                    "redirect_ids": [],
                    "merge_before_redirect": false,
                    "merge_instructions": [],
                    "reason": reason,
                    "no_action": true,
                    "confidence": "low",
                },
            }));
            continue;
        }

        let (prompt, prompt_bytes) = build_merge_prompt(&skill.content, &enriched);

        let chosen_prompt = if prompt_bytes > HARD_PROMPT_BYTES {
            let (trimmed, trimmed_bytes) = build_merge_prompt_trimmed(&skill.content, &enriched);
            if trimmed_bytes > HARD_PROMPT_BYTES {
                log::warn!(
                    "[cannibalization_audit] Candidate {} still exceeds hard limit after trimming ({} bytes). Skipping.",
                    candidate_id,
                    trimmed_bytes
                );
                failed_candidates.push(candidate_id.clone());
                batch_outputs.push(serde_json::json!({
                    "candidate_id": candidate_id,
                    "success": false,
                    "message": format!("Prompt exceeded hard limit ({} bytes)", trimmed_bytes),
                    "merge_recommendation": null,
                }));
                continue;
            }
            log::info!(
                "[cannibalization_audit] Candidate {} trimmed from {} to {} bytes",
                candidate_id,
                prompt_bytes,
                trimmed_bytes
            );
            trimmed
        } else {
            prompt
        };

        // Additional safety: warn if we're over target but under hard
        if chosen_prompt.len() > TARGET_PROMPT_BYTES && chosen_prompt.len() <= HARD_PROMPT_BYTES {
            log::info!(
                "[cannibalization_audit] Candidate {} prompt is {} bytes (over target {})",
                candidate_id,
                chosen_prompt.len(),
                TARGET_PROMPT_BYTES
            );
        }

        // Run the structured extractor inside a fresh runtime because this
        // function is called from within tokio::task::spawn_blocking.
        let extract_result = {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    log::warn!(
                        "[cannibalization_audit] Failed to create runtime for candidate {}: {}",
                        candidate_id,
                        e
                    );
                    failed_candidates.push(candidate_id.clone());
                    batch_outputs.push(serde_json::json!({
                        "candidate_id": candidate_id,
                        "success": false,
                        "message": format!("Runtime error: {}", e),
                        "merge_recommendation": null,
                    }));
                    continue;
                }
            };
            rt.block_on(async {
                crate::rig::extraction::extract_structured::<
                    crate::models::cannibalization::CandidateAnalysisOutput,
                >(
                    agent_provider,
                    &chosen_prompt,
                    Some("You are an expert SEO strategist. Analyze the candidate and return structured JSON."),
                    Some("direct"),
                    None,
                )
                .await
            })
        };

        match extract_result {
            Ok(mut rec) => {
                // Defensive normalization: ensure required fields are present.
                if rec.cluster_id.is_empty() {
                    rec.cluster_id = candidate_id.clone();
                }
                if rec.cluster_theme.is_empty() {
                    rec.cluster_theme = enriched["theme"].as_str().unwrap_or("").to_string();
                }
                if rec.confidence.is_empty() {
                    rec.confidence = "medium".to_string();
                }

                // ── Product guards (post-LLM) ─────────────────────────────────
                if let Some(reason) = apply_post_llm_product_guards(&enriched, &mut rec) {
                    guard_degraded_count += 1;
                    log::warn!(
                        "[cannibalization_audit] Candidate {}: post-LLM product guard — {}",
                        candidate_id,
                        reason
                    );
                }

                // ── Deterministic URL resolution ──────────────────────────────
                // The agent selects pages by stable `id`; we resolve those ids
                // to canonical `/blog/<slug>` URLs here. The agent never owns URL
                // strings, so it cannot introduce malformed (e.g. underscored)
                // slugs into the merge plan. Any id that does not resolve to a
                // page in the candidate set is treated as no_action with a reason.
                let page_url_by_id: std::collections::HashMap<i64, String> = enriched["pages"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| {
                        let id = p["id"].as_i64()?;
                        let url = crate::content::slug::format_blog_link(
                            p["url"]
                                .as_str()
                                .unwrap_or(p["url_slug"].as_str().unwrap_or("")),
                        );
                        Some((id, url))
                    })
                    .collect();

                let keep_url: Option<String> = (rec.keep_id > 0)
                    .then(|| page_url_by_id.get(&rec.keep_id).cloned())
                    .flatten();
                let missing_redirect_ids: Vec<i64> = rec
                    .redirect_ids
                    .iter()
                    .filter(|id| !page_url_by_id.contains_key(id))
                    .copied()
                    .collect();
                let redirect_urls: Vec<String> = rec
                    .redirect_ids
                    .iter()
                    .filter_map(|id| page_url_by_id.get(id).cloned())
                    .collect();

                if !rec.no_action {
                    if keep_url.is_none() {
                        rec.no_action = true;
                        rec.reason = format!(
                            "Model returned keep_id={} which is not in the candidate page set; cannot resolve a canonical keeper URL.",
                            rec.keep_id
                        );
                        guard_degraded_count += 1;
                        log::warn!(
                            "[cannibalization_audit] Candidate {}: recommendation degraded to no_action — keep_id={} is not in the candidate page set",
                            candidate_id,
                            rec.keep_id
                        );
                    } else if !missing_redirect_ids.is_empty() {
                        rec.no_action = true;
                        rec.reason = format!(
                            "Model returned redirect_ids not present in the candidate page set: {:?}",
                            missing_redirect_ids
                        );
                        guard_degraded_count += 1;
                        log::warn!(
                            "[cannibalization_audit] Candidate {}: recommendation degraded to no_action — redirect_ids not in the candidate page set: {:?}",
                            candidate_id,
                            missing_redirect_ids
                        );
                    }
                }

                let mut rec_json = match serde_json::to_value(&rec) {
                    Ok(v) => v,
                    Err(e) => {
                        log::warn!(
                            "[cannibalization_audit] Failed to serialize analysis for candidate {}: {}",
                            candidate_id,
                            e
                        );
                        failed_candidates.push(candidate_id.clone());
                        batch_outputs.push(serde_json::json!({
                            "candidate_id": candidate_id,
                            "success": false,
                            "message": format!("Serialize error: {}", e),
                            "merge_recommendation": null,
                        }));
                        continue;
                    }
                };
                // Inject the deterministically-resolved canonical URLs so the
                // batch output (and downstream reducer) carries both the agent's
                // id selection and the resolved `/blog/<slug>` strings.
                if let Some(obj) = rec_json.as_object_mut() {
                    if let Some(ku) = keep_url {
                        obj.insert("keep_url".to_string(), serde_json::Value::String(ku));
                    }
                    obj.insert(
                        "redirect_urls".to_string(),
                        serde_json::Value::Array(
                            redirect_urls
                                .into_iter()
                                .map(serde_json::Value::String)
                                .collect(),
                        ),
                    );
                }
                batch_outputs.push(serde_json::json!({
                    "candidate_id": candidate_id,
                    "success": true,
                    "message": "Analyzed successfully",
                    "merge_recommendation": rec_json,
                }));
            }
            Err(e) => {
                log::warn!(
                    "[cannibalization_audit] Structured extraction failed for candidate {}: {}",
                    candidate_id,
                    e
                );
                failed_candidates.push(candidate_id.clone());
                batch_outputs.push(serde_json::json!({
                    "candidate_id": candidate_id,
                    "success": false,
                    "message": format!("Extraction error: {}", e),
                    "merge_recommendation": null,
                }));
            }
        }
    }

    let batch_doc = serde_json::json!({
        "generated_at": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "batch_outputs": batch_outputs,
        "failed_candidates": failed_candidates,
        "guard_degraded_count": guard_degraded_count,
    });

    let batch_path = paths
        .automation_dir
        .join("cannibalization_batch_outputs.json");
    if let Err(e) = std::fs::write(
        &batch_path,
        serde_json::to_string_pretty(&batch_doc).unwrap_or_default() + "\n",
    ) {
        log::warn!(
            "[cannibalization_audit] Failed to write batch outputs: {}",
            e
        );
    }

    let success_count = batch_outputs
        .iter()
        .filter(|o| o["success"].as_bool().unwrap_or(false))
        .count();

    if guard_degraded_count > 0 {
        log::warn!(
            "[cannibalization_audit] {} recommendation(s) degraded by product / id-resolution guards",
            guard_degraded_count
        );
    }

    StepResult {
        success: failed_candidates.is_empty() || success_count > 0,
        message: format!(
            "Analyzed {}/{} candidates successfully. Failed: {}. Degraded by product/id-resolution guards: {}",
            success_count,
            candidates_len,
            failed_candidates.len(),
            guard_degraded_count
        ),
        output: Some(serde_json::to_string_pretty(&batch_doc).unwrap_or_default()),
        artifact_key: None,
    }
}

// ─── Package enrichment ───────────────────────────────────────────────────────

/// Attach real `word_count`, `outline_text`, and `top_queries` from article
/// evidence (and optionally `ctr_query_metrics`) so the agent can judge merge
/// intent. Thin 60-word `excerpt` remains only as a last-resort fallback.
pub(crate) fn enrich_candidate_with_packages(
    conn: Option<&Connection>,
    project_id: &str,
    candidate: &serde_json::Value,
) -> serde_json::Value {
    let mut enriched = candidate.clone();
    let Some(conn) = conn else {
        return enriched;
    };
    let Some(pages) = enriched["pages"].as_array_mut() else {
        return enriched;
    };

    for page in pages.iter_mut() {
        let article_id = match page["id"].as_i64() {
            Some(id) if id > 0 => id,
            _ => continue,
        };

        if let Ok(Some(row)) =
            crate::content::article_evidence::get_row_by_article_id(conn, project_id, article_id)
        {
            if row.word_count > 0 {
                page["word_count"] = serde_json::json!(row.word_count);
            }
            if let Some(outline) = row.outline_text {
                if !outline.trim().is_empty() {
                    page["outline_text"] = serde_json::Value::String(outline);
                }
            }
            // Prefer evidence top_queries_json when non-empty.
            if !row.top_queries_json.trim().is_empty() && row.top_queries_json.trim() != "[]" {
                if let Ok(queries) =
                    serde_json::from_str::<serde_json::Value>(&row.top_queries_json)
                {
                    page["top_queries"] = queries;
                }
            }
            if let Some(h1) = row.h1 {
                if page["h1"].as_str().unwrap_or("").is_empty() {
                    page["h1"] = serde_json::Value::String(h1);
                }
            }
            if let Some(title) = row.title {
                if page["title"].as_str().unwrap_or("").is_empty() {
                    page["title"] = serde_json::Value::String(title);
                }
            }
            if let Some(kw) = row.target_keyword {
                if page["target_keyword"].as_str().unwrap_or("").is_empty() {
                    page["target_keyword"] = serde_json::Value::String(kw);
                }
            }
        }

        // Fill top_queries from ctr_query_metrics when still missing.
        let needs_queries = page
            .get("top_queries")
            .map(|v| v.as_array().map(|a| a.is_empty()).unwrap_or(true))
            .unwrap_or(true);
        if needs_queries {
            if let Ok(rows) = crate::db::get_ctr_query_metrics(conn, project_id, article_id) {
                if !rows.is_empty() {
                    let top: Vec<serde_json::Value> = rows
                        .into_iter()
                        .take(10)
                        .map(|r| {
                            serde_json::json!({
                                "query": r.query,
                                "impressions": r.impressions,
                                "clicks": r.clicks,
                                "avg_position": r.avg_position,
                            })
                        })
                        .collect();
                    page["top_queries"] = serde_json::Value::Array(top);
                }
            }
        }
    }

    enriched
}

// ─── Product guards ───────────────────────────────────────────────────────────

/// Pre-LLM guards that skip the agent entirely (invalid lane / page count).
/// Returns a human-readable reason when the candidate should be no_action.
pub(crate) fn pre_llm_product_guard_reason(candidate: &serde_json::Value) -> Option<String> {
    let raw_lane = candidate["lane"].as_str().unwrap_or("").trim();
    if EvidenceLane::parse(raw_lane).is_none() {
        return Some(format!(
            "Candidate missing or invalid lane (got {:?}); only exact_keyword, shared_query, near_dupe are allowed.",
            if raw_lane.is_empty() {
                "<empty>"
            } else {
                raw_lane
            }
        ));
    }

    let page_count = candidate["pages"]
        .as_array()
        .map(|a| a.len())
        .unwrap_or(0);
    if page_count < MIN_CANDIDATE_PAGES || page_count > MAX_CANDIDATE_PAGES {
        return Some(format!(
            "Candidate has {} pages; merge shortlist requires 2–{} pages.",
            page_count, MAX_CANDIDATE_PAGES
        ));
    }

    None
}

/// Post-LLM product guards. Mutates `rec` when a guard fires.
/// Returns Some(reason) when a guard degraded the recommendation.
///
/// Multi-intent without shared query (near_dupe only):
/// If pages have distinct non-empty target_keywords AND shared_query_count == 0 /
/// empty shared queries, force `no_action`. shared_query and exact_keyword lanes
/// are exempt (exact has same keyword; shared_query already has SERP evidence).
pub(crate) fn apply_post_llm_product_guards(
    candidate: &serde_json::Value,
    rec: &mut crate::models::cannibalization::CandidateAnalysisOutput,
) -> Option<String> {
    let lane = EvidenceLane::parse(candidate["lane"].as_str().unwrap_or(""));

    // Multi-intent near_dupe without shared queries → force no_action.
    if lane == Some(EvidenceLane::NearDupe) && !rec.no_action {
        if is_multi_intent_without_shared_query(candidate) {
            rec.no_action = true;
            rec.confidence = "low".to_string();
            rec.reason = "near_dupe multi-intent without shared query: pages have distinct target_keywords and no shared SERP queries; refusing merge.".to_string();
            return Some(rec.reason.clone());
        }
    }

    None
}

/// True when pages have ≥2 distinct non-empty target_keywords AND no shared queries.
///
/// Primary field is `shared_queries`; `top_shared_queries` is accepted as a
/// legacy alias when reading older artifacts.
pub(crate) fn is_multi_intent_without_shared_query(candidate: &serde_json::Value) -> bool {
    if has_shared_query_evidence(candidate) {
        return false;
    }

    let mut keywords: HashSet<String> = HashSet::new();
    if let Some(pages) = candidate["pages"].as_array() {
        for p in pages {
            let kw = p["target_keyword"]
                .as_str()
                .unwrap_or("")
                .trim()
                .to_lowercase();
            if !kw.is_empty() {
                keywords.insert(kw);
            }
        }
    }
    keywords.len() >= 2
}

fn has_shared_query_evidence(candidate: &serde_json::Value) -> bool {
    if candidate["shared_query_count"].as_u64().unwrap_or(0) > 0 {
        return true;
    }
    let primary_nonempty = candidate["shared_queries"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    if primary_nonempty {
        return true;
    }
    // Legacy alias when primary is missing or empty.
    candidate["top_shared_queries"]
        .as_array()
        .map(|a| !a.is_empty())
        .unwrap_or(false)
}

/// Build the full merge-analysis prompt for a single candidate.
pub(crate) fn build_merge_prompt(skill_content: &str, candidate: &serde_json::Value) -> (String, usize) {
    let candidate_json = serde_json::to_string_pretty(candidate).unwrap_or_default();
    let prompt = skill_content.to_string()
        + "\n\n---\n\n## Merge Candidate\n\n"
        + &candidate_json
        + "\n\nAnalyze ONLY this candidate cluster. Decide if the pages represent true cannibalization (same search intent competing in SERPs) or just topical similarity.\n\n"
        + "If true cannibalization: recommend keep_id (the page id to keep), redirect_ids (page ids to redirect), and merge instructions.\n"
        + "If not: return no_action with a reason.\n\n"
        + "CRITICAL: Select pages by numeric id only (keep_id / redirect_ids). Never emit URLs — the app resolves ids to canonical URLs.\n"
        + "CRITICAL: Return ONLY a single JSON object matching the Output Contract. Do not include markdown prose outside the JSON.";
    let bytes = prompt.len();
    (prompt, bytes)
}

/// Build a trimmed prompt without bulky page fields (second-level budget fallback).
pub(crate) fn build_merge_prompt_trimmed(
    skill_content: &str,
    candidate: &serde_json::Value,
) -> (String, usize) {
    let mut trimmed = candidate.clone();
    if let Some(pages) = trimmed["pages"].as_array_mut() {
        for page in pages {
            if let serde_json::Value::Object(ref mut map) = page {
                map.remove("excerpt");
                // Keep outline_text / top_queries as primary evidence; only drop
                // if still over budget would require a third pass — not needed yet.
            }
        }
    }
    let candidate_json = serde_json::to_string_pretty(&trimmed).unwrap_or_default();
    let prompt = skill_content.to_string()
        + "\n\n---\n\n## Merge Candidate (Trimmed)\n\n"
        + &candidate_json
        + "\n\nAnalyze ONLY this candidate cluster. Decide if the pages represent true cannibalization (same search intent competing in SERPs) or just topical similarity.\n\n"
        + "If true cannibalization: recommend keep_id (the page id to keep), redirect_ids (page ids to redirect), and merge instructions.\n"
        + "If not: return no_action with a reason.\n\n"
        + "CRITICAL: Select pages by numeric id only (keep_id / redirect_ids). Never emit URLs — the app resolves ids to canonical URLs.\n"
        + "CRITICAL: Return ONLY a single JSON object matching the Output Contract. Do not include markdown prose outside the JSON.";
    let bytes = prompt.len();
    (prompt, bytes)
}
