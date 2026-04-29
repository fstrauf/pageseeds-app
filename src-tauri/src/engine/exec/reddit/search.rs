use super::{load_search_params_from_artifact, parse_config_fallback};
use crate::models::task::Task;

/// Compute engagement, accessibility, and overall scores.
///
/// - engagement  = min(10, upvotes / max(1, days_old) / 10)
/// - accessibility = <10 comments→10, 10–30→8, 30–100→6, 100+→2
/// - relevance   = 5.0 (placeholder; AI pass upgrades it via `exec_reddit_enrich`)
/// - final_score = average of three components
pub(crate) fn compute_scores(
    upvotes: i64,
    comment_count: i64,
    days_old: i64,
) -> (f64, f64, f64, f64, &'static str) {
    let relevance_score: f64 = 5.0;
    let age = days_old.max(1) as f64;
    let engagement_score = (upvotes as f64 / age / 10.0).min(10.0).max(0.0);
    let accessibility_score: f64 = match comment_count {
        c if c < 10 => 10.0,
        c if c < 30 => 8.0,
        c if c < 100 => 6.0,
        _ => 2.0,
    };
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
    (
        relevance_score,
        engagement_score,
        accessibility_score,
        final_score,
        severity,
    )
}

// ─── Search ───────────────────────────────────────────────────────────────────

