use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::indexing_health::{
    DistinctivenessVerdict, IndexingCampaignPlan, IndexingCampaignSummary, IndexingTargetContext,
    IndexingTargetPlan, PrerequisiteCheck, PrerequisiteReport, TargetDiagnosis,
};
use crate::models::task::Task;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 4: Reduce Plan
// ═══════════════════════════════════════════════════════════════════════════════

/// Read all previous step outputs and produce the final campaign plan.
pub(crate) fn exec_ihc_reduce_plan(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Load target contexts
    let contexts_path = paths.automation_dir.join("indexing_target_contexts.json");
    let contexts_doc: serde_json::Value = match std::fs::read_to_string(&contexts_path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| serde_json::json!({ "targets": [] })),
        Err(_) => serde_json::json!({ "targets": [] }),
    };

    let target_contexts: Vec<IndexingTargetContext> = contexts_doc["targets"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    // Load distinctiveness verdicts
    let verdicts_path = paths
        .automation_dir
        .join("indexing_distinctiveness_verdicts.json");
    let verdicts: HashMap<String, DistinctivenessVerdict> = std::fs::read_to_string(&verdicts_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v["verdicts"].as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| {
            let verdict: DistinctivenessVerdict = serde_json::from_value(v).ok()?;
            Some((verdict.target_url.clone(), verdict))
        })
        .collect();

    // Load exact keyword duplicates for flagging
    let dupes_path = paths.automation_dir.join("exact_keyword_duplicates.json");
    let dupes_doc: serde_json::Value = std::fs::read_to_string(&dupes_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({ "duplicates": [] }));
    let dupe_keywords: Vec<String> = dupes_doc["duplicates"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|d| d["keyword"].as_str().map(String::from))
        .collect();

    let mut plans: Vec<IndexingTargetPlan> = Vec::new();
    let mut summary = IndexingCampaignSummary {
        total_targets: target_contexts.len(),
        fix_content: 0,
        add_links: 0,
        merge: 0,
        rewrite_title_h1: 0,
        fix_indexing: 0,
        no_action: 0,
    };

    for ctx in &target_contexts {
        let verdict = verdicts.get(&ctx.target.url);
        let action = determine_action(ctx, verdict, &dupe_keywords);

        match action.as_str() {
            "fix_content" => summary.fix_content += 1,
            "add_links" => summary.add_links += 1,
            "merge" => summary.merge += 1,
            "rewrite_title_h1" => summary.rewrite_title_h1 += 1,
            "fix_indexing" => summary.fix_indexing += 1,
            _ => summary.no_action += 1,
        }

        plans.push(IndexingTargetPlan {
            url: ctx.target.url.clone(),
            reason_code: ctx.target.reason_code.clone(),
            recommended_action: action,
            context_artifact_key: Some(format!(
                "ihc_target_context_{}",
                slugify_url(&ctx.target.url)
            )),
            distinctiveness_verdict: verdict.cloned(),
            content_audit_summary: None,
            word_count: Some(ctx.target.word_count),
            incoming_links: Some(ctx.target.incoming_links),
            file: Some(ctx.target.file.clone()).filter(|f| !f.is_empty()),
        });
    }

    // Capture summary values before moving summary into plan
    let summary_msg = format!(
        "Campaign plan: {} fix_content, {} add_links, {} merge, {} rewrite_title_h1, {} fix_indexing, {} no_action",
        summary.fix_content, summary.add_links, summary.merge, summary.rewrite_title_h1, summary.fix_indexing, summary.no_action
    );

    let plan = IndexingCampaignPlan {
        generated_at: chrono::Utc::now().to_rfc3339(),
        targets: plans,
        summary,
    };

    let plan_json = match serde_json::to_string_pretty(&plan) {
        Ok(j) => j,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize campaign plan: {}", e),
                output: None,
            }
        }
    };

    // Save to database (new primary storage)
    let now_iso = chrono::Utc::now().to_rfc3339();
    if let Ok(db) = rusqlite::Connection::open(crate::db::default_db_path()) {
        let _ = crate::db::content_audit::save_audit_artifact(
            &db,
            &task.project_id,
            "indexing_campaign_plan",
            &now_iso,
            &plan_json,
        );
    }

    StepResult {
        success: true,
        message: summary_msg,
        output: Some(plan_json),
    }
}

pub(crate) fn determine_action(
    ctx: &IndexingTargetContext,
    verdict: Option<&DistinctivenessVerdict>,
    _dupe_keywords: &[String],
) -> String {
    // Priority order from spec
    if ctx.target.content_audit_health == "poor" {
        return "fix_content".to_string();
    }

    if ctx.target.incoming_links == 0 {
        return "add_links".to_string();
    }

    if let Some(v) = verdict {
        if v.verdict == "OVERLAP" && v.confidence == "high" {
            return "merge".to_string();
        }
        if v.verdict == "OVERLAP" && (v.confidence == "medium" || v.confidence == "low") {
            return "rewrite_title_h1".to_string();
        }
    }

    if ctx.target.reason_code == "not_indexed_crawled"
        && ctx.diagnosis.is_long
        && ctx.diagnosis.has_links
    {
        return "no_action".to_string();
    }

    "fix_indexing".to_string()
}

pub(crate) fn slugify_url(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .replace('/', "_")
        .replace('.', "_")
        .replace(':', "_")
        .to_lowercase()
}
