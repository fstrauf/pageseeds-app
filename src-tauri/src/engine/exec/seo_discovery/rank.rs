use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::seo_discovery::{SeoOpportunitiesDoc, SeoOpportunity};
use crate::models::task::Task;

/// Signals collected for a single article from all available audit artifacts.
#[derive(Debug, Clone, Default)]
pub(crate) struct ArticleSignals {
    pub(crate) article_id: i64,
    pub(crate) url_slug: String,
    pub(crate) title: String,
    pub(crate) file: String,
    pub(crate) target_keyword: String,
    pub(crate) content_health: String,
    pub(crate) checks_failed: i64,
    pub(crate) health_score: i64,
    pub(crate) word_count: i64,
    pub(crate) internal_links: i64,
    pub(crate) impressions: f64,
    pub(crate) clicks: f64,
    pub(crate) ctr: f64,
    pub(crate) avg_position: f64,
    pub(crate) target_ctr: f64,
    pub(crate) clicks_lost: f64,
    pub(crate) ctr_opportunity: bool,
    pub(crate) indexing_status: String,
    pub(crate) cannibalized: bool,
    pub(crate) hub_gap: bool,
    pub(crate) ux_anomaly_z_score: f64,
    pub(crate) review_status: String,
    pub(crate) last_edited_at: String,
    pub(crate) last_reviewed_at: String,
}

