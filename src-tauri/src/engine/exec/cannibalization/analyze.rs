//! Step 6: Agentic candidate analysis.

use super::*;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

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
/// Reads `cannibalization_candidates.json`, calls the agent once per candidate,
/// and writes `cannibalization_batch_outputs.json`.
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
                crate::db::content_audit::get_latest_audit_artifact(&conn, &task.project_id, "cannibalization_candidates").ok().flatten()
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
        };
    }

    const TARGET_PROMPT_BYTES: usize = 15_000;
    const HARD_PROMPT_BYTES: usize = 20_000;

    let skill = match crate::engine::skills::load_skill_or_fail(repo_root, "cannibalization-strategy") {
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

    let mut batch_outputs: Vec<serde_json::Value> = Vec::new();
    let mut failed_candidates: Vec<String> = Vec::new();
    let mut guard_degraded_count: usize = 0;

    for candidate in &candidates {
        let candidate_id = candidate["candidate_id"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        let (prompt, prompt_bytes) = build_merge_prompt(&skill.content, &candidate);

        let chosen_prompt = if prompt_bytes > HARD_PROMPT_BYTES {
            let (trimmed, trimmed_bytes) = build_merge_prompt_trimmed(&skill.content, &candidate);
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
                    rec.cluster_theme = candidate["theme"].as_str().unwrap_or("").to_string();
                }
                if rec.confidence.is_empty() {
                    rec.confidence = "medium".to_string();
                }
                // ── Deterministic URL resolution ──────────────────────────────
                // The agent selects pages by stable `id`; we resolve those ids
                // to canonical `/blog/<slug>` URLs here. The agent never owns URL
                // strings, so it cannot introduce malformed (e.g. underscored)
                // slugs into the merge plan. Any id that does not resolve to a
                // page in the candidate set is treated as no_action with a reason.
                let page_url_by_id: std::collections::HashMap<i64, String> = candidate["pages"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|p| {
                        let id = p["id"].as_i64()?;
                        let url = crate::content::slug::format_blog_link(
                            p["url"].as_str().unwrap_or(p["url_slug"].as_str().unwrap_or("")),
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
            "[cannibalization_audit] {} recommendation(s) degraded to no_action by the id-resolution guard (model returned keep_id/redirect_ids not in the candidate page set)",
            guard_degraded_count
        );
    }

    StepResult {
        success: failed_candidates.is_empty() || success_count > 0,
        message: format!(
            "Analyzed {}/{} candidates successfully. Failed: {}. Degraded to no_action by id-resolution guard: {}",
            success_count,
            candidates_len,
            failed_candidates.len(),
            guard_degraded_count
        ),
        output: Some(serde_json::to_string_pretty(&batch_doc).unwrap_or_default()),
    }
}

/// Build the full merge-analysis prompt for a single candidate.
pub(crate) fn build_merge_prompt(skill_content: &str, candidate: &serde_json::Value) -> (String, usize) {
    let candidate_json = serde_json::to_string_pretty(candidate).unwrap_or_default();
    let prompt = skill_content.to_string()
        + "\n\n---\n\n## Merge Candidate\n\n"
        + &candidate_json
        + "\n\nAnalyze ONLY this candidate cluster. Decide if the pages represent true cannibalization (same search intent competing in SERPs) or just topical similarity.\n\n"
        + "If true cannibalization: recommend a keeper URL, redirect URLs, and merge instructions.\n"
        + "If not: return no_action with a reason.\n\n"
        + "CRITICAL: Return ONLY a single JSON object matching the Output Contract. Do not include markdown prose outside the JSON.";
    let bytes = prompt.len();
    (prompt, bytes)
}

/// Build a trimmed prompt without page excerpts (second-level budget fallback).
pub(crate) fn build_merge_prompt_trimmed(
    skill_content: &str,
    candidate: &serde_json::Value,
) -> (String, usize) {
    let mut trimmed = candidate.clone();
    if let Some(pages) = trimmed["pages"].as_array_mut() {
        for page in pages {
            if let serde_json::Value::Object(ref mut map) = page {
                map.remove("excerpt");
            }
        }
    }
    let candidate_json = serde_json::to_string_pretty(&trimmed).unwrap_or_default();
    let prompt = skill_content.to_string()
        + "\n\n---\n\n## Merge Candidate (Trimmed)\n\n"
        + &candidate_json
        + "\n\nAnalyze ONLY this candidate cluster. Decide if the pages represent true cannibalization (same search intent competing in SERPs) or just topical similarity.\n\n"
        + "If true cannibalization: recommend a keeper URL, redirect URLs, and merge instructions.\n"
        + "If not: return no_action with a reason.\n\n"
        + "CRITICAL: Return ONLY a single JSON object matching the Output Contract. Do not include markdown prose outside the JSON.";
    let bytes = prompt.len();
    (prompt, bytes)
}