/// Deterministic Reddit search step.
///
/// Reads queries/subreddits from the structured search params artifact (produced by
/// reddit_config_parse_stage), calls the Reddit API, applies the 14-day filter and
/// MEDIUM+ score filter, deduplicates, and returns the full filtered candidate pool.
pub(crate) async fn exec_reddit_search(
    task: &Task,
    project_path: &str,
) -> crate::engine::workflows::StepResult {
    const MAX_AGE_DAYS: i64 = 14;
    const MAX_SEARCH_PAIRS: usize = 50;

    log::info!(
        "[reddit_search] starting for project={} path={}",
        task.project_id,
        project_path
    );

    // Try to load structured search params from artifact (produced by reddit_config_parse_stage)
    let params = load_search_params_from_artifact(task, project_path);

    // Fallback: parse config directly if no artifact (backward compatibility)
    let params = match params {
        Some(p) => p,
        None => {
            log::info!("[reddit_search] no structured params artifact found, falling back to direct config parse");
            let config_path = format!("{}/.github/automation/reddit_config.md", project_path);
            match std::fs::read_to_string(&config_path) {
                Ok(config) => parse_config_fallback(&config),
                Err(e) => {
                    return crate::engine::workflows::StepResult {
                        success: false,
                        message: format!(
                            "reddit_config.md not found at {} — create it first: {}",
                            config_path, e
                        ),
                        output: None,
                    }
                }
            }
        }
    };

    // Build queries list from keywords or topics
    let queries: Vec<String> = if !params.query_keywords.is_empty() {
        params.query_keywords.clone()
    } else {
        params.trigger_topics.clone()
    };

    let seed_subs = params.seed_subreddits;
    let excluded_subs = params.excluded_subreddits;
    let mention_stance = params.mention_stance;

    log::info!(
        "[reddit_search] queries ({}) {:?}  seed_subreddits ({}) {:?}",
        queries.len(),
        &queries[..queries.len().min(5)],
        seed_subs.len(),
        &seed_subs[..seed_subs.len().min(5)]
    );

    if queries.is_empty() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: "No search queries found. The reddit_config_parse_stage should have extracted query_keywords or trigger_topics from reddit_config.md.".to_string(),
            output: None,
        };
    }

    let search_pairs: Vec<(String, String)> = if seed_subs.is_empty() {
        log::warn!("[reddit_search] no seed subreddits — falling back to global search");
        queries
            .iter()
            .take(MAX_SEARCH_PAIRS)
            .map(|q| (String::new(), q.clone()))
            .collect()
    } else {
        // Round-robin across subreddits so each gets a fair share of queries.
        // With 50 pairs and 15 subreddits, each gets ~3 queries instead of
        // exhausting all 43 queries on sub1 before touching sub2.
        let mut pairs: Vec<(String, String)> = Vec::new();
        let mut query_idx = 0usize;
        while pairs.len() < MAX_SEARCH_PAIRS && query_idx < queries.len() {
            for sub in &seed_subs {
                if pairs.len() >= MAX_SEARCH_PAIRS {
                    break;
                }
                pairs.push((sub.clone(), queries[query_idx].clone()));
            }
            query_idx += 1;
        }
        pairs
    };
    log::info!("[reddit_search] {} search pairs", search_pairs.len());

    let mut all_posts: Vec<serde_json::Value> = Vec::new();
    let mut seen_ids: std::collections::HashSet<String> = Default::default();
    let mut subreddit_counts: std::collections::HashMap<String, usize> = Default::default();
    const MAX_RESULTS_PER_SUBREDDIT: usize = 5;
    let mut too_old = 0usize;
    let mut excluded_sub_count = 0usize;
    let mut below_threshold = 0usize;
    let mut history_filtered = 0usize;
    let mut subreddit_capped = 0usize;

    let history_manager =
        crate::reddit::history::RedditHistoryManager::new(std::path::Path::new(project_path));
    let handled_ids = history_manager.get_all_handled_ids();

    // Resolve Reddit OAuth credentials if available — OAuth search avoids the
    // aggressive bot detection that blocks the public JSON API.
    let resolver = crate::config::env_resolver::EnvResolver::new(project_path);
    let env_files = resolver.env_files();
    log::info!(
        "[reddit_search] resolver env_files ({}): {:?}",
        env_files.len(),
        env_files
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
    );
    let resolved_id = resolver.resolve("REDDIT_CLIENT_ID");
    let resolved_secret = resolver.resolve("REDDIT_CLIENT_SECRET");
    let resolved_token = resolver.resolve("REDDIT_REFRESH_TOKEN");
    log::info!(
        "[reddit_search] credential resolution: client_id={} client_secret={} refresh_token={}",
        if resolved_id.is_some() {
            "found"
        } else {
            "missing"
        },
        if resolved_secret.is_some() {
            "found"
        } else {
            "missing"
        },
        if resolved_token.is_some() {
            "found"
        } else {
            "missing"
        }
    );
    let reddit_creds = match (resolved_id, resolved_secret, resolved_token) {
        (Some((id, _)), Some((secret, _)), Some((token, _))) => {
            log::info!("[reddit_search] using OAuth-authenticated search (oauth.reddit.com)");
            Some(crate::reddit::search::RedditCredentials {
                client_id: id,
                client_secret: secret,
                refresh_token: token,
            })
        }
        _ => {
            log::warn!(
                "[reddit_search] no Reddit OAuth credentials found — falling back to public API. \
                 If you get 403 errors, add REDDIT_CLIENT_ID, REDDIT_CLIENT_SECRET, and REDDIT_REFRESH_TOKEN to ~/.config/automation/secrets.env"
            );
            None
        }
    };

    // Consecutive failures tracker — if Reddit is actively blocking us, stop early
    let mut consecutive_failures = 0usize;
    const MAX_CONSECUTIVE_FAILURES: usize = 5;
    const REQUEST_DELAY_MS: u64 = 2500; // 2.5s between requests to stay under Reddit's radar

    for (subreddit, query) in &search_pairs {
        if consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
            log::warn!(
                "[reddit_search] stopping early after {} consecutive failures — Reddit appears to be blocking requests",
                consecutive_failures
            );
            break;
        }

        let result = match crate::reddit::search::search_submissions(
            query,
            subreddit,
            10,
            "relevance",
            "week",
            REQUEST_DELAY_MS,
            reddit_creds.as_ref(),
        )
        .await
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!(
                    "[reddit_search] search failed sub={:?} q={:?}: {}",
                    subreddit,
                    query,
                    e
                );
                consecutive_failures += 1;
                continue;
            }
        };

        if result.was_rate_limited {
            consecutive_failures += 1;
            continue;
        }
        consecutive_failures = 0; // reset on success

        let posts = result.posts;
        let before = all_posts.len();
        for post in posts {
            let post_id = post.post_id.clone();

            if let Some(sub) = post.subreddit.as_ref() {
                let sub_lower = sub.to_lowercase();
                if excluded_subs.contains(&sub_lower) {
                    excluded_sub_count += 1;
                    continue;
                }
            }

            let days_old = post.days_old.unwrap_or(0);
            if days_old > MAX_AGE_DAYS {
                too_old += 1;
                continue;
            }

            if !seen_ids.insert(post_id.clone()) {
                continue;
            }

            if handled_ids.contains(&post_id) {
                history_filtered += 1;
                continue;
            }

            // Enforce per-subreddit cap so no single community dominates results
            let sub_key = post.subreddit.clone().unwrap_or_default().to_lowercase();
            let sub_count = subreddit_counts.entry(sub_key.clone()).or_insert(0);
            if *sub_count >= MAX_RESULTS_PER_SUBREDDIT {
                subreddit_capped += 1;
                continue;
            }
            *sub_count += 1;

            let upvotes = post.upvotes.unwrap_or(0);
            let comments = post.comment_count.unwrap_or(0);
            let (relevance, engagement, accessibility, final_score, severity) =
                compute_scores(upvotes, comments, days_old);

            if final_score < 5.0 {
                below_threshold += 1;
                continue;
            }

            all_posts.push(serde_json::json!({
                "post_id": post_id,
                "title": post.title,
                "url": post.url,
                "subreddit": post.subreddit,
                "author": post.author,
                "upvotes": upvotes,
                "comment_count": comments,
                "days_old": days_old,
                "created_at": post.created_at,
                "posted_date": post.created_at,
                "selftext": post.selftext,
                "relevance_score": relevance,
                "engagement_score": engagement,
                "accessibility_score": accessibility,
                "final_score": final_score,
                "severity": severity,
                "mention_stance": mention_stance,
            }));
        }
        log::info!(
            "[reddit_search] +{} accepted (total {})",
            all_posts.len() - before,
            all_posts.len()
        );
    }

    all_posts.sort_by(|a, b| {
        let fa = a["final_score"].as_f64().unwrap_or(0.0);
        let fb = b["final_score"].as_f64().unwrap_or(0.0);
        fb.partial_cmp(&fa).unwrap_or(std::cmp::Ordering::Equal)
    });

    log::info!(
        "[reddit_search] done — kept={} too_old={} excluded_sub={} below_threshold={} history_filtered={} subreddit_capped={}",
        all_posts.len(), too_old, excluded_sub_count, below_threshold, history_filtered, subreddit_capped
    );

    if all_posts.is_empty() {
        return crate::engine::workflows::StepResult {
            success: false,
            message: format!(
                "No Reddit posts found across {} search pairs ({} too old, {} excluded, {} below threshold, {} subreddit-capped)",
                search_pairs.len(), too_old, excluded_sub_count, below_threshold, subreddit_capped
            ),
            output: None,
        };
    }

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Found {} Reddit posts across {} subreddits ({} too old, {} excluded, {} below threshold, {} already handled, {} subreddit-capped)",
            all_posts.len(), subreddit_counts.len(), too_old, excluded_sub_count, below_threshold, history_filtered, subreddit_capped
        ),
        output: Some(serde_json::to_string(&serde_json::json!({"posts": all_posts})).unwrap_or_default()),
    }
}
