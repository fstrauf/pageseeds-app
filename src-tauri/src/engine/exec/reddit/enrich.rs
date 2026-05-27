use super::load_search_params_from_artifact;
use crate::models::task::Task;
use rusqlite::Connection;
use std::path::Path;

// ─── Persist ─────────────────────────────────────────────────────────────────

/// Parse a JSON array of Reddit opportunity objects and upsert each into SQLite.
///
/// Tolerates partial fields — only `post_id` is required.
/// Clears pending rows from previous runs before inserting fresh results.
/// Rows with reply_status='posted' or 'skipped' are preserved.
pub(crate) fn persist_reddit_opportunities(conn: &Connection, project_id: &str, json_str: &str) {
    log::info!(
        "[reddit] persist_reddit_opportunities project={} json_len={}",
        project_id,
        json_str.len()
    );

    let value: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            let preview = crate::engine::text::char_prefix(json_str, 200);
            log::warn!(
                "[reddit] failed to parse JSON: {} — first 200 chars: {:?}",
                e,
                preview
            );
            return;
        }
    };

    let array = if value.is_array() {
        value.as_array().cloned().unwrap_or_default()
    } else if let Some(arr) = ["opportunities", "results", "posts", "items"]
        .iter()
        .find_map(|key| value.get(key).and_then(|v| v.as_array()).cloned())
    {
        arr
    } else {
        log::warn!(
            "[reddit] unrecognised JSON structure — keys: {:?}",
            value
                .as_object()
                .map(|o| o.keys().cloned().collect::<Vec<_>>())
        );
        return;
    };

    // Clear pending rows from previous runs; preserve posted/skipped for history dedup.
    let deleted = conn
        .execute(
            "DELETE FROM reddit_opportunities WHERE project_id=?1 AND reply_status='pending'",
            rusqlite::params![project_id],
        )
        .unwrap_or(0);
    if deleted > 0 {
        log::info!(
            "[reddit] cleared {} stale pending rows for project={}",
            deleted,
            project_id
        );
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut upserted = 0usize;
    let mut skipped = 0usize;

    for item in &array {
        let post_id = match item.get("post_id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => {
                skipped += 1;
                continue;
            }
        };

        let already_handled: bool = conn.query_row(
            "SELECT COUNT(*) FROM reddit_opportunities WHERE post_id=?1 AND reply_status IN ('posted','skipped')",
            rusqlite::params![post_id],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0) > 0;
        if already_handled {
            skipped += 1;
            continue;
        }

        let opp = crate::models::reddit::RedditOpportunity {
            post_id,
            title: item
                .get("title")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            url: item.get("url").and_then(|v| v.as_str()).map(str::to_string),
            subreddit: item
                .get("subreddit")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            author: item
                .get("author")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            posted_date: item
                .get("posted_date")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            upvotes: item.get("upvotes").and_then(|v| v.as_i64()),
            comment_count: item.get("comment_count").and_then(|v| v.as_i64()),
            relevance_score: item.get("relevance_score").and_then(|v| v.as_f64()),
            engagement_score: item.get("engagement_score").and_then(|v| v.as_f64()),
            accessibility_score: item.get("accessibility_score").and_then(|v| v.as_f64()),
            final_score: item.get("final_score").and_then(|v| v.as_f64()),
            severity: item
                .get("severity")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            why_relevant: item
                .get("why_relevant")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            key_pain_points: item
                .get("key_pain_points")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            website_fit: item
                .get("website_fit")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            mention_stance: item
                .get("mention_stance")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            product_name: None, // Will be set during enrichment from artifact
            reply_status: item
                .get("reply_status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending")
                .to_string(),
            reply_text: item
                .get("reply_text")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            reply_url: item
                .get("reply_url")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            reply_upvotes: item.get("reply_upvotes").and_then(|v| v.as_i64()),
            reply_replies: item.get("reply_replies").and_then(|v| v.as_i64()),
            posted_at: item
                .get("posted_at")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            project_id: project_id.to_string(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };

        match crate::reddit::db::upsert_opportunity(conn, &opp) {
            Ok(_) => {
                upserted += 1;
            }
            Err(e) => {
                log::warn!("[reddit] upsert failed post_id={}: {}", opp.post_id, e);
                skipped += 1;
            }
        }
    }

    log::info!(
        "[reddit] done — upserted={} skipped={} project={}",
        upserted,
        skipped,
        project_id
    );
}

// ─── Enrichment ───────────────────────────────────────────────────────────────

