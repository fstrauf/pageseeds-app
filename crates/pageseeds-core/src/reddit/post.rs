/// Native Reddit comment posting via the Reddit OAuth2 REST API.
///
/// Flow:
///   1. Exchange the stored refresh_token for a short-lived access_token.
///   2. POST to /api/comment as the authenticated user.
///
/// No Python / PRAW required. Credentials are passed in by the caller
/// (typically resolved from ~/.config/automation/secrets.env via EnvResolver).
use serde::Deserialize;

use crate::error::{Error, Result};

const USER_AGENT: &str = "PageSeeds:pageseeds-app:v1.0 (desktop; contact pageseeds.com)";

// ─── API response shapes ─────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct CommentResponse {
    json: CommentJson,
}

#[derive(Debug, Deserialize)]
struct CommentJson {
    #[serde(default)]
    errors: Vec<serde_json::Value>,
    data: Option<CommentData>,
}

#[derive(Debug, Deserialize)]
struct CommentData {
    things: Vec<CommentThing>,
}

#[derive(Debug, Deserialize)]
struct CommentThing {
    data: CommentThingData,
}

#[derive(Debug, Deserialize)]
struct CommentThingData {
    id: String,
    #[serde(default)]
    permalink: Option<String>,
}

// ─── Public types ─────────────────────────────────────────────────────────────

pub struct CommentResult {
    pub comment_id: String,
    pub permalink: String,
}

// ─── Implementation ───────────────────────────────────────────────────────────

