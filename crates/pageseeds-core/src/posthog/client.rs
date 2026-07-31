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
/// HogQL default row cap is 100; raise so multi-day × multi-path windows stay complete.
const QUERY_ROW_LIMIT: u32 = 50_000;

/// Configuration for the PostHog Query API client.
#[derive(Debug, Clone)]
pub struct PosthogClientConfig {
    pub api_key: String,
    pub project_id: String,
    /// API host without scheme (e.g. `us.posthog.com` or `eu.posthog.com`).
    pub host: String,
    /// Override base URL for tests (e.g. `http://127.0.0.1:PORT`).
    pub base_url_override: Option<String>,
    pub window_days: u8,
    /// Optional `$host` property filter (site hostname, not API host).
    /// When set (from project `site_base_url()`), HogQL scopes events to that host.
    pub filter_host: Option<String>,
}

impl PosthogClientConfig {
    pub fn new(api_key: String, project_id: String) -> Self {
        Self {
            api_key,
            project_id,
            host: DEFAULT_HOST.to_string(),
            base_url_override: None,
            window_days: WINDOW_DAYS,
            filter_host: None,
        }
    }

    /// API host from env-style value: strip scheme if present.
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = normalize_host(&host.into());
        self
    }

    pub fn with_base_url(mut self, base: impl Into<String>) -> Self {
        self.base_url_override = Some(base.into());
        self
    }

    /// Scope events to `properties.$host` (hostname only, e.g. `example.com`).
    pub fn with_filter_host(mut self, host: impl Into<String>) -> Self {
        let h = host.into();
        let trimmed = h.trim();
        self.filter_host = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        };
        self
    }
}

