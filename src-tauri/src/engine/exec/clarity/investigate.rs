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
/// Only the plain `url` dimension set carries de-duplicated per-page counts.
/// The `url+device` / `url+source` sets split the same sessions across rows,
/// so summing them together with `url` multiplies every count.
const URL_DIMENSION_SET: &str = "url";

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

    // Each collection run stamps every row with the same snapshot date and
    // stores three URL-carrying dimension sets. Restricting aggregation to the
    // latest snapshot's `url` set keeps per-page counts at 1x; aggregating the
    // whole window would multiply them by snapshots × sets (~21x steady state).
    let snapshot_date = match db::latest_snapshot_date(
        conn,
        &task.project_id,
        URL_DIMENSION_SET,
        &start_date,
        &end_date,
    ) {
        Ok(Some(d)) => d,
        Ok(None) => {
            return StepResult {
                success: false,
                message: "No Clarity export data found. Run collect_clarity first.".to_string(),
                output: None,
            }
        }
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to load Clarity rows: {}", e),
                output: None,
            }
        }
    };

    let rows = match db::list_rows(
        conn,
        &task.project_id,
        &start_date,
        &end_date,
        Some(URL_DIMENSION_SET),
        Some(&snapshot_date),
    ) {
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

    let gsc_context = load_gsc_context(conn, &task.project_id);
    let page_scores = aggregate_page_scores(&rows, &gsc_context, &project_id);

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

    // Parse the agent output via the shared extractor: fenced/prose-wrapped
    // JSON is recovered; malformed or empty output fails the step loudly
    // instead of reporting success with 0 findings.
    let findings = match parse_findings(&result) {
        Ok(f) => f,
        Err(e) => {
            log::warn!(
                "[clarity_investigate] {}. Raw length={}",
                e,
                result.len()
            );
            return StepResult {
                success: false,
                message: format!("Failed to parse Clarity investigation output: {}", e),
                output: None,
            };
        }
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

/// GSC context attached to a page score as LLM-facing context only. It never
/// influences the behavioral z-score ranking.
#[derive(Debug, Clone, Copy)]
struct GscContext {
    clicks: f64,
    impressions: f64,
    position: f64,
}

/// Load per-slug GSC context from the article sidecar metadata written by
/// `collect_gsc` (namespace `gsc` in `article_metadata`).
///
/// Best-effort: any failure yields an empty map so a missing or failed GSC
/// sync never breaks the Clarity pipeline — the GSC fields simply stay absent.
fn load_gsc_context(conn: &Connection, project_id: &str) -> HashMap<String, GscContext> {
    let mut map = HashMap::new();

    let articles = match crate::content::article_index::list_articles(conn, project_id) {
        Ok(a) => a,
        Err(e) => {
            log::warn!("[clarity_summarise] failed to load articles for GSC join: {}", e);
            return map;
        }
    };
    let metadata = match crate::db::list_project_metadata(conn, project_id) {
        Ok(m) => m,
        Err(e) => {
            log::warn!("[clarity_summarise] failed to load article metadata for GSC join: {}", e);
            return map;
        }
    };

    let gsc_by_article: HashMap<i64, serde_json::Value> = metadata
        .into_iter()
        .filter(|(_, namespace, _)| namespace == "gsc")
        .filter_map(|(article_id, _, payload)| {
            serde_json::from_str(&payload)
                .ok()
                .map(|v| (article_id, v))
        })
        .collect();

    for article in &articles {
        if article.url_slug.is_empty() {
            continue;
        }
        let Some(gsc) = gsc_by_article.get(&article.id) else {
            continue;
        };
        let slug = crate::content::slug::normalize_url_slug(&article.url_slug);
        if slug.is_empty() {
            continue;
        }
        map.entry(slug).or_insert(GscContext {
            clicks: gsc.get("clicks").and_then(|v| v.as_f64()).unwrap_or(0.0),
            impressions: gsc
                .get("impressions")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
            position: gsc
                .get("avg_position")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0),
        });
    }

    map
}

/// Aggregate already-filtered Clarity rows into per-page scores, ranked by a
/// composite behavioral z-score and truncated to the top pages.
///
/// Rows must be pre-filtered to a single snapshot's `url` dimension set —
/// this function sums everything it is given.
fn aggregate_page_scores(
    rows: &[crate::clarity::models::ClarityExportRow],
    gsc_context: &HashMap<String, GscContext>,
    clarity_project_id: &str,
) -> Vec<ClarityPageScore> {
    // Aggregate per URL.
    let mut by_url: HashMap<String, PageAccumulator> = HashMap::new();
    for row in rows {
        let url = match row_url(row) {
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

    // Compute rates and attach GSC context (matched on the normalized URL slug).
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
            let gsc = gsc_context
                .get(&crate::content::slug::normalize_url_slug(&url))
                .copied();
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
                clarity_dashboard_url: clarity_dashboard_url(clarity_project_id, &url),
                gsc_clicks: gsc.map(|g| g.clicks),
                gsc_impressions: gsc.map(|g| g.impressions),
                gsc_position: gsc.map(|g| g.position),
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
    page_scores
}

/// Parse agent output into findings.
///
/// Uses the shared `engine::text::extract_json` helper so fenced or
/// prose-wrapped JSON is recovered. Empty output or output that does not
/// match the findings schema is a hard error — the step must not report
/// success with 0 findings when the model output was unusable.
fn parse_findings(output: &str) -> Result<Vec<ClarityFinding>, String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return Err("agent returned empty output".to_string());
    }
    let value = crate::engine::text::extract_json(trimmed)
        .ok_or_else(|| "no JSON payload found in agent output".to_string())?;
    if let Ok(result) = serde_json::from_value::<ClarityInvestigationResult>(value.clone()) {
        return Ok(result.findings);
    }
    serde_json::from_value::<Vec<ClarityFinding>>(value)
        .map_err(|e| format!("output did not match the findings schema: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clarity::models::ClarityExportRow;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn setup_project(conn: &Connection, project_id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES (?1, ?2, ?3, 1, 'workspace')",
            [project_id, "Test Project", "/tmp/clarity-test"],
        )
        .unwrap();
    }

    fn traffic_row(date: &str, dimension_set: &str, url: &str, sessions: f64) -> ClarityExportRow {
        let mut dimensions = HashMap::new();
        dimensions.insert("URL".to_string(), serde_json::json!(url));
        let mut values = HashMap::new();
        values.insert("totalSessionCount".to_string(), serde_json::json!(sessions));
        ClarityExportRow {
            clarity_date: date.to_string(),
            dimension_set: dimension_set.to_string(),
            metric_name: "Traffic".to_string(),
            dimensions,
            values,
        }
    }

    /// 7 daily snapshots × 3 URL-carrying dimension sets for one URL. `url`
    /// rows carry `sessions`; the device/source rows split the same sessions.
    fn seed_multi_snapshot_rows(
        conn: &Connection,
        project_id: &str,
        url: &str,
        sessions: f64,
    ) {
        let dates = [
            "2026-07-14",
            "2026-07-15",
            "2026-07-16",
            "2026-07-17",
            "2026-07-18",
            "2026-07-19",
            "2026-07-20",
        ];
        let mut rows = Vec::new();
        for date in dates {
            rows.push(traffic_row(date, "url", url, sessions));
            rows.push(traffic_row(date, "url+device", url, sessions * 0.6));
            rows.push(traffic_row(date, "url+device", url, sessions * 0.4));
            rows.push(traffic_row(date, "url+source", url, sessions * 0.7));
            rows.push(traffic_row(date, "url+source", url, sessions * 0.3));
        }
        db::insert_rows(conn, project_id, "2026-07-20T00:00:00Z", &rows).unwrap();
    }

    /// Replicates the query path in `exec_clarity_summarise`.
    fn latest_url_rows(conn: &Connection, project_id: &str) -> Vec<ClarityExportRow> {
        let snapshot = db::latest_snapshot_date(
            conn,
            project_id,
            URL_DIMENSION_SET,
            "2026-07-13",
            "2026-07-20",
        )
        .unwrap()
        .unwrap();
        db::list_rows(
            conn,
            project_id,
            "2026-07-13",
            "2026-07-20",
            Some(URL_DIMENSION_SET),
            Some(&snapshot),
        )
        .unwrap()
    }

    #[test]
    fn aggregation_uses_latest_snapshot_url_set_only() {
        let conn = in_memory_db();
        setup_project(&conn, "p1");
        seed_multi_snapshot_rows(&conn, "p1", "https://example.com/pricing", 100.0);
        seed_multi_snapshot_rows(&conn, "p1", "https://example.com/features", 50.0);

        let rows = latest_url_rows(&conn, "p1");
        let scores = aggregate_page_scores(&rows, &HashMap::new(), "clarity-proj");

        let pricing = scores
            .iter()
            .find(|p| p.url == "https://example.com/pricing")
            .expect("pricing page present");
        // 1x the real count — not 7 snapshots × 3 sets ≈ 21x.
        assert_eq!(pricing.total_sessions, 100.0);
        let features = scores
            .iter()
            .find(|p| p.url == "https://example.com/features")
            .expect("features page present");
        assert_eq!(features.total_sessions, 50.0);
    }

    #[test]
    fn noise_floor_uses_real_session_counts() {
        let conn = in_memory_db();
        setup_project(&conn, "p1");
        // 5 real sessions per snapshot: 5 × 21 = 105 unfiltered, which would
        // pass the >= 10 floor if counts were inflated.
        seed_multi_snapshot_rows(&conn, "p1", "https://example.com/noise", 5.0);
        seed_multi_snapshot_rows(&conn, "p1", "https://example.com/signal", 15.0);

        let rows = latest_url_rows(&conn, "p1");
        let scores = aggregate_page_scores(&rows, &HashMap::new(), "clarity-proj");

        assert!(
            scores.iter().all(|p| p.url != "https://example.com/noise"),
            "page with <10 real sessions must not pass the noise floor"
        );
        assert!(
            scores.iter().any(|p| p.url == "https://example.com/signal"),
            "page with >=10 real sessions must be kept"
        );
    }

    #[test]
    fn gsc_context_joins_on_normalized_slug() {
        let conn = in_memory_db();
        setup_project(&conn, "p1");
        seed_multi_snapshot_rows(&conn, "p1", "https://example.com/pricing", 100.0);
        seed_multi_snapshot_rows(&conn, "p1", "https://example.com/unknown", 50.0);

        let mut gsc = HashMap::new();
        gsc.insert(
            "pricing".to_string(),
            GscContext {
                clicks: 42.0,
                impressions: 1200.0,
                position: 8.5,
            },
        );

        let rows = latest_url_rows(&conn, "p1");
        let scores = aggregate_page_scores(&rows, &gsc, "clarity-proj");

        let pricing = scores
            .iter()
            .find(|p| p.url == "https://example.com/pricing")
            .unwrap();
        assert_eq!(pricing.gsc_clicks, Some(42.0));
        assert_eq!(pricing.gsc_impressions, Some(1200.0));
        assert_eq!(pricing.gsc_position, Some(8.5));

        let unknown = scores
            .iter()
            .find(|p| p.url == "https://example.com/unknown")
            .unwrap();
        assert_eq!(unknown.gsc_clicks, None);
        assert_eq!(unknown.gsc_impressions, None);
        assert_eq!(unknown.gsc_position, None);
    }

    #[test]
    fn parse_findings_recovers_fenced_and_prose_wrapped_json() {
        let fenced = "```json\n{\"findings\":[{\"issue_type\":\"Rage clicks\",\"severity\":\"high\",\"url\":\"/pricing\",\"evidence\":\"rate 2.1%\",\"recommendation\":\"inspect CTA\",\"clarity_dashboard_url\":\"https://clarity.example\"}]}\n```";
        let findings = parse_findings(fenced).unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].url, "/pricing");

        let prose = "Here are the results:\n{\"findings\": []}\nHope that helps.";
        let findings = parse_findings(prose).unwrap();
        assert!(findings.is_empty());
    }

    #[test]
    fn parse_findings_accepts_bare_array_shape() {
        let output = "[{\"issue_type\":\"Dead clicks\",\"severity\":\"low\",\"url\":\"/x\",\"evidence\":\"e\",\"recommendation\":\"r\",\"clarity_dashboard_url\":\"u\"}]";
        let findings = parse_findings(output).unwrap();
        assert_eq!(findings.len(), 1);
    }

    #[test]
    fn parse_findings_fails_loudly_on_malformed_output() {
        assert!(parse_findings("The model could not produce JSON today.").is_err());
        assert!(parse_findings("{\"unexpected\": true}").is_err());
    }

    #[test]
    fn parse_findings_fails_loudly_on_empty_output() {
        assert!(parse_findings("").is_err());
        assert!(parse_findings("   \n  ").is_err());
    }
}