/// Exchange a Reddit refresh token for a short-lived access token.
pub(crate) async fn get_access_token(
    client: &reqwest::Client,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<String> {
    let resp: TokenResponse = client
        .post("https://www.reddit.com/api/v1/access_token")
        .basic_auth(client_id, Some(client_secret))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    Ok(resp.access_token)
}

/// Detect whether a Reddit error message is a rate-limit response and extract
/// the suggested wait time in seconds.
fn parse_rate_limit_seconds(error_msg: &str) -> Option<u64> {
    // Reddit returns messages like:
    // "Looks like you've been doing that a lot. Take a break for 9 minutes before trying again."
    if !error_msg.to_lowercase().contains("take a break")
        && !error_msg.to_lowercase().contains("doing that a lot")
    {
        return None;
    }

    // Try to extract "X minutes"
    let re = regex::Regex::new(r"(\d+)\s*minute").ok()?;
    if let Some(caps) = re.captures(error_msg) {
        if let Ok(mins) = caps[1].parse::<u64>() {
            return Some(mins * 60);
        }
    }

    // Try to extract "X seconds"
    let re_sec = regex::Regex::new(r"(\d+)\s*second").ok()?;
    if let Some(caps) = re_sec.captures(error_msg) {
        if let Ok(secs) = caps[1].parse::<u64>() {
            return Some(secs);
        }
    }

    // Default fallback: 10 minutes
    Some(600)
}

/// Build the API-level error message from a CommentResponse.
fn extract_comment_error(resp: &CommentResponse) -> String {
    resp.json
        .errors
        .iter()
        .filter_map(|e| e.as_array())
        .filter_map(|arr| arr.get(1).and_then(|v| v.as_str()).map(|s| s.to_string()))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Submit a reply to a Reddit post (t3) using the stored OAuth2 refresh token.
///
/// Retries aggressively on rate-limit errors, honouring Reddit's suggested
/// wait time. Will keep retrying until success or until ~30 minutes have
/// elapsed, to avoid hanging a queue runner forever.
///
/// Non-rate-limit errors (e.g. deleted post, auth failure) fail immediately.
///
/// `client_id`, `client_secret`, and `refresh_token` are the three Reddit API
/// credentials stored in ~/.config/automation/secrets.env.
pub async fn submit_comment(
    post_id: &str,
    text: &str,
    client_id: &str,
    client_secret: &str,
    refresh_token: &str,
) -> Result<CommentResult> {
    let client = reqwest::Client::builder().user_agent(USER_AGENT).build()?;

    let access_token = get_access_token(&client, client_id, client_secret, refresh_token).await?;

    // Reddit fullname for a submission is "t3_<id>"
    let thing_id = format!("t3_{}", post_id);

    let start = std::time::Instant::now();
    let mut attempt: u32 = 0;
    let mut last_err: Option<Error> = None;

    loop {
        if attempt > 0 {
            let mut wait_secs = last_err
                .as_ref()
                .and_then(|e| parse_rate_limit_seconds(&e.to_string()))
                .unwrap_or(60);

            // Reddit sometimes lies and says "0 seconds" — add a buffer so we
            // don't hammer the API and burn another attempt immediately.
            if wait_secs == 0 {
                wait_secs = 60;
                log::info!(
                    "[reddit_post_reply] Reddit said 0s wait — adding 60s buffer (post_id={})",
                    post_id
                );
            }

            // Cap any single wait at 15 minutes
            let capped = std::cmp::min(wait_secs, 15 * 60);

            log::info!(
                "[reddit_post_reply] retry attempt {} after {}s (post_id={})",
                attempt + 1,
                capped,
                post_id
            );
            tokio::time::sleep(tokio::time::Duration::from_secs(capped)).await;
        }

        // Hard stop after 30 minutes of retries so we don't block the queue
        // runner indefinitely on a permanently rate-limited account.
        if start.elapsed().as_secs() > 30 * 60 {
            log::warn!(
                "[reddit_post_reply] giving up after ~30 minutes of retries (post_id={})",
                post_id
            );
            break;
        }

        let response = match client
            .post("https://oauth.reddit.com/api/comment")
            .bearer_auth(&access_token)
            .form(&[
                ("thing_id", thing_id.as_str()),
                ("text", text),
                ("api_type", "json"),
            ])
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(Error::Other(format!("HTTP error: {}", e)));
                break;
            }
        };

        let status = response.status();

        // Handle HTTP 429 Too Many Requests
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let mut wait_secs = 60u64;
            if let Some(retry_after) = response.headers().get("retry-after") {
                if let Ok(sec_str) = retry_after.to_str() {
                    if let Ok(secs) = sec_str.parse::<u64>() {
                        wait_secs = secs;
                    }
                }
            }
            log::warn!(
                "[reddit_post_reply] 429 Too Many Requests (post_id={}) — waiting {}s",
                post_id,
                wait_secs
            );
            last_err = Some(Error::Other(format!(
                "Reddit API rate limited (429). Retry-After: {}s",
                wait_secs
            )));
            attempt += 1;
            continue;
        }

        // Parse JSON body (Reddit returns 200 with errors inside)
        let resp: CommentResponse = match response.json().await {
            Ok(r) => r,
            Err(e) => {
                last_err = Some(Error::Other(format!("JSON parse error: {}", e)));
                break;
            }
        };

        // Reddit returns API-level errors even on HTTP 200.
        if !resp.json.errors.is_empty() {
            let msg = extract_comment_error(&resp);
            let full_msg = if msg.is_empty() {
                format!("{:?}", resp.json.errors)
            } else {
                msg.clone()
            };

            // Check if this is a rate-limit error
            if parse_rate_limit_seconds(&full_msg).is_some() {
                log::warn!(
                    "[reddit_post_reply] rate limited: {} (post_id={})",
                    full_msg,
                    post_id
                );
                last_err = Some(Error::Other(format!(
                    "Reddit API error: {}",
                    full_msg
                )));
                attempt += 1;
                continue; // retry
            }

            // Non-rate-limit error — don't retry
            return Err(Error::Other(format!(
                "Reddit API error: {}",
                full_msg
            )));
        }

        let thing = resp
            .json
            .data
            .and_then(|d| d.things.into_iter().next())
            .ok_or_else(|| Error::Other("Reddit returned no comment data in response".to_string()))?;

        let permalink = thing
            .data
            .permalink
            .map(|p| {
                if p.starts_with("http") {
                    p
                } else {
                    format!("https://www.reddit.com{}", p)
                }
            })
            .unwrap_or_default();

        return Ok(CommentResult {
            comment_id: thing.data.id,
            permalink,
        });
    }

    // All retries exhausted
    Err(last_err.unwrap_or_else(|| Error::Other(
        "Reddit comment failed after retries".to_string()
    )))
}