/// Build a unified, ranked opportunity list from all available audit artifacts.
///
/// Reads content_audit, ctr_audit_context, cannibalization_clusters,
/// indexing_target_contexts, and clarity_summary; scores each article;
/// writes `seo_opportunities.json`; and persists opportunities to SQLite.
pub fn exec_rank_opportunities(
    task: &Task,
    project_path: &str,
    conn: &rusqlite::Connection,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let generated_at = chrono::Utc::now().to_rfc3339();

    let project_articles = crate::engine::exec::common::load_project_articles(&paths);
    let audit = crate::engine::exec::common::load_audit_snapshot(&task.project_id, &paths);

    let ctr_context = load_ctr_context(conn, &task.project_id, &paths);
    let cannibalization = load_cannibalization_clusters(conn, &task.project_id, &paths);
    let indexing = load_indexing_contexts(&paths);
    let clarity = load_clarity_summary(&paths);

    let mut opportunities: Vec<SeoOpportunity> = Vec::new();

    for article in &project_articles.articles {
        let signals = build_signals(
            article,
            &audit,
            &ctr_context,
            &cannibalization,
            &indexing,
            &clarity,
        );

        if should_skip(&signals) {
            continue;
        }

        let score = opportunity_score(&signals);
        if score <= 0 {
            continue;
        }

        let effort = classify_effort(&signals);
        let recommended_action = recommended_action(&signals, &effort);
        let primary_signal = primary_signal(&signals);

        opportunities.push(SeoOpportunity {
            article_id: signals.article_id,
            url_slug: signals.url_slug.clone(),
            title: signals.title.clone(),
            file: signals.file.clone(),
            target_keyword: signals.target_keyword.clone(),
            opportunity_score: score,
            effort: effort.to_string(),
            recommended_action: recommended_action.to_string(),
            primary_signal: primary_signal.to_string(),
            signals_json: signals_to_json(&signals),
        });
    }

    opportunities.sort_by(|a, b| b.opportunity_score.cmp(&a.opportunity_score));

    let doc = SeoOpportunitiesDoc {
        generated_at: generated_at.clone(),
        total_opportunities: opportunities.len(),
        opportunities,
    };

    // Write JSON artifact to disk
    let out_path = paths.automation_dir.join("seo_opportunities.json");
    let doc_json = match serde_json::to_string_pretty(&doc) {
        Ok(j) => j,
        Err(e) => {
            return StepResult::fail(format!("Failed to serialize opportunities: {}", e));
        }
    };
    if let Err(e) = std::fs::write(&out_path, &doc_json) {
        return StepResult::fail(format!(
            "Failed to write {}: {}",
            out_path.display(),
            e
        ));
    }

    // Persist to database
    if let Err(e) = crate::db::seo_discovery::save_opportunities(
        conn,
        &task.project_id,
        &generated_at,
        &doc.opportunities,
    ) {
        log::warn!("[seo_discovery] Failed to persist opportunities: {}", e);
    } else {
        let _ = crate::db::seo_discovery::decline_stale_opportunities(
            conn,
            &task.project_id,
            &generated_at,
        );
    }

    StepResult {
        success: true,
        message: format!(
            "Ranked {} SEO opportunities (top score: {})",
            doc.total_opportunities,
            doc.opportunities.first().map(|o| o.opportunity_score).unwrap_or(0)
        ),
        output: Some(doc_json),
        artifact_key: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Artifact loaders
// ═══════════════════════════════════════════════════════════════════════════════

fn load_ctr_context(
    conn: &rusqlite::Connection,
    project_id: &str,
    paths: &ProjectPaths,
) -> Option<serde_json::Value> {
    crate::db::content_audit::get_latest_audit_artifact(conn, project_id, "ctr_audit_context")
        .ok()
        .flatten()
        .or_else(|| {
            let path = paths.automation_dir.join("ctr_audit_context.json");
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        })
}

fn load_cannibalization_clusters(
    conn: &rusqlite::Connection,
    project_id: &str,
    paths: &ProjectPaths,
) -> Option<serde_json::Value> {
    crate::db::content_audit::get_latest_audit_artifact(conn, project_id, "cannibalization_clusters")
        .ok()
        .flatten()
        .or_else(|| {
            let path = paths.automation_dir.join("cannibalization_clusters.json");
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
        })
}

fn load_indexing_contexts(paths: &ProjectPaths) -> Option<serde_json::Value> {
    let path = paths.automation_dir.join("indexing_target_contexts.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn load_clarity_summary(paths: &ProjectPaths) -> Option<serde_json::Value> {
    let path = paths.automation_dir.join("clarity_summary.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Signal building
// ═══════════════════════════════════════════════════════════════════════════════

fn build_signals(
    article: &serde_json::Value,
    audit: &crate::engine::exec::common::AuditSnapshot,
    ctr_context: &Option<serde_json::Value>,
    cannibalization: &Option<serde_json::Value>,
    indexing: &Option<serde_json::Value>,
    clarity: &Option<serde_json::Value>,
) -> ArticleSignals {
    let article_id = article["id"].as_i64().unwrap_or(0);
    let url_slug = article["url_slug"].as_str().unwrap_or("").to_string();
    let file = article["file"].as_str().unwrap_or("").to_string();
    let title = article["title"].as_str().unwrap_or("").to_string();
    let target_keyword = article["target_keyword"].as_str().unwrap_or("").to_string();

    let audit_row = audit.by_slug.get(&url_slug).or_else(|| audit.by_file.get(&file));

    let content_health = audit_row
        .and_then(|a| a["health"].as_str())
        .unwrap_or("unknown")
        .to_string();
    let checks_failed = audit_row
        .and_then(|a| a["checks_failed"].as_i64())
        .unwrap_or(0);
    let health_score = audit_row
        .and_then(|a| a["health_score"].as_i64())
        .unwrap_or(0);
    let word_count = audit_row
        .and_then(|a| a["word_count"].as_i64())
        .unwrap_or(0);
    let internal_links = audit_row
        .and_then(|a| a["checks"]["internal_links"]["value"].as_i64())
        .unwrap_or(0);

    let mut signals = ArticleSignals {
        article_id,
        url_slug,
        title,
        file,
        target_keyword,
        content_health,
        checks_failed,
        health_score,
        word_count,
        internal_links,
        review_status: article["review_status"].as_str().unwrap_or("").to_string(),
        last_edited_at: article["last_edited_at"].as_str().unwrap_or("").to_string(),
        last_reviewed_at: article["last_reviewed_at"].as_str().unwrap_or("").to_string(),
        ..Default::default()
    };

    // CTR context
    if let Some(ctr) = ctr_context {
        if let Some(articles) = ctr["articles"].as_array() {
            if let Some(record) = articles.iter().find(|a| {
                a["id"].as_i64() == Some(article_id)
                    || a["url_slug"].as_str() == Some(&signals.url_slug)
            }) {
                signals.impressions = record["gsc"]["impressions"].as_f64().unwrap_or(0.0);
                signals.clicks = record["gsc"]["clicks"].as_f64().unwrap_or(0.0);
                signals.ctr = record["gsc"]["ctr"].as_f64().unwrap_or(0.0);
                signals.avg_position = record["gsc"]["avg_position"].as_f64().unwrap_or(0.0);
                signals.target_ctr = record["target_ctr"].as_f64().unwrap_or(0.0);
                signals.clicks_lost = record["clicks_lost"].as_f64().unwrap_or(0.0);
                signals.ctr_opportunity =
                    signals.clicks_lost >= 10.0 && signals.avg_position >= 1.0 && signals.avg_position <= 20.0;
            }
        }
    }

    // Cannibalization
    if let Some(can) = cannibalization {
        if let Some(clusters) = can["clusters"].as_array() {
            for cluster in clusters {
                if let Some(pages) = cluster["pages"].as_array() {
                    if pages.iter().any(|p| {
                        p["id"].as_i64() == Some(article_id)
                            || normalize_slug(p["url"].as_str().unwrap_or("")) == signals.url_slug
                    }) {
                        signals.cannibalized = true;
                        signals.hub_gap = !cluster["hub_exists"].as_bool().unwrap_or(true);
                        break;
                    }
                }
            }
        }
    }

    // Indexing
    if let Some(idx) = indexing {
        if let Some(targets) = idx["targets"].as_array() {
            if let Some(target) = targets.iter().find(|t| {
                normalize_slug(t["target"]["url"].as_str().unwrap_or("")) == signals.url_slug
                    || t["target"]["slug"].as_str().unwrap_or("") == signals.url_slug
            }) {
                signals.indexing_status = target["target"]["reason_code"]
                    .as_str()
                    .unwrap_or("not_indexed_other")
                    .to_string();
            }
        }
    }

    // Clarity UX anomaly
    if let Some(clarity) = clarity {
        if let Some(scores) = clarity["page_scores"].as_array() {
            if let Some(score) = scores.iter().find(|s| {
                normalize_slug(s["url"].as_str().unwrap_or("")) == signals.url_slug
            }) {
                signals.ux_anomaly_z_score = score["z_score"].as_f64().unwrap_or(0.0);
            }
        }
    }

    signals
}

fn normalize_slug(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .trim_end_matches('/')
        .to_lowercase()
}

pub(crate) fn should_skip(s: &ArticleSignals) -> bool {
    // Skip drafts and articles already being reviewed
    if s.review_status == "in_review" {
        return true;
    }

    // Skip articles edited very recently — give fixes time to mature in GSC
    if !s.last_edited_at.is_empty() {
        if let Ok(edited) = chrono::DateTime::parse_from_rfc3339(&s.last_edited_at) {
            let days = chrono::Utc::now()
                .signed_duration_since(edited.with_timezone(&chrono::Utc))
                .num_days()
                .max(0);
            if days < 30 {
                return true;
            }
        }
    }

    false
}

// ═══════════════════════════════════════════════════════════════════════════════
// Scoring
// ═══════════════════════════════════════════════════════════════════════════════

pub(crate) fn opportunity_score(s: &ArticleSignals) -> i64 {
    let mut score = 0i64;

    // CTR opportunity: 1 click lost ≈ 10 points, plus urgency bonus for top-10 positions
    score += (s.clicks_lost * 10.0) as i64;
    if s.ctr_opportunity && s.avg_position <= 10.0 {
        score += 500;
    }

    // Content health
    score += match s.content_health.as_str() {
        "poor" => 800,
        "needs_improvement" => 400,
        _ => 0,
    };
    score += s.checks_failed * 25;
    score += (100 - s.health_score).max(0) * 3;

    // Indexing
    score += match s.indexing_status.as_str() {
        "not_indexed_crawled" => 600,
        "not_indexed_other" | "not_indexed_discovered" => 300,
        _ => 0,
    };

    // Cannibalization
    if s.cannibalized && s.hub_gap {
        score += 500;
    } else if s.cannibalized {
        score += 250;
    }

    // UX anomaly: weight only when there is also search traffic
    if s.ux_anomaly_z_score > 2.0 && s.impressions > 50.0 {
        score += (s.ux_anomaly_z_score * 100.0) as i64;
    }

    // Quick-win boost
    if s.word_count > 0 && s.word_count < 600 && s.internal_links < 3 {
        score += 200;
    }

    score
}

pub(crate) fn classify_effort(s: &ArticleSignals) -> &'static str {
    if s.indexing_status.starts_with("not_indexed") && s.internal_links == 0 {
        "low"
    } else if s.cannibalized && s.hub_gap {
        "high"
    } else if s.content_health == "poor" || s.word_count < 600 {
        "medium"
    } else if s.ctr_opportunity {
        "low"
    } else {
        "medium"
    }
}

pub(crate) fn recommended_action(s: &ArticleSignals, effort: &str) -> &'static str {
    if s.indexing_status.starts_with("not_indexed") {
        "fix_indexing_internal_links"
    } else if s.cannibalized && s.hub_gap {
        "consolidate_cluster"
    } else if s.ctr_opportunity && s.content_health != "poor" {
        "fix_ctr_article"
    } else {
        "fix_content_article"
    }
}

pub(crate) fn primary_signal(s: &ArticleSignals) -> &'static str {
    if s.indexing_status.starts_with("not_indexed") {
        "indexing"
    } else if s.cannibalized && s.hub_gap {
        "cannibalization"
    } else if s.ctr_opportunity {
        "ctr"
    } else if s.content_health == "poor" || s.content_health == "needs_improvement" {
        "content_health"
    } else if s.ux_anomaly_z_score > 2.0 {
        "ux_anomaly"
    } else {
        "content_health"
    }
}

fn signals_to_json(s: &ArticleSignals) -> serde_json::Value {
    serde_json::json!({
        "content_health": s.content_health,
        "checks_failed": s.checks_failed,
        "health_score": s.health_score,
        "word_count": s.word_count,
        "internal_links": s.internal_links,
        "impressions": s.impressions,
        "clicks": s.clicks,
        "ctr": s.ctr,
        "avg_position": s.avg_position,
        "target_ctr": s.target_ctr,
        "clicks_lost": s.clicks_lost,
        "ctr_opportunity": s.ctr_opportunity,
        "indexing_status": s.indexing_status,
        "cannibalized": s.cannibalized,
        "hub_gap": s.hub_gap,
        "ux_anomaly_z_score": s.ux_anomaly_z_score,
    })
}
