use reqwest::Client;
use serde_json::Value;

use crate::error::{Error, Result};

pub struct GscClient {
    access_token: String,
    client: Client,
}

impl GscClient {
    pub fn new(access_token: impl Into<String>) -> Self {
        Self {
            access_token: access_token.into(),
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// POST to the Search Analytics API for a given site URL.
    pub async fn search_analytics_query(&self, site_url: &str, body: &Value) -> Result<Value> {
        let encoded = urlencoding::encode(site_url).into_owned();
        let url = format!(
            "https://searchconsole.googleapis.com/webmasters/v3/sites/{}/searchAnalytics/query",
            encoded
        );
        self.post(&url, body).await
    }

    /// POST to the URL Inspection API.
    pub async fn url_inspection_inspect(&self, body: &Value) -> Result<Value> {
        const URL: &str = "https://searchconsole.googleapis.com/v1/urlInspection/index:inspect";
        const MAX_ATTEMPTS: usize = 3;

        for attempt in 1..=MAX_ATTEMPTS {
            let resp = self
                .client
                .post(URL)
                .bearer_auth(&self.access_token)
                .json(body)
                .send()
                .await;

            match resp {
                Ok(resp) => {
                    if resp.status().is_success() {
                        return Ok(resp.json().await?);
                    }

                    let status = resp.status();
                    let text = resp.text().await.unwrap_or_default();
                    let retryable = status.as_u16() == 429 || status.is_server_error();

                    if retryable && attempt < MAX_ATTEMPTS {
                        let backoff_ms = (attempt as u64) * 1000;
                        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                        continue;
                    }

                    return Err(Error::Other(format!("GSC API error {}: {}", status, text)));
                }
                Err(e) => {
                    if attempt < MAX_ATTEMPTS {
                        let backoff_ms = (attempt as u64) * 1000;
                        tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                        continue;
                    }
                    return Err(Error::Other(format!("GSC request failed: {}", e)));
                }
            }
        }

        Err(Error::Other("GSC inspection request failed after retries".to_string()))
    }

    async fn post(&self, url: &str, body: &Value) -> Result<Value> {
        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.access_token)
            .json(body)
            .send()
            .await?;

        if resp.status().is_success() {
            Ok(resp.json().await?)
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            Err(Error::Other(format!("GSC API error {}: {}", status, text)))
        }
    }
}
