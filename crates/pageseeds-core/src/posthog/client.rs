//! PostHog Query API client (HogQL) for conversion tape collection.
//!
//! Deterministic only — no LLM. Fetches event counts grouped by day + `$pathname`.

use crate::posthog::models::PosthogPageDailyRow;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde_json::Value;
use std::time::Duration;

const DEFAULT_HOST: &str = "us.posthog.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const WINDOW_DAYS: u8 = 28;

/// Configuration for the PostHog Query API client.
#[derive(Debug, Clone)]
pub struct PosthogClientConfig {
    pub api_key: String,
    pub project_id: String,
    /// Host without scheme (e.g. `us.posthog.com` or `eu.posthog.com`).
    pub host: String,
    /// Override base URL for tests (e.g. `http://127.0.0.1:PORT`).
    pub base_url_override: Option<String>,
    pub window_days: u8,
}

impl PosthogClientConfig {
    pub fn new(api_key: String, project_id: String) -> Self {
        Self {
            api_key,
            project_id,
            host: DEFAULT_HOST.to_string(),
            base_url_override: None,
            window_days: WINDOW_DAYS,
        }
    }

    /// Host from env-style value: strip scheme if present.
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = normalize_host(&host.into());
        self
    }

    pub fn with_base_url(mut self, base: impl Into<String>) -> Self {
        self.base_url_override = Some(base.into());
        self
    }
}

/// Strip `https://` / `http://` and trailing slash from host-like strings.
pub fn normalize_host(raw: &str) -> String {
    let s = raw
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/');
    if s.is_empty() {
        DEFAULT_HOST.to_string()
    } else {
        s.to_string()
    }
}

/// Lightweight PostHog Query API client.
#[derive(Debug, Clone)]
pub struct PosthogClient {
    config: PosthogClientConfig,
    http: reqwest::Client,
}

impl PosthogClient {
    pub fn new(config: PosthogClientConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    fn query_url(&self) -> String {
        if let Some(base) = &self.config.base_url_override {
            let base = base.trim_end_matches('/');
            return format!(
                "{}/api/projects/{}/query/",
                base, self.config.project_id
            );
        }
        format!(
            "https://{}/api/projects/{}/query/",
            self.config.host, self.config.project_id
        )
    }

    /// Fetch last-N-day event counts by day + `$pathname` for one event name.
    pub async fn fetch_event_daily(
        &self,
        event: &str,
    ) -> Result<Vec<PosthogPageDailyRow>, String> {
        let days = self.config.window_days.max(1);
        // HogQL: group by day + pathname. Event name is escaped as a SQL string literal.
        let escaped_event = event.replace('\'', "''");
        let hogql = format!(
            "SELECT toDate(timestamp) AS day, properties.$pathname AS page, count() AS cnt \
             FROM events \
             WHERE event = '{escaped_event}' \
               AND timestamp >= now() - INTERVAL {days} DAY \
               AND properties.$pathname IS NOT NULL \
             GROUP BY day, page \
             ORDER BY day"
        );

        let body = serde_json::json!({
            "query": {
                "kind": "HogQLQuery",
                "query": hogql,
            }
        });

        let response = self
            .http
            .post(self.query_url())
            .header(
                AUTHORIZATION,
                format!("Bearer {}", self.config.api_key),
            )
            .header(CONTENT_TYPE, "application/json")
            .timeout(REQUEST_TIMEOUT)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("PostHog API request failed for event '{event}': {e}"))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<could not read body>".to_string());
            return Err(format!(
                "PostHog API returned {status} for event '{event}': {body}"
            ));
        }

        let payload: Value = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse PostHog response for '{event}': {e}"))?;

        parse_query_results(&payload, event)
    }

    /// Fetch all configured events and concatenate rows.
    pub async fn fetch_all_events(
        &self,
        events: &[String],
    ) -> Result<Vec<PosthogPageDailyRow>, String> {
        let mut all = Vec::new();
        for event in events {
            let rows = self.fetch_event_daily(event).await?;
            all.extend(rows);
        }
        Ok(all)
    }
}

