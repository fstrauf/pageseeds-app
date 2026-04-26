use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::time::Duration;

use crate::error::Result;
use crate::models::reddit::SubmissionSummary;

// ─── Reddit JSON API response shapes ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: SearchData,
}

#[derive(Debug, Deserialize)]
struct SearchData {
    children: Vec<SearchChild>,
}

#[derive(Debug, Deserialize)]
struct SearchChild {
    data: PostData,
}

#[derive(Debug, Deserialize)]
struct PostData {
    id: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    selftext: Option<String>,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    score: Option<i64>,
    #[serde(default)]
    subreddit: Option<String>,
    #[serde(default)]
    permalink: Option<String>,
    #[serde(default)]
    created_utc: Option<f64>,
    #[serde(default)]
    num_comments: Option<i64>,
}

// ─── Public search function ───────────────────────────────────────────────────

/// Shared reqwest client for Reddit API calls.
/// Created once per process to reuse connections and TCP/TLS state.
fn reddit_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/135.0.0.0 Safari/537.36")
        .timeout(Duration::from_secs(15))
        .connect_timeout(Duration::from_secs(10))
        .build()?)
}

/// OAuth credentials for authenticated Reddit API access.
/// When provided, search uses `oauth.reddit.com` which has higher rate limits
/// and is not subject to the same bot detection as the public JSON API.
pub struct RedditCredentials {
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
}

/// Result of a single Reddit search attempt.
#[allow(dead_code)]
pub struct SearchResult {
    pub posts: Vec<SubmissionSummary>,
    pub was_rate_limited: bool,
    pub used_oauth: bool,
}

