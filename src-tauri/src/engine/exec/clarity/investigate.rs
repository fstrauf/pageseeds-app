use crate::clarity::{
    client::clarity_dashboard_url,
    db,
    export,
    models::{
        ClarityFinding, ClarityInvestigationResult, ClarityPageScore, ClaritySummary,
        ClaritySummaryMeta,
    },
};
use crate::engine::project_paths::ProjectPaths;
use crate::engine::task_store;
use crate::engine::workflows::{StepResult, WorkflowStep};
use crate::models::task::Task;
use rusqlite::Connection;
use std::collections::HashMap;

const DAYS_ANALYZED: i64 = 7;
const TOP_PAGES_LIMIT: usize = 20;

/// Parse a numeric value from the Clarity value map.
fn parse_value(values: &HashMap<String, serde_json::Value>, key: &str) -> f64 {
    values
        .get(key)
        .and_then(|v| {
            if let Some(n) = v.as_f64() {
                Some(n)
            } else if let Some(s) = v.as_str() {
                s.parse::<f64>().ok()
            } else {
                None
            }
        })
        .unwrap_or(0.0)
}

/// Parse a dimension value as string.
fn dimension_value(dimensions: &HashMap<String, serde_json::Value>, key: &str) -> Option<String> {
    dimensions.get(key).and_then(|v| {
        if let Some(s) = v.as_str() {
            Some(s.to_string())
        } else {
            serde_json::to_string(v).ok()
        }
    })
}

/// Extract the page URL from a Clarity row.
/// The Clarity Export API returns the URL in values as "Url" rather than in dimensions as "URL".
fn row_url(row: &crate::clarity::models::ClarityExportRow) -> Option<String> {
    dimension_value(&row.dimensions, "URL")
        .or_else(|| row.values.get("Url").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .filter(|u| !u.is_empty() && u != "null")
}

/// Deterministic aggregation of Clarity rows into per-page scores.
pub fn exec_clarity_summarise(task: &Task, project_path: &str, conn: &Connection) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Resolve Clarity project ID from the project record.
    let project = match task_store::get_project(conn, &task.project_id) {
        Ok(p) => p,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to load project '{}': {}", task.project_id, e),
                output: None,
            }
        }
    };
    let project_id = match project.clarity_project_id.as_deref().filter(|id| !id.is_empty()) {
        Some(id) => id.to_string(),
        None => {
            return StepResult {
                success: false,
                message: "clarity_project_id not set in project settings".to_string(),
                output: None,
            }
        }
    };

    let end_date = chrono::Utc::now().date_naive().to_string();
    let start_date = (chrono::Utc::now().date_naive() - chrono::Days::new(DAYS_ANALYZED as u64))
        .to_string();

    let rows = match db::list_rows(conn, &task.project_id, &start_date, &end_date) {
        Ok(r) => r,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to load Clarity rows: {}", e),
                output: None,
            }
        }
    };

    if rows.is_empty() {
        return StepResult {
            success: false,
            message: "No Clarity export data found. Run collect_clarity first.".to_string(),
            output: None,
        };
    }

    // Aggregate per URL.
    let mut by_url: HashMap<String, PageAccumulator> = HashMap::new();
    for row in rows {
        let url = match row_url(&row) {
            Some(u) => u,
            None => continue,
        };

        let acc = by_url.entry(url).or_default();
        let metric = crate::clarity::models::ClarityMetric::from_api_name(&row.metric_name);

        match metric {
            crate::clarity::models::ClarityMetric::Traffic => {
                acc.total_sessions += parse_value(&row.values, "totalSessionCount");
            }
            crate::clarity::models::ClarityMetric::RageClickCount => {
                acc.rage_click_count += parse_value(&row.values, "subTotal");
            }
            crate::clarity::models::ClarityMetric::DeadClickCount => {
                acc.dead_click_count += parse_value(&row.values, "subTotal");
            }
            crate::clarity::models::ClarityMetric::QuickbackClick => {
                acc.quickback_count += parse_value(&row.values, "subTotal");
            }
            crate::clarity::models::ClarityMetric::ExcessiveScroll => {
                acc.excessive_scroll_count += parse_value(&row.values, "subTotal");
            }
            crate::clarity::models::ClarityMetric::ErrorClickCount => {
                acc.error_click_count += parse_value(&row.values, "subTotal");
            }
            crate::clarity::models::ClarityMetric::ScriptErrorCount => {
                acc.script_error_count += parse_value(&row.values, "subTotal");
            }
            crate::clarity::models::ClarityMetric::EngagementTime => {
                let sessions = parse_value(&row.values, "sessionsCount").max(1.0);
                let total_seconds = parse_value(&row.values, "totalTime");
                // Average across dimension sets; simple weighted average by sessions.
                acc.engagement_time_sum += total_seconds;
                acc.engagement_time_sessions += sessions;
            }
            crate::clarity::models::ClarityMetric::ScrollDepth => {
                let sessions = parse_value(&row.values, "sessionsCount").max(1.0);
                let depth = parse_value(&row.values, "averageScrollDepth");
                acc.scroll_depth_sum += depth * sessions;
                acc.scroll_depth_sessions += sessions;
            }
            _ => {}
        }
    }

    // Compute rates and z-scores.
    let mut page_scores: Vec<ClarityPageScore> = by_url
        .into_iter()
        .map(|(url, acc)| {
            let sessions = acc.total_sessions.max(1.0);
            let avg_engagement = if acc.engagement_time_sessions > 0.0 {
                acc.engagement_time_sum / acc.engagement_time_sessions
            } else {
                0.0
            };
            let avg_scroll = if acc.scroll_depth_sessions > 0.0 {
                acc.scroll_depth_sum / acc.scroll_depth_sessions
            } else {
                0.0
            };
            ClarityPageScore {
                url: url.clone(),
                total_sessions: acc.total_sessions,
                rage_click_count: acc.rage_click_count,
                dead_click_count: acc.dead_click_count,
                quickback_count: acc.quickback_count,
                excessive_scroll_count: acc.excessive_scroll_count,
                error_click_count: acc.error_click_count,
                script_error_count: acc.script_error_count,
                avg_engagement_seconds: avg_engagement,
                avg_scroll_depth: avg_scroll,
                rage_click_rate: acc.rage_click_count / sessions,
                dead_click_rate: acc.dead_click_count / sessions,
                quickback_rate: acc.quickback_count / sessions,
                z_score: 0.0,
                clarity_dashboard_url: clarity_dashboard_url(&project_id, &url),
            }
        })
        .filter(|p| p.total_sessions >= 10.0)
        .collect();

    // Compute a simple composite z-score for ranking.
    let mean_rate = |extractor: fn(&ClarityPageScore) -> f64| {
        let sum: f64 = page_scores.iter().map(extractor).sum();
        let count = page_scores.len().max(1) as f64;
        sum / count
    };
    let std_rate = |extractor: fn(&ClarityPageScore) -> f64, mean: f64| {
        let variance: f64 = page_scores
            .iter()
            .map(|p| {
                let diff = extractor(p) - mean;
                diff * diff
            })
            .sum::<f64>()
            / page_scores.len().max(1) as f64;
        variance.sqrt().max(0.0001)
    };

    let rage_mean = mean_rate(|p| p.rage_click_rate);
    let rage_std = std_rate(|p| p.rage_click_rate, rage_mean);
    let dead_mean = mean_rate(|p| p.dead_click_rate);
    let dead_std = std_rate(|p| p.dead_click_rate, dead_mean);
    let quick_mean = mean_rate(|p| p.quickback_rate);
    let quick_std = std_rate(|p| p.quickback_rate, quick_mean);

    for p in &mut page_scores {
        let rage_z = (p.rage_click_rate - rage_mean) / rage_std;
        let dead_z = (p.dead_click_rate - dead_mean) / dead_std;
        let quick_z = (p.quickback_rate - quick_mean) / quick_std;
        // Weight quickbacks highest because they strongly signal landing-page mismatch.
        p.z_score = rage_z * 0.3 + dead_z * 0.3 + quick_z * 0.4;
    }

    page_scores.sort_by(|a, b| b.z_score.partial_cmp(&a.z_score).unwrap_or(std::cmp::Ordering::Equal));
    page_scores.truncate(TOP_PAGES_LIMIT);

    let summary = ClaritySummary {
        meta: ClaritySummaryMeta {
            project_id: project_id.clone(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            days_analyzed: DAYS_ANALYZED,
        },
        page_scores,
        top_findings: Vec::new(),
    };

    if let Err(e) = export::write_summary(&paths.automation_dir, &summary) {
        return StepResult {
            success: false,
            message: format!("Failed to write clarity_summary.json: {}", e),
            output: None,
        };
    }

    StepResult {
        success: true,
        message: format!("Summarised {} pages into clarity_summary.json", summary.page_scores.len()),
        output: Some(serde_json::to_string(&summary.meta).unwrap_or_default()),
    }
}

