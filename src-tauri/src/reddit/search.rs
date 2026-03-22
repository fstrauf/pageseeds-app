use chrono::{DateTime, Utc};
use serde::Deserialize;

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

pub async fn search_submissions(
    query: &str,
    subreddit: &str,
    limit: i32,
    sort: &str,
    time_filter: &str,
) -> Result<Vec<SubmissionSummary>> {
    let base = if subreddit.is_empty() {
        "https://www.reddit.com/search.json".to_string()
    } else {
        format!("https://www.reddit.com/r/{}/search.json", subreddit)
    };

    let client = reqwest::Client::builder()
        .user_agent("PageSeeds/1.0 (desktop app; contact pageseeds.com)")
        .build()?;

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

    let resp: SearchResponse = client
        .get(&base)
        .query(&query_params)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let now = Utc::now();

    let summaries = resp
        .data
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
        .collect();

    Ok(summaries)
}
