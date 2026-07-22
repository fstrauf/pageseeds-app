use super::extract_post_details_from_task;
use crate::models::task::Task;
use rusqlite::Connection;

/// Fetch enriched Reddit opportunities from the database and return them as JSON.
/// This is called as the final step of the reddit workflow to return concrete
/// posting suggestions with drafted replies to the user.
pub fn exec_reddit_fetch_results(
    conn: &Connection,
    project_id: &str,
) -> crate::engine::workflows::StepResult {
    use crate::models::reddit::RedditOpportunity;

    log::info!(
        "[reddit_fetch_results] fetching enriched opportunities for project={}",
        project_id
    );

    let mut opportunities: Vec<RedditOpportunity> = Vec::new();

    match conn.prepare(
        "SELECT post_id, title, url, subreddit, author, posted_date, upvotes, comment_count,
                relevance_score, engagement_score, accessibility_score, final_score, severity,
                why_relevant, key_pain_points, website_fit, mention_stance, product_name, reply_status,
                reply_text, reply_url, reply_upvotes, reply_replies, posted_at,
                project_id, created_at, updated_at, selftext
         FROM reddit_opportunities
         WHERE project_id=?1 AND reply_status='pending'
         ORDER BY final_score DESC NULLS LAST, relevance_score DESC NULLS LAST
         LIMIT 20"
    ) {
        Ok(mut stmt) => {
            match stmt.query_map(rusqlite::params![project_id], |row| {
                let pain_points_json: String = row.get::<_, String>(14).unwrap_or_else(|_| "[]".to_string());
                let pain_points: Vec<String> = serde_json::from_str(&pain_points_json).unwrap_or_default();

                Ok(RedditOpportunity {
                    post_id: row.get(0)?,
                    title: row.get(1).ok(),
                    selftext: row.get(27).ok(),
                    url: row.get(2).ok(),
                    subreddit: row.get(3).ok(),
                    author: row.get(4).ok(),
                    posted_date: row.get(5).ok(),
                    upvotes: row.get(6).ok(),
                    comment_count: row.get(7).ok(),
                    relevance_score: row.get(8).ok(),
                    engagement_score: row.get(9).ok(),
                    accessibility_score: row.get(10).ok(),
                    final_score: row.get(11).ok(),
                    severity: row.get(12).ok(),
                    why_relevant: row.get(13).ok(),
                    key_pain_points: pain_points,
                    website_fit: row.get(15).ok(),
                    mention_stance: row.get(16).ok(),
                    product_name: row.get(17).ok(),
                    reply_status: row.get(18).unwrap_or_else(|_| "pending".to_string()),
                    reply_text: row.get(19).ok(),
                    reply_url: row.get(20).ok(),
                    reply_upvotes: row.get(21).ok(),
                    reply_replies: row.get(22).ok(),
                    posted_at: row.get(23).ok(),
                    project_id: row.get(24)?,
                    created_at: row.get(25)?,
                    updated_at: row.get(26)?,
                })
            }) {
                Ok(rows) => {
                    for opp in rows.flatten() {
                        opportunities.push(opp);
                    }
                }
                Err(e) => {
                    log::warn!("[reddit_fetch_results] query failed: {}", e);
                }
            }
        }
        Err(e) => {
            log::warn!("[reddit_fetch_results] prepare failed: {}", e);
        }
    }

    // Count opportunities with drafted replies
    let with_replies = opportunities
        .iter()
        .filter(|o| o.reply_text.is_some())
        .count();

    log::info!(
        "[reddit_fetch_results] found {} opportunities ({} with drafted replies)",
        opportunities.len(),
        with_replies
    );

    if opportunities.is_empty() {
        return crate::engine::workflows::StepResult {
            success: true,
            message: "No pending Reddit opportunities found. Run the search to find new posts."
                .to_string(),
            output: Some("[]".to_string()),
            artifact_key: None,
        };
    }

    match serde_json::to_string_pretty(&opportunities) {
        Ok(json) => crate::engine::workflows::StepResult {
            success: true,
            message: format!(
                "Found {} Reddit opportunities with {} drafted replies. Review them below:",
                opportunities.len(),
                with_replies
            ),
            output: Some(json),
            artifact_key: None,
        },
        Err(e) => crate::engine::workflows::StepResult::fail(format!("Failed to serialize opportunities: {}", e)),
    }
}

// ─── Post Reply to Reddit ────────────────────────────────────────────────────

