use super::PostTaskContext;

// ─── Quality gate + topic health helpers ─────────────────────────────────────

/// Find the article file that was just written by a content task.
///
/// Priority:
/// 1. "File: ..." in task description.
/// 2. Most recently created/updated article for the project.
pub(crate) fn find_written_article_file(ctx: &PostTaskContext<'_>) -> Option<String> {
    let desc = ctx.task.description.as_deref().unwrap_or("");

    // 1. File path from description.
    if let Some(start) = desc.find("File: ") {
        let rest = &desc[start + 6..];
        let end = rest.find(" |").or_else(|| rest.find('\n')).unwrap_or(rest.len());
        let file = rest[..end].trim();
        if !file.is_empty() {
            return Some(file.to_string());
        }
    }

    // 2. Fallback: most recent article for the project.
    let row: Result<String, rusqlite::Error> = ctx.conn.query_row(
        "SELECT file FROM articles
         WHERE project_id = ?1 AND file IS NOT NULL AND file != ''
         ORDER BY COALESCE(updated_at, created_at) DESC
         LIMIT 1",
        rusqlite::params![&ctx.task.project_id],
        |r| r.get(0),
    );
    row.ok()
}

/// Pure classification logic for topic health.
///
/// Extracted so the threshold math can be unit-tested without filesystem or DB state.
pub(crate) fn classify_topic_health(
    avg_quality: i64,
    quality_count: i64,
    total_clicks: f64,
    total_impressions: f64,
) -> (&'static str, Option<f64>) {
    let health_status = if avg_quality >= 70 && (total_clicks > 0.0 || total_impressions >= 1000.0) {
        "promising"
    } else if avg_quality < 50 && total_impressions < 100.0 && total_clicks == 0.0 {
        "depleted"
    } else {
        "unproven"
    };

    let signal_score = if quality_count > 0 {
        Some((avg_quality as f64) + (total_clicks * 10.0) + (total_impressions / 100.0))
    } else {
        None
    };

    (health_status, signal_score)
}

/// Reduce content review / audit signals into per-topic health scores on research_shortlist.
pub(crate) fn run_topic_health_reducer(ctx: &PostTaskContext<'_>) -> crate::error::Result<()> {
    use crate::db::research_shortlist;

    // Load latest audit artifacts for this project.
    let paths = crate::engine::project_paths::ProjectPaths::from_path(ctx.project_path);
    let audit_path = paths.automation_dir.join("content_audit.json");
    let audit_json = std::fs::read_to_string(&audit_path).unwrap_or_default();
    if audit_json.is_empty() {
        return Ok(());
    }
    let audit: serde_json::Value = serde_json::from_str(&audit_json).unwrap_or_default();
    let articles = audit["articles"].as_array().unwrap_or(&Vec::new()).clone();
    if articles.is_empty() {
        return Ok(());
    }

    // Group audited articles by target_keyword/theme and aggregate signals.
    let mut by_theme: std::collections::HashMap<String, Vec<serde_json::Value>> = std::collections::HashMap::new();
    for article in articles {
        let theme = article["target_keyword"]
            .as_str()
            .or_else(|| article["url_slug"].as_str())
            .unwrap_or("")
            .to_string();
        if theme.is_empty() {
            continue;
        }
        by_theme.entry(theme).or_default().push(article);
    }

    for (theme, items) in by_theme {
        let mut total_quality: i64 = 0;
        let mut quality_count: i64 = 0;
        let mut total_impressions: f64 = 0.0;
        let mut total_clicks: f64 = 0.0;
        let mut min_quality: i64 = i64::MAX;

        for item in &items {
            let quality = item["quality_score"].as_i64().unwrap_or(0);
            if quality > 0 {
                total_quality += quality;
                quality_count += 1;
                min_quality = min_quality.min(quality);
            }
            total_impressions += item["gsc"]["impressions"].as_f64().unwrap_or(0.0);
            total_clicks += item["gsc"]["clicks"].as_f64().unwrap_or(0.0);
        }

        let avg_quality = if quality_count > 0 {
            total_quality / quality_count
        } else {
            0
        };

        let (health_status, signal_score) = classify_topic_health(
            avg_quality,
            quality_count,
            total_clicks,
            total_impressions,
        );

        let normalized_theme = theme.to_lowercase().trim().to_string();
        if normalized_theme.is_empty() {
            continue;
        }

        // Update exact theme match if it exists; otherwise update any shortlist entry
        // whose theme contains the keyword or vice versa.
        let existing = research_shortlist::list_entries(ctx.conn, &ctx.task.project_id, None)?;
        let matched_id = existing.iter().find(|e| e.theme.to_lowercase() == normalized_theme).map(|e| e.id);

        if matched_id.is_some() {
            research_shortlist::update_health(
                ctx.conn,
                &ctx.task.project_id,
                &theme,
                health_status,
                signal_score,
            )?;
        } else {
            // Best-effort fuzzy match: first shortlist theme that contains the keyword.
            for entry in existing {
                if normalized_theme.contains(&entry.theme.to_lowercase())
                    || entry.theme.to_lowercase().contains(&normalized_theme)
                {
                    research_shortlist::update_health(
                        ctx.conn,
                        &ctx.task.project_id,
                        &entry.theme,
                        health_status,
                        signal_score,
                    )?;
                    break;
                }
            }
        }
    }

    Ok(())
}