/// Extract a hostname suitable for PostHog `properties.$host` from a site base URL.
///
/// `https://example.com/blog` → `example.com`; empty / unusable → `None`.
pub fn filter_host_from_site_base_url(base: &str) -> Option<String> {
    let trimmed = base.trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_scheme = trimmed
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let host = without_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .split('@')
        .next_back()
        .unwrap_or("")
        .trim();
    // Drop optional :port
    let host = host.split(':').next().unwrap_or(host).trim();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
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

    /// Build the HogQL for one event (public for tests; used by [`Self::fetch_event_daily`]).
    pub fn build_event_daily_hogql(&self, event: &str) -> String {
        let days = self.config.window_days.max(1);
        // HogQL: group by day + pathname. Event name is escaped as a SQL string literal.
        let escaped_event = event.replace('\'', "''");
        let host_clause = self
            .config
            .filter_host
            .as_deref()
            .map(|h| {
                let escaped_host = h.replace('\'', "''");
                format!(" AND properties.$host = '{escaped_host}'")
            })
            .unwrap_or_default();
        format!(
            "SELECT toDate(timestamp) AS day, properties.$pathname AS page, count() AS cnt \
             FROM events \
             WHERE event = '{escaped_event}' \
               AND timestamp >= now() - INTERVAL {days} DAY \
               AND properties.$pathname IS NOT NULL{host_clause} \
             GROUP BY day, page \
             ORDER BY day ASC \
             LIMIT {QUERY_ROW_LIMIT}"
        )
    }

    /// Configured collection window (days). Single source for artifact meta.
    pub fn window_days(&self) -> u8 {
        self.config.window_days.max(1)
    }

    /// Fetch last-N-day event counts by day + `$pathname` for one event name.
    pub async fn fetch_event_daily(
        &self,
        event: &str,
    ) -> Result<Vec<PosthogPageDailyRow>, String> {
        let hogql = self.build_event_daily_hogql(event);

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

        if payload
            .get("hasMore")
            .or_else(|| payload.get("has_more"))
            .and_then(|v| v.as_bool())
            == Some(true)
        {
            log::warn!(
                "[posthog] query for event '{event}' reported hasMore=true after LIMIT {QUERY_ROW_LIMIT}; \
                 conversion tape may be truncated"
            );
        }

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
    // Use get(..10) so multi-byte/short edge cases never panic on slice.
    match s.get(..10) {
        Some(prefix) if s.len() >= 10 => Some(prefix.to_string()),
        _ => Some(s),
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

    #[test]
    fn filter_host_from_site_base_url_strips_scheme_and_path() {
        assert_eq!(
            filter_host_from_site_base_url("https://example.com/blog"),
            Some("example.com".into())
        );
        assert_eq!(
            filter_host_from_site_base_url("http://www.example.com"),
            Some("www.example.com".into())
        );
        assert_eq!(filter_host_from_site_base_url(""), None);
        assert_eq!(filter_host_from_site_base_url("   "), None);
    }

    #[test]
    fn hogql_includes_limit_and_optional_host_filter() {
        let cfg = PosthogClientConfig::new("k".into(), "1".into())
            .with_filter_host("example.com");
        let client = PosthogClient::new(cfg);
        let q = client.build_event_daily_hogql("signup_started");
        assert!(q.contains("LIMIT 50000"), "query must raise default 100-row cap: {q}");
        assert!(
            q.contains("properties.$host = 'example.com'"),
            "host filter missing: {q}"
        );
        assert!(q.contains("ORDER BY day ASC"), "stable ASC order: {q}");

        let no_host = PosthogClient::new(PosthogClientConfig::new("k".into(), "1".into()));
        let q2 = no_host.build_event_daily_hogql("signup_started");
        assert!(q2.contains("LIMIT 50000"));
        assert!(!q2.contains("properties.$host"));
    }

    #[test]
    fn parse_more_than_100_rows() {
        let mut results = Vec::new();
        for i in 0..150 {
            results.push(serde_json::json!([
                format!("2026-07-{:02}", (i % 28) + 1),
                format!("/blog/page-{i}"),
                i + 1
            ]));
        }
        let payload = serde_json::json!({ "results": results });
        let rows = parse_query_results(&payload, "signup_started").unwrap();
        assert_eq!(rows.len(), 150);
        assert_eq!(rows[149].count, 150.0);
    }

    #[test]
    fn value_as_date_string_short_and_long() {
        assert_eq!(
            value_as_date_string(&Value::String("2026-07-01T12:00:00Z".into())).as_deref(),
            Some("2026-07-01")
        );
        assert_eq!(
            value_as_date_string(&Value::String("short".into())).as_deref(),
            Some("short")
        );
    }

    #[tokio::test]
    async fn fetch_event_daily_mocked_endpoint_sends_limit_and_host_and_parses_many_rows() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, Request, ResponseTemplate};

        let server = MockServer::start().await;
        let captured: std::sync::Arc<std::sync::Mutex<Option<String>>> =
            std::sync::Arc::new(std::sync::Mutex::new(None));
        let captured_c = captured.clone();

        let mut results = Vec::new();
        for i in 0..120 {
            results.push(serde_json::json!([
                format!("2026-07-{:02}", (i % 28) + 1),
                format!("/blog/p{i}"),
                1
            ]));
        }

        Mock::given(method("POST"))
            .and(path("/api/projects/99/query/"))
            .respond_with(move |req: &Request| {
                if let Ok(body) = serde_json::from_slice::<Value>(&req.body) {
                    if let Some(q) = body.pointer("/query/query").and_then(|v| v.as_str()) {
                        *captured_c.lock().unwrap() = Some(q.to_string());
                    }
                }
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "results": results,
                }))
            })
            .mount(&server)
            .await;

        let client = PosthogClient::new(
            PosthogClientConfig::new("test-key".into(), "99".into())
                .with_base_url(server.uri())
                .with_filter_host("example.com"),
        );
        let rows = client
            .fetch_event_daily("signup_started")
            .await
            .expect("mocked fetch");
        assert!(rows.len() > 100, "must parse >100 rows, got {}", rows.len());

        let q = captured.lock().unwrap().clone().expect("captured hogql");
        assert!(q.contains("LIMIT"), "LIMIT missing from request HogQL: {q}");
        assert!(
            q.contains("properties.$host = 'example.com'"),
            "host filter missing from request HogQL: {q}"
        );
    }
}