/// Search Reddit submissions with automatic retry on 429 rate-limit.
///
/// - If `credentials` is provided, uses OAuth (`oauth.reddit.com`) which avoids
///   the public API bot detection that causes 403s.
/// - If `credentials` is None, falls back to the public `www.reddit.com/search.json`
///   with pacing delays and retries.
/// - Waits `delay_ms` before making the request (callers should pace requests).
/// - Retries up to 2 times on 429 with exponential backoff.
/// - Returns `was_rate_limited=true` if the final attempt still got 429/403,
///   so the caller can decide to skip remaining queries.
pub async fn search_submissions(
    query: &str,
    subreddit: &str,
    limit: i32,
    sort: &str,
    time_filter: &str,
    delay_ms: u64,
    credentials: Option<&RedditCredentials>,
) -> Result<SearchResult> {
    // Pace requests: sleep before each call
    if delay_ms > 0 {
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }

    let (base, use_oauth) = if let Some(_creds) = credentials {
        let url = if subreddit.is_empty() {
            "https://oauth.reddit.com/search.json".to_string()
        } else {
            format!("https://oauth.reddit.com/r/{}/search.json", subreddit)
        };
        (url, true)
    } else {
        let url = if subreddit.is_empty() {
            "https://www.reddit.com/search.json".to_string()
        } else {
            format!("https://www.reddit.com/r/{}/search.json", subreddit)
        };
        (url, false)
    };

    let client = reddit_client()?;

    // Build access token for OAuth mode
    let oauth_token = if use_oauth {
        let creds = credentials.unwrap();
        match crate::reddit::post::get_access_token(
            &client,
            &creds.client_id,
            &creds.client_secret,
            &creds.refresh_token,
        ).await {
            Ok(token) => Some(token),
            Err(e) => {
                log::warn!(
                    "[reddit_search] OAuth token refresh failed, falling back to public API: {}",
                    e
                );
                None
            }
        }
    } else {
        None
    };

    // If OAuth token failed to refresh, fall back to public URL
    let (base, use_oauth) = if oauth_token.is_none() && use_oauth {
        let url = if subreddit.is_empty() {
            "https://www.reddit.com/search.json".to_string()
        } else {
            format!("https://www.reddit.com/r/{}/search.json", subreddit)
        };
        (url, false)
    } else {
        (base, use_oauth)
    };

    let mut query_params: Vec<(&str, String)> = vec![
        ("q", query.to_string()),
        ("limit", limit.to_string()),
        ("sort", sort.to_string()),
        ("t", time_filter.to_string()),
        ("type", "link".to_string()),
    ];
    if !subreddit.is_empty() {
        query_params.push(("restrict_sr", "1".to_string()));
    }

    // Retry loop: up to 3 attempts total with exponential backoff on 429
    let mut last_err: Option<reqwest::Error> = None;
    for attempt in 0..3 {
        if attempt > 0 {
            let backoff = 2u64.saturating_pow(attempt - 1) * 1000;
            log::info!(
                "[reddit_search] retry attempt {} after {}ms for sub={:?} q={:?}",
                attempt + 1,
                backoff,
                subreddit,
                query
            );
            tokio::time::sleep(Duration::from_millis(backoff)).await;
        }

        let mut req = client.get(&base).query(&query_params);
        if let Some(ref token) = oauth_token {
            req = req.bearer_auth(token);
        }

        match req.send().await {
            Ok(response) => {
                let status = response.status();
                if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    log::warn!(
                        "[reddit_search] 429 Too Many Requests sub={:?} q={:?} attempt={}",
                        subreddit,
                        query,
                        attempt + 1
                    );
                    if let Some(retry_after) = response.headers().get("retry-after") {
                        if let Ok(sec_str) = retry_after.to_str() {
                            if let Ok(secs) = sec_str.parse::<u64>() {
                                log::info!(
                                    "[reddit_search] honoring Retry-After: {}s",
                                    secs
                                );
                                tokio::time::sleep(Duration::from_secs(secs)).await;
                                continue;
                            }
                        }
                    }
                    last_err = Some(response.error_for_status().unwrap_err());
                    continue;
                }
                if status == reqwest::StatusCode::FORBIDDEN {
                    log::warn!(
                        "[reddit_search] 403 Forbidden sub={:?} q={:?} — Reddit may be blocking this IP/client",
                        subreddit,
                        query
                    );
                    return Ok(SearchResult {
                        posts: vec![],
                        was_rate_limited: true,
                        used_oauth: use_oauth,
                    });
                }
                if status == reqwest::StatusCode::UNAUTHORIZED && use_oauth {
                    log::warn!(
                        "[reddit_search] 401 Unauthorized sub={:?} q={:?} — OAuth token may be expired",
                        subreddit,
                        query
                    );
                    return Ok(SearchResult {
                        posts: vec![],
                        was_rate_limited: true,
                        used_oauth: use_oauth,
                    });
                }
                match response.error_for_status() {
                    Ok(ok_resp) => {
                        let resp: SearchResponse = ok_resp.json().await.map_err(|e| {
                            crate::error::Error::Other(format!(
                                "JSON parse error: {}"
                            , e))
                        })?;
                        let posts = parse_posts(resp);
                        return Ok(SearchResult {
                            posts,
                            was_rate_limited: false,
                            used_oauth: use_oauth,
                        });
                    }
                    Err(e) => {
                        last_err = Some(e);
                        break; // other 4xx/5xx — don't retry
                    }
                }
            }
            Err(e) => {
                last_err = Some(e);
                break;
            }
        }
    }

    // All retries exhausted
    if let Some(e) = last_err {
        Err(crate::error::Error::Other(format!(
            "Reddit search failed after retries: {}"
        , e)))
    } else {
        Ok(SearchResult {
            posts: vec![],
            was_rate_limited: true,
            used_oauth: use_oauth,
        })
    }
}

fn parse_posts(resp: SearchResponse) -> Vec<SubmissionSummary> {
    let now = Utc::now();
    resp.data
        .children
        .into_iter()
        .map(|child| {
            let p = child.data;

            let created_at = p.created_utc.and_then(|ts| {
                DateTime::from_timestamp(ts as i64, 0).map(|dt| dt.to_rfc3339())
            });

            let days_old = p.created_utc.and_then(|ts| {
                DateTime::from_timestamp(ts as i64, 0)
                    .map(|created| (now - created).num_days())
            });

            let url = p.permalink.map(|permalink| {
                if permalink.starts_with("http") {
                    permalink
                } else {
                    format!("https://www.reddit.com{}", permalink)
                }
            });

            // Suppress "[removed]" / "[deleted]" boilerplate
            let selftext = p.selftext.filter(|s| {
                !s.is_empty() && s != "[removed]" && s != "[deleted]"
            });

            SubmissionSummary {
                post_id: p.id,
                title: p.title,
                url,
                subreddit: p.subreddit,
                author: Some(p.author.unwrap_or_else(|| "[deleted]".to_string())),
                upvotes: p.score,
                comment_count: p.num_comments,
                created_at,
                days_old,
                selftext,
            }
        })
        .collect()
}