/// AI enrichment pass: fills in `why_relevant`, `key_pain_points`, `website_fit`,
/// and draft `reply_text`, and recalculates `relevance_score` / `final_score`.
///
/// Fetches up to 5 un-enriched posts per call; silently returns if none pending.
///
/// Reads product_name and mention_stance from the reddit_config_parse_stage artifact
/// (produced by the agentic config parse step). Falls back to deterministic parsing
/// only if no artifact is found.
pub fn exec_reddit_enrich(
    conn: &Connection,
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) {
    let project_id = &task.project_id;
    log::info!("[reddit_enrich] starting for project={}", project_id);

    let rows: Vec<(
        String,
        Option<String>,
        Option<String>,
        Option<f64>,
        Option<f64>,
    )> = {
        let mut result = Vec::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT post_id, title, subreddit, engagement_score, accessibility_score \
             FROM reddit_opportunities \
             WHERE project_id=?1 \
               AND (why_relevant IS NULL OR reply_text IS NULL) \
               AND reply_status != 'skipped' \
             LIMIT 5",
        ) {
            if let Ok(mapped) = stmt.query_map(rusqlite::params![project_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<f64>>(3)?,
                    row.get::<_, Option<f64>>(4)?,
                ))
            }) {
                for item in mapped.flatten() {
                    result.push(item);
                }
            } else {
                log::warn!("[reddit_enrich] query failed");
                return;
            }
        } else {
            log::warn!("[reddit_enrich] prepare failed");
            return;
        }
        result
    };

    if rows.is_empty() {
        log::info!("[reddit_enrich] no unenriched posts — skipping");
        return;
    }
    log::info!("[reddit_enrich] {} posts to enrich", rows.len());

    let automation_dir = Path::new(project_path).join(".github").join("automation");
    // Primary: project.md (consolidated). Fallback: legacy files.
    let project_context = std::fs::read_to_string(automation_dir.join("project.md"))
        .or_else(|_| {
            // Legacy fallback: stitch old files together
            let summary = std::fs::read_to_string(automation_dir.join("project_summary.md"))
                .unwrap_or_default();
            let brand =
                std::fs::read_to_string(automation_dir.join("brandvoice.md")).unwrap_or_default();
            let brief = std::fs::read_to_string(automation_dir.join("seo_content_brief.md"))
                .unwrap_or_default();
            Ok::<String, std::io::Error>(format!("{}\n\n{}\n\n{}", summary, brand, brief))
        })
        .unwrap_or_default();
    let reddit_config_raw =
        std::fs::read_to_string(automation_dir.join("reddit_config.md")).unwrap_or_default();
    let guardrails =
        std::fs::read_to_string(automation_dir.join("reddit").join("_reply_guardrails.md"))
            .unwrap_or_default();

    if project_context.is_empty() && reddit_config_raw.is_empty() {
        log::warn!("[reddit_enrich] no project context — skipping");
        return;
    }

    // Try to load structured params from the artifact (produced by reddit_config_parse_stage)
    let (product_name, mention_stance_str) =
        match load_search_params_from_artifact(task, project_path) {
            Some(params) => {
                let name = params
                    .product_name
                    .unwrap_or_else(|| "the product".to_string());
                let stance = params.mention_stance.to_uppercase();
                log::info!(
                    "[reddit_enrich] using artifact params: product_name='{}', stance='{}'",
                    name,
                    stance
                );
                (name, stance)
            }
            None => {
                // Fallback: deterministic parse from reddit_config.md
                log::info!(
                    "[reddit_enrich] no artifact found, falling back to deterministic parse"
                );
                let cfg = crate::reddit::config::parse_reddit_config(&reddit_config_raw);
                let name = cfg
                    .product_name
                    .unwrap_or_else(|| "the product".to_string());
                let stance = cfg.mention_stance.as_str().to_string();
                (name, stance)
            }
        };

    let stance_instruction = match mention_stance_str.as_str() {
        "REQUIRED" => format!(
            "REQUIRED: The reply MUST contain the exact product name \"{}\" — no vague substitutes.",
            product_name
        ),
        "RECOMMENDED" => format!(
            "RECOMMENDED: Mention \"{}\" by name if the topic is a natural fit.",
            product_name
        ),
        "OPTIONAL" => format!(
            "OPTIONAL: You may mention \"{}\" if it fits naturally.",
            product_name
        ),
        "OMIT" =>
            "OMIT: Do NOT mention any product name in this reply.".to_string(),
        _ => format!(
            "OPTIONAL: You may mention \"{}\" if it fits naturally.",
            product_name
        ),
    };

    let posts_block: String = rows
        .iter()
        .enumerate()
        .map(|(i, (pid, title, sub, _, _))| {
            format!(
                "{}. post_id=\"{}\"  subreddit=\"{}\"  title=\"{}\"",
                i + 1,
                pid,
                sub.as_deref().unwrap_or("unknown"),
                title
                    .as_deref()
                    .unwrap_or("(no title)")
                    .replace('"', "'")
                    .chars()
                    .take(200)
                    .collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let context = format!(
        "## PROJECT CONTEXT\n{project_context}\n\n\
         ## REDDIT CONFIG\n{reddit_config_raw}\n\n\
         ## REPLY GUARDRAILS\n{guardrails}\n\n\
         ## PRODUCT MENTION RULES\n\
         Product name: {product_name}\n\
         Mention stance: {mention_stance_str}\n\
         {stance_instruction}\n\n\
         ## POST TITLES\n\
         {posts_block}",
        project_context = project_context,
        reddit_config_raw = reddit_config_raw,
        guardrails = guardrails,
        product_name = product_name,
        mention_stance_str = mention_stance_str,
        stance_instruction = stance_instruction,
        posts_block = posts_block,
    );

    let repo_root = Path::new(project_path);
    // The reddit-enrich skill file already contains the canonical Output Contract.
    let output = match crate::engine::agent::run_agent_with_skill(
        "reddit-enrich",
        repo_root,
        &context,
        agent_provider,
        None,
    ) {
        Ok(o) => o,
        Err(e) => {
            log::warn!("[reddit_enrich] agent failed: {}", e);
            return;
        }
    };

    let enrichments: Vec<serde_json::Value> = match crate::engine::text::extract_json(&output) {
        Some(value) => match value {
            serde_json::Value::Array(arr) => arr,
            _ => {
                let preview = crate::engine::text::char_prefix(&output, 300);
                log::warn!("[reddit_enrich] agent output is not a JSON array — first 300 chars: {:?}",
                    preview);
                return;
            }
        },
        None => {
            let preview = crate::engine::text::char_prefix(&output, 300);
            log::warn!("[reddit_enrich] could not extract JSON from agent output — first 300 chars: {:?}",
                preview);
            return;
        }
    };

    let now = chrono::Utc::now().to_rfc3339();
    let mut updated = 0usize;

    for item in &enrichments {
        let post_id = match item.get("post_id").and_then(|v| v.as_str()) {
            Some(id) => id,
            None => continue,
        };

        let relevance_score = item
            .get("relevance_score")
            .and_then(|v| v.as_f64())
            .unwrap_or(5.0)
            .max(0.0)
            .min(10.0);

        let why_relevant = item
            .get("why_relevant")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let website_fit = item
            .get("website_fit")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let reply_text = item
            .get("reply_text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let pain_points_json = item
            .get("key_pain_points")
            .and_then(|v| v.as_array())
            .map(|arr| serde_json::to_string(arr).unwrap_or_else(|_| "[]".to_string()))
            .unwrap_or_else(|| "[]".to_string());

        let (engagement_score, accessibility_score): (f64, f64) = rows
            .iter()
            .find(|(pid, _, _, _, _)| pid == post_id)
            .map(|(_, _, _, eng, acc)| (eng.unwrap_or(5.0), acc.unwrap_or(5.0)))
            .unwrap_or((5.0, 5.0));

        let final_score = (relevance_score + engagement_score + accessibility_score) / 3.0;
        let severity = if final_score >= 8.5 {
            "CRITICAL"
        } else if final_score >= 7.0 {
            "HIGH"
        } else if final_score >= 5.0 {
            "MEDIUM"
        } else {
            "LOW"
        };

        match conn.execute(
            "UPDATE reddit_opportunities \
             SET relevance_score=?1, why_relevant=?2, key_pain_points=?3, website_fit=?4, \
                 final_score=?5, severity=?6, reply_text=?7, mention_stance=?8, product_name=?9, updated_at=?10 \
             WHERE post_id=?11 AND project_id=?12",
            rusqlite::params![
                relevance_score, why_relevant, pain_points_json, website_fit,
                final_score, severity,
                if reply_text.is_empty() { None } else { Some(reply_text) },
                &mention_stance_str,
                &product_name,
                now, post_id, project_id
            ],
        ) {
            Ok(n) if n > 0 => { updated += 1; }
            Ok(_) => log::warn!("[reddit_enrich] post_id={} not found in DB", post_id),
            Err(e) => log::warn!("[reddit_enrich] update failed for {}: {}", post_id, e),
        }
    }

    log::info!(
        "[reddit_enrich] enriched+drafted {}/{} posts project={}",
        updated,
        rows.len(),
        project_id
    );
}
