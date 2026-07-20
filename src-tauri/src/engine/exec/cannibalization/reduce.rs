//! Step 7: Reduce batch outputs into final strategy.

use super::*;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

// ═══════════════════════════════════════════════════════════════════════════════
// Step 4: Reduce Strategy
// ═══════════════════════════════════════════════════════════════════════════════

/// Deterministic reducer that merges batch outputs into the final
/// `cannibalization_strategy.json`.
///
/// Validates merge recommendations and includes deterministic hub data.
pub(crate) fn exec_can_reduce_strategy(_task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let batch_path = paths
        .automation_dir
        .join("cannibalization_batch_outputs.json");
    let batch_doc: serde_json::Value = match crate::engine::exec::common::read_json(
        &batch_path,
        "cannibalization_batch_outputs.json",
    ) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let hub_gaps_path = paths.automation_dir.join("hub_gaps.json");
    let hub_gaps_doc: serde_json::Value = std::fs::read_to_string(&hub_gaps_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "hub_gaps": [] }));

    let mut merge_recommendations: Vec<serde_json::Value> = Vec::new();
    let mut risks: Vec<String> = Vec::new();
    let mut guard_degraded_count: usize = 0;

    if let Some(outputs) = batch_doc["batch_outputs"].as_array() {
        for output in outputs {
            if !output["success"].as_bool().unwrap_or(false) {
                if let Some(cid) = output["candidate_id"].as_str() {
                    risks.push(format!(
                        "Candidate {} failed: {}",
                        cid,
                        output["message"].as_str().unwrap_or("unknown error")
                    ));
                }
                continue;
            }

            if let Some(rec) = output["merge_recommendation"].as_object() {
                if rec
                    .get("no_action")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    // Distinguish guard-degraded recommendations (the analyze
                    // step's id-resolution guard rewrote them to no_action)
                    // from genuine model no_action decisions.
                    let reason = rec.get("reason").and_then(|v| v.as_str()).unwrap_or("");
                    if reason.contains("keep_id")
                        || reason.contains("redirect_ids")
                        || reason.contains("candidate page set")
                    {
                        guard_degraded_count += 1;
                    }
                    continue;
                }

                // Deterministic slug normalization: merge URLs must be canonical
                // `/blog/<hyphenated-slug>` paths. The analyze step resolves them
                // from agent-selected ids, but we normalize defensively here too so
                // hand-edited or legacy batch outputs can never emit a non-resolvable
                // (e.g. underscored) URL into the strategy artifact or downstream
                // 301 redirects.
                let keep_url_raw = rec.get("keep_url").and_then(|v| v.as_str()).unwrap_or("");
                let keep_url = crate::content::slug::format_blog_link(keep_url_raw);
                if keep_url_raw.trim().is_empty() || keep_url == "/blog/" {
                    risks.push(format!(
                        "Missing keep_url for candidate {}",
                        output["candidate_id"].as_str().unwrap_or("?")
                    ));
                    continue;
                }

                let redirect_urls: Vec<String> = rec
                    .get("redirect_urls")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(crate::content::slug::format_blog_link))
                            .collect()
                    })
                    .unwrap_or_default();

                if redirect_urls.is_empty() {
                    risks.push(format!(
                        "No redirect_urls for candidate {} (keeper: {})",
                        output["candidate_id"].as_str().unwrap_or("?"),
                        keep_url
                    ));
                }

                let mut rec = rec.clone();
                // Overwrite with canonical URLs so the artifact and downstream
                // consolidate_cluster executor never see raw agent strings.
                rec.insert(
                    "keep_url".to_string(),
                    serde_json::Value::String(keep_url.clone()),
                );
                rec.insert(
                    "redirect_urls".to_string(),
                    serde_json::Value::Array(
                        redirect_urls
                            .iter()
                            .cloned()
                            .map(serde_json::Value::String)
                            .collect(),
                    ),
                );
                if !rec.contains_key("confidence") {
                    rec.insert(
                        "confidence".to_string(),
                        serde_json::Value::String("medium".to_string()),
                    );
                }
                // Defensive fallback: ensure unique cluster_id from candidate_id.
                let has_valid_cluster_id = rec
                    .get("cluster_id")
                    .and_then(|v| v.as_str())
                    .map(|s| !s.is_empty())
                    .unwrap_or(false);
                if !has_valid_cluster_id {
                    rec.insert("cluster_id".to_string(), output["candidate_id"].clone());
                }

                merge_recommendations.push(serde_json::Value::Object(rec));
            }
        }
    }

    // Deduplicate merge recommendations by cluster_id. The agentic step can
    // return the same cluster_id for multiple candidates; without dedup the
    // frontend renders duplicate React keys and the approval/task-creation
    // flow treats them as a single recommendation, causing UI bugs.
    {
        let mut seen = std::collections::HashSet::new();
        merge_recommendations.retain(|rec| {
            let id = rec.get("cluster_id").and_then(|v| v.as_str()).unwrap_or("");
            if id.is_empty() {
                return false;
            }
            seen.insert(id.to_string())
        });
    }

    if guard_degraded_count > 0 {
        log::warn!(
            "[cannibalization_audit] {} recommendation(s) discarded: degraded to no_action by the id-resolution guard (possible skill drift or model contract violation)",
            guard_degraded_count
        );
        risks.push(format!(
            "{} recommendation(s) discarded: model returned keep_id/redirect_ids not in the candidate page set (possible skill drift or model contract violation)",
            guard_degraded_count
        ));
    }

    let hub_recommendations: Vec<serde_json::Value> = hub_gaps_doc["hub_gaps"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|gap| {
            serde_json::json!({
                "topic": gap["theme"],
                "suggested_url": gap["suggested_url"],
                "suggested_title": gap["suggested_title"],
                "spoke_pages": gap["spoke_pages"].as_array().map(|arr| {
                    arr.iter().filter_map(|p| p["id"].as_i64()).collect::<Vec<i64>>()
                }).unwrap_or_default(),
                "outline_suggestion": "",
                "reason": gap["reason"],
                "deterministic": true,
            })
        })
        .collect();

    let strategy = serde_json::json!({
        "generated_at": chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "merge_recommendations": merge_recommendations,
        "hub_recommendations": hub_recommendations,
        "risks": risks,
    });

    let strategy_path = paths.automation_dir.join("cannibalization_strategy.json");
    // Delete any stale strategy file before writing the new one. This prevents
    // old duplicate recommendations from persisting if a previous audit run
    // produced a larger strategy and the current run produces fewer.
    let _ = std::fs::remove_file(&strategy_path);
    if let Err(e) = std::fs::write(
        &strategy_path,
        serde_json::to_string_pretty(&strategy).unwrap_or_default() + "\n",
    ) {
        log::warn!(
            "[cannibalization_audit] Failed to write strategy file: {}",
            e
        );
    }

    StepResult {
        success: true,
        message: format!(
            "Strategy reduced: {} merge recommendations, {} hub recommendations, {} risks, {} recommendation(s) discarded by id-resolution guard",
            merge_recommendations.len(),
            hub_recommendations.len(),
            risks.len(),
            guard_degraded_count
        ),
        output: Some(serde_json::to_string_pretty(&strategy).unwrap_or_default()),
    }
}
