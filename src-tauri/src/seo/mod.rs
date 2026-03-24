pub mod backlinks;
pub mod keywords;
pub mod traffic;

use crate::error::{Error, Result};

const CAPSOLVER_CREATE_URL: &str = "https://api.capsolver.com/createTask";
const CAPSOLVER_RESULT_URL: &str = "https://api.capsolver.com/getTaskResult";

/// Ahrefs Cloudflare Turnstile site key.
const AHREFS_TURNSTILE_KEY: &str = "0x4AAAAAAAAzi9ITzSN9xKMi";

fn capsolver_create_url() -> String {
    std::env::var("PAGESEEDS_CAPSOLVER_CREATE_URL")
        .unwrap_or_else(|_| CAPSOLVER_CREATE_URL.to_string())
}

fn capsolver_result_url() -> String {
    std::env::var("PAGESEEDS_CAPSOLVER_RESULT_URL")
        .unwrap_or_else(|_| CAPSOLVER_RESULT_URL.to_string())
}

/// Solve a Cloudflare Turnstile challenge for an Ahrefs free tool page using CapSolver.
/// `site_url` must be the exact page URL (e.g. keyword-generator with query params).
/// Returns the Turnstile token passed as "captcha" to Ahrefs endpoints.
pub async fn solve_ahrefs_captcha(api_key: &str, site_url: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36")
        .build()
        .map_err(Error::Http)?;

    // Create task
    let create_body = serde_json::json!({
        "clientKey": api_key,
        "task": {
            "type": "AntiTurnstileTaskProxyLess",
            "websiteURL": site_url,
            "websiteKey": AHREFS_TURNSTILE_KEY,
            "metadata": {"action": ""}
        }
    });

    let create_resp: serde_json::Value = client
        .post(capsolver_create_url())
        .json(&create_body)
        .send()
        .await?
        .json()
        .await?;

    let error_id = create_resp["errorId"].as_i64().unwrap_or(1);
    if error_id != 0 {
        let desc = create_resp["errorDescription"]
            .as_str()
            .unwrap_or("unknown error");
        return Err(Error::Other(format!("CapSolver create task error: {}", desc)));
    }

    let task_id = create_resp["taskId"]
        .as_str()
        .ok_or_else(|| Error::Other("Missing taskId in CapSolver response".to_string()))?
        .to_string();

    // Poll for result — max 60 seconds (30 × 2 s)
    for _ in 0..30 {
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        let result_resp: serde_json::Value = client
            .post(capsolver_result_url())
            .json(&serde_json::json!({
                "clientKey": api_key,
                "taskId": task_id
            }))
            .send()
            .await?
            .json()
            .await?;

        if result_resp["errorId"].as_i64().unwrap_or(0) != 0 {
            let desc = result_resp["errorDescription"]
                .as_str()
                .unwrap_or("unknown error");
            return Err(Error::Other(format!("CapSolver task error: {}", desc)));
        }

        let status = result_resp["status"].as_str().unwrap_or("processing");
        if status == "ready" {
            let token = result_resp["solution"]["token"]
                .as_str()
                .ok_or_else(|| {
                    Error::Other("Missing token in CapSolver solution".to_string())
                })?
                .to_string();
            return Ok(token);
        }
    }

    Err(Error::Other(
        "CapSolver task timed out after 60 seconds".to_string(),
    ))
}