/// Parse `{ "results": [[day, page, count], ...] }` and tolerate object rows.
pub fn parse_query_results(payload: &Value, event: &str) -> Result<Vec<PosthogPageDailyRow>, String> {
    let results = payload
        .get("results")
        .or_else(|| payload.pointer("/query_status/results"))
        .ok_or_else(|| "PostHog response missing 'results' array".to_string())?;

    let arr = results
        .as_array()
        .ok_or_else(|| "PostHog 'results' is not an array".to_string())?;

    let mut rows = Vec::with_capacity(arr.len());
    for item in arr {
        if let Some(row) = parse_result_row(item, event) {
            rows.push(row);
        }
    }
    Ok(rows)
}

fn parse_result_row(item: &Value, event: &str) -> Option<PosthogPageDailyRow> {
    if let Some(arr) = item.as_array() {
        // [day, page, count]
        if arr.len() < 3 {
            return None;
        }
        let date = value_as_date_string(&arr[0])?;
        let page = value_as_string(&arr[1])?;
        if page.is_empty() {
            return None;
        }
        let count = value_as_f64(&arr[2])?;
        return Some(PosthogPageDailyRow {
            page,
            event: event.to_string(),
            date,
            count,
        });
    }

    if let Some(obj) = item.as_object() {
        let date = obj
            .get("day")
            .or_else(|| obj.get("date"))
            .and_then(value_as_date_string)?;
        let page = obj
            .get("page")
            .or_else(|| obj.get("pathname"))
            .and_then(value_as_string)?;
        if page.is_empty() {
            return None;
        }
        let count = obj
            .get("cnt")
            .or_else(|| obj.get("count"))
            .and_then(value_as_f64)
            .unwrap_or(0.0);
        return Some(PosthogPageDailyRow {
            page,
            event: event.to_string(),
            date,
            count,
        });
    }

    None
}

fn value_as_string(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn value_as_date_string(v: &Value) -> Option<String> {
    let s = value_as_string(v)?;
    // HogQL may return "2026-07-01" or full timestamps — keep date prefix.
    if s.len() >= 10 {
        Some(s[..10].to_string())
    } else {
        Some(s)
    }
}

fn value_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_host_strips_scheme() {
        assert_eq!(normalize_host("https://eu.posthog.com/"), "eu.posthog.com");
        assert_eq!(normalize_host("us.posthog.com"), "us.posthog.com");
        assert_eq!(normalize_host(""), DEFAULT_HOST);
    }

    #[test]
    fn parse_array_rows() {
        let payload = serde_json::json!({
            "results": [
                ["2026-07-01", "/blog/foo", 3],
                ["2026-07-02", "/pricing", 1.5],
            ]
        });
        let rows = parse_query_results(&payload, "signup_started").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].page, "/blog/foo");
        assert_eq!(rows[0].event, "signup_started");
        assert_eq!(rows[0].date, "2026-07-01");
        assert_eq!(rows[0].count, 3.0);
        assert_eq!(rows[1].count, 1.5);
    }

    #[test]
    fn parse_object_rows() {
        let payload = serde_json::json!({
            "results": [
                {"day": "2026-07-01T00:00:00Z", "page": "/blog/bar", "cnt": 7}
            ]
        });
        let rows = parse_query_results(&payload, "cta_clicked").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].date, "2026-07-01");
        assert_eq!(rows[0].page, "/blog/bar");
        assert_eq!(rows[0].count, 7.0);
        assert_eq!(rows[0].event, "cta_clicked");
    }

    #[test]
    fn parse_missing_results_errors() {
        let payload = serde_json::json!({"ok": true});
        assert!(parse_query_results(&payload, "x").is_err());
    }
}