/// Agentic interpretation of the Clarity summary.
pub fn exec_clarity_investigate(
    step: &WorkflowStep,
    _task: &Task,
    project_path: &str,
    provider: &str,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let summary = match export::read_summary(&paths.automation_dir) {
        Ok(Some(s)) => s,
        Ok(None) => {
            return StepResult {
                success: false,
                message: "clarity_summary.json not found. Run clarity_summarise first.".to_string(),
                output: None,
            }
        }
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to read clarity_summary.json: {}", e),
                output: None,
            }
        }
    };

    let context = serde_json::to_string(&summary).unwrap_or_default();

    let result = match crate::engine::agent::run_agent_with_skill(
        "clarity-investigate",
        std::path::Path::new(project_path),
        &context,
        provider,
        step.params.get("skill").map(|s| s.as_str()),
    ) {
        Ok(output) => output,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Clarity investigation agent failed: {}", e),
                output: None,
            }
        }
    };

    // Try structured extraction first; fall back to parsing the raw JSON output.
    let findings: Vec<ClarityFinding> =
        match serde_json::from_str::<ClarityInvestigationResult>(&result) {
            Ok(r) => r.findings,
            Err(_) => match serde_json::from_str::<Vec<ClarityFinding>>(&result) {
                Ok(f) => f,
                Err(e) => {
                    log::warn!(
                        "[clarity_investigate] failed to parse agent output as structured JSON: {}. Raw length={}",
                        e,
                        result.len()
                    );
                    Vec::new()
                }
            },
        };

    let mut summary = summary;
    summary.top_findings = findings;

    if let Err(e) = export::write_summary(&paths.automation_dir, &summary) {
        return StepResult {
            success: false,
            message: format!("Failed to update clarity_summary.json: {}", e),
            output: None,
        };
    }

    StepResult {
        success: true,
        message: format!(
            "Produced {} Clarity findings; review in TaskDetail",
            summary.top_findings.len()
        ),
        output: Some(serde_json::to_string(&summary.top_findings).unwrap_or_default()),
    }
}

#[derive(Default)]
struct PageAccumulator {
    total_sessions: f64,
    rage_click_count: f64,
    dead_click_count: f64,
    quickback_count: f64,
    excessive_scroll_count: f64,
    error_click_count: f64,
    script_error_count: f64,
    engagement_time_sum: f64,
    engagement_time_sessions: f64,
    scroll_depth_sum: f64,
    scroll_depth_sessions: f64,
}