/// Execute a reddit_reply task: post the reply to Reddit via API.
///
/// Extracts post_id and reply_text from the task description, validates the reply
/// (base rules + project mention stance) BEFORE any Reddit API call — a failed
/// validation fails the task — then posts the comment and updates the database.
pub fn exec_reddit_post_reply(
    task: &Task,
    project_path: &str,
    conn: &Connection,
) -> crate::engine::workflows::StepResult {
    use crate::config::env_resolver::EnvResolver;

    log::info!("[reddit_post_reply] starting for task {}", task.id);

    // Extract post_id and reply_text from task description
    let (post_id, reply_text) = match extract_post_details_from_task(task) {
        Some((id, text)) => (id, text),
        None => {
            return crate::engine::workflows::StepResult::fail("Could not extract post_id and reply_text from task description"
                    .to_string());
        }
    };

    log::info!("[reddit_post_reply] posting to post_id={}", post_id);

    // Validate the reply BEFORE any Reddit API call — the automated lane must
    // enforce the same rules as the manual UI lane (length, no URLs, no markdown
    // links, sentence/word counts, mention stance).
    let base_check = crate::reddit::validation::validate_reply(&reply_text);
    if !base_check.valid {
        let reason = base_check.error.unwrap_or_else(|| "invalid reply".to_string());
        log::warn!("[reddit_post_reply] validation failed: {}", reason);
        return crate::engine::workflows::StepResult::fail(format!("Reply validation failed: {}", reason));
    }
    let automation_dir = std::path::Path::new(project_path)
        .join(".github")
        .join("automation");
    let stance_check = crate::reddit::validation::validate_project_stance(&reply_text, &automation_dir);
    if !stance_check.valid {
        let reason = stance_check
            .error
            .unwrap_or_else(|| "mention stance violation".to_string());
        log::warn!("[reddit_post_reply] stance validation failed: {}", reason);
        return crate::engine::workflows::StepResult::fail(format!("Reply validation failed: {}", reason));
    }

    // Load Reddit credentials
    let resolver = EnvResolver::new(std::path::Path::new(project_path));

    let client_id = match resolver.resolve("REDDIT_CLIENT_ID") {
        Some((v, _)) => v,
        None => {
            return crate::engine::workflows::StepResult::fail("REDDIT_CLIENT_ID not set — add it to ~/.config/automation/secrets.env"
                    .to_string());
        }
    };

    let client_secret = match resolver.resolve("REDDIT_CLIENT_SECRET") {
        Some((v, _)) => v,
        None => {
            return crate::engine::workflows::StepResult::fail("REDDIT_CLIENT_SECRET not set — add it to ~/.config/automation/secrets.env"
                        .to_string());
        }
    };

    let refresh_token = match resolver.resolve("REDDIT_REFRESH_TOKEN") {
        Some((v, _)) => v,
        None => {
            return crate::engine::workflows::StepResult::fail("REDDIT_REFRESH_TOKEN not set — add it to ~/.config/automation/secrets.env"
                        .to_string());
        }
    };

    // Create a local runtime for the async Reddit API call
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            return crate::engine::workflows::StepResult::fail(format!("Failed to create runtime: {}", e));
        }
    };

    // Post to Reddit
    let result = rt.block_on(async {
        crate::reddit::post::submit_comment(
            &post_id,
            &reply_text,
            &client_id,
            &client_secret,
            &refresh_token,
        )
        .await
    });

    match result {
        Ok(comment_result) => {
            let reply_url = comment_result.permalink;
            let _now = chrono::Utc::now().to_rfc3339();

            // Update database with posted status
            if let Err(e) = crate::reddit::db::mark_posted(conn, &post_id, &reply_text, &reply_url)
            {
                log::warn!("[reddit_post_reply] failed to mark posted in DB: {}", e);
            }

            // Update history file
            let history_manager = crate::reddit::history::RedditHistoryManager::new(
                std::path::Path::new(project_path),
            );
            if let Err(e) = history_manager.mark_posted(&post_id) {
                log::warn!("[reddit_post_reply] failed to write history: {}", e);
            }

            log::info!(
                "[reddit_post_reply] successfully posted comment {}",
                comment_result.comment_id
            );

            crate::engine::workflows::StepResult {
                success: true,
                message: format!("Posted reply to Reddit: {}", reply_url),
                output: Some(format!(
                    "{{\"comment_id\":\"{}\",\"permalink\":\"{}\"}}",
                    comment_result.comment_id, reply_url
                )),
                artifact_key: None,
            }
        }
        Err(e) => {
            log::error!("[reddit_post_reply] failed to post: {}", e);
            crate::engine::workflows::StepResult::fail(format!("Failed to post to Reddit: {}", e))
        }
    }
}
