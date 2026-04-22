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

/// Submit a reply to a Reddit post (t3) using the stored OAuth2 refresh token.
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
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()?;

    let access_token =
        get_access_token(&client, client_id, client_secret, refresh_token).await?;

    // Reddit fullname for a submission is "t3_<id>"
    let thing_id = format!("t3_{}", post_id);

    let resp: CommentResponse = client
        .post("https://oauth.reddit.com/api/comment")
        .bearer_auth(&access_token)
        .form(&[
            ("thing_id", thing_id.as_str()),
            ("text", text),
            ("api_type", "json"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    // Reddit returns API-level errors even on HTTP 200.
    if !resp.json.errors.is_empty() {
        let msg = resp
            .json
            .errors
            .iter()
            .filter_map(|e| e.as_array())
            .filter_map(|arr| arr.get(1).and_then(|v| v.as_str()).map(|s| s.to_string()))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::Other(format!("Reddit API error: {}", if msg.is_empty() { format!("{:?}", resp.json.errors) } else { msg })));
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

    Ok(CommentResult {
        comment_id: thing.data.id,
        permalink,
    })
}
