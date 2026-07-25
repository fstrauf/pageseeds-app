use crate::clarity::models::{ClarityDimension, ClarityMetricBlock};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use std::collections::HashMap;
use std::time::Duration;

const CLARITY_EXPORT_BASE: &str = "https://www.clarity.ms/export-data/api/v1/project-live-insights";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// Small set of dimensioned requests that fit within the 10-request daily budget.
/// Each tuple is (dimension_set_label, dimensions).
pub const DEFAULT_DIMENSION_SETS: &[(&str, &[ClarityDimension])] = &[
    ("url", &[ClarityDimension::URL]),
    ("url+device", &[ClarityDimension::URL, ClarityDimension::Device]),
    ("url+source", &[ClarityDimension::URL, ClarityDimension::Source]),
    ("os+browser", &[ClarityDimension::OS, ClarityDimension::Browser]),
    ("country", &[ClarityDimension::CountryRegion]),
];

/// Configuration needed to call the Clarity Export API.
#[derive(Debug, Clone)]
pub struct ClarityClientConfig {
    pub api_token: String,
    pub project_id: String,
    pub num_of_days: u8,
}

impl ClarityClientConfig {
    pub fn new(api_token: String, project_id: String) -> Self {
        Self {
            api_token,
            project_id,
            num_of_days: 3,
        }
    }
}

/// Lightweight Clarity Export API client.
#[derive(Debug, Clone)]
pub struct ClarityClient {
    config: ClarityClientConfig,
    http: reqwest::Client,
}

impl ClarityClient {
    pub fn new(config: ClarityClientConfig) -> Self {
        Self {
            config,
            http: reqwest::Client::new(),
        }
    }

    /// Fetch live insights for a single dimension set.
    pub async fn fetch_dimension_set(
        &self,
        label: &str,
        dimensions: &[ClarityDimension],
    ) -> Result<Vec<ClarityMetricBlock>, String> {
        let mut params = HashMap::new();
        params.insert("numOfDays".to_string(), self.config.num_of_days.to_string());
        for (i, dim) in dimensions.iter().enumerate() {
            let key = format!("dimension{}", i + 1);
            params.insert(key, dim.as_api_name().to_string());
        }

        let response = self
            .http
            .get(CLARITY_EXPORT_BASE)
            .query(&params)
            .header(AUTHORIZATION, format!("Bearer {}", self.config.api_token))
            .header(CONTENT_TYPE, "application/json")
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await
            .map_err(|e| format!("Clarity API request failed for {}: {}", label, e))?;

        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<could not read body>".to_string());
            return Err(format!(
                "Clarity API returned {} for {}: {}",
                status, label, body
            ));
        }

        let blocks: Vec<ClarityMetricBlock> = response
            .json()
            .await
            .map_err(|e| format!("Failed to parse Clarity response for {}: {}", label, e))?;

        Ok(blocks)
    }

    /// Fetch all default dimension sets and return them with their labels.
    pub async fn fetch_all(
        &self,
    ) -> Result<Vec<(&'static str, Vec<ClarityMetricBlock>)>, String> {
        let mut results = Vec::with_capacity(DEFAULT_DIMENSION_SETS.len());
        for (label, dims) in DEFAULT_DIMENSION_SETS {
            let blocks = self.fetch_dimension_set(label, dims).await?;
            results.push((*label, blocks));
        }
        Ok(results)
    }

    /// Quick connection test using the URL dimension and 1 day.
    pub async fn test_connection(&self) -> Result<Vec<ClarityMetricBlock>, String> {
        let test_config = ClarityClientConfig {
            api_token: self.config.api_token.clone(),
            project_id: self.config.project_id.clone(),
            num_of_days: 1,
        };
        let client = ClarityClient::new(test_config);
        client.fetch_dimension_set("url", &[ClarityDimension::URL]).await
    }
}

/// Build a direct link to the Clarity recordings dashboard filtered by URL.
pub fn clarity_dashboard_url(project_id: &str, url: &str) -> String {
    let encoded = urlencoding::encode(url);
    format!(
        "https://clarity.microsoft.com/projects/view/{}/recordings?URL={}",
        project_id, encoded
    )
}
