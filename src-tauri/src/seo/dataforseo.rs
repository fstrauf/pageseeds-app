use crate::error::{Error, Result};
use crate::seo::intent::{classify_batch_by_pattern, IntentClassification};
use crate::seo::keywords::{KeywordDifficultyResult, KeywordIdea, KeywordIdeasResult, SerpEntry};
use crate::seo::provider::SeoDataProvider;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;

const DATAFORSEO_BASE_URL: &str = "https://api.dataforseo.com";

/// DataForSEO SEO data provider implementation.
pub struct DataForSeoProvider {
    login: String,
    password: String,
    client: reqwest::Client,
}

impl DataForSeoProvider {
    pub fn new(login: String, password: String) -> Self {
        Self {
            login,
            password,
            client: reqwest::Client::new(),
        }
    }

    /// Build Basic auth header value.
    fn auth_header(&self) -> String {
        let credentials = format!("{}:{}", self.login, self.password);
        format!(
            "Basic {}",
            base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                credentials.as_bytes()
            )
        )
    }

    /// Build full API URL.
    fn api_url(&self, path: &str) -> String {
        format!("{}{}", DATAFORSEO_BASE_URL, path)
    }

    /// Parse a DataForSEO keyword ideas response into our internal format.
    ///
    /// Supports both `keyword_suggestions` (items flat) and `related_keywords`
    /// (items nested under `keyword_data`) response shapes.
    fn parse_keyword_ideas_response(
        &self,
        data: &Value,
        seed_keyword: &str,
        country: &str,
        search_engine: &str,
    ) -> Result<KeywordIdeasResult> {
        let tasks = data
            .get("tasks")
            .and_then(|t| t.as_array())
            .ok_or_else(|| {
                Error::Other("Invalid DataForSEO response: missing tasks".to_string())
            })?;

        let mut ideas: Vec<KeywordIdea> = vec![];
        let mut question_ideas: Vec<KeywordIdea> = vec![];

        for task in tasks {
            let empty_vec = vec![];
            let result = task
                .get("result")
                .and_then(|r| r.as_array())
                .unwrap_or(&empty_vec);

            for item in result {
                let empty_keywords = vec![];
                let keywords = item
                    .get("items")
                    .and_then(|k| k.as_array())
                    .unwrap_or(&empty_keywords);

                for kw in keywords {
                    // related_keywords nests data under `keyword_data`; keyword_suggestions is flat.
                    let data_node = kw.get("keyword_data").unwrap_or(kw);

                    let keyword_text = data_node
                        .get("keyword")
                        .and_then(|k| k.as_str())
                        .unwrap_or("")
                        .to_string();

                    if keyword_text.is_empty() {
                        continue;
                    }

                    let kw_lower = keyword_text.to_lowercase();
                    let is_question = kw_lower.starts_with("how ")
                        || kw_lower.starts_with("what ")
                        || kw_lower.starts_with("why ")
                        || kw_lower.starts_with("when ")
                        || kw_lower.starts_with("where ")
                        || kw_lower.starts_with("who ")
                        || kw_lower.starts_with("can ")
                        || kw_lower.starts_with("is ")
                        || kw_lower.starts_with("are ")
                        || kw_lower.starts_with("does ");

                    // keyword_info sub-object contains volume, cpc, competition
                    let keyword_info = data_node.get("keyword_info");
                    let search_volume = keyword_info
                        .and_then(|ki| ki.get("search_volume"))
                        .and_then(|v| v.as_i64());

                    let cpc = keyword_info
                        .and_then(|ki| ki.get("cpc"))
                        .and_then(|v| v.as_f64());

                    let competition = keyword_info
                        .and_then(|ki| ki.get("competition"))
                        .and_then(|v| v.as_f64());

                    // keyword_properties.keyword_difficulty is the 0-100 KD score
                    let kd = data_node
                        .get("keyword_properties")
                        .and_then(|kp| kp.get("keyword_difficulty"))
                        .and_then(|v| v.as_f64());

                    let difficulty_label = kd.map(|d| {
                        if d < 15.0 {
                            "Easy".to_string()
                        } else if d < 30.0 {
                            "Low".to_string()
                        } else if d < 50.0 {
                            "Medium".to_string()
                        } else {
                            "Hard".to_string()
                        }
                    });

                    // search_intent_info.main_intent
                    let intent = data_node
                        .get("search_intent_info")
                        .and_then(|si| si.get("main_intent"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let idea = KeywordIdea {
                        keyword: keyword_text,
                        idea_type: if is_question {
                            "question".to_string()
                        } else {
                            "regular".to_string()
                        },
                        difficulty: difficulty_label,
                        kd,
                        intent,
                        volume: search_volume.map(|v| format!("{}", v)),
                        volume_exact: search_volume,
                        cpc,
                        competition,
                        country: Some(country.to_string()),
                    };

                    if is_question {
                        question_ideas.push(idea);
                    } else {
                        ideas.push(idea);
                    }
                }
            }
        }

        Ok(KeywordIdeasResult {
            keyword: seed_keyword.to_string(),
            country: country.to_string(),
            search_engine: search_engine.to_string(),
            ideas,
            question_ideas,
        })
    }

    /// Parse a DataForSEO keyword difficulty response into our internal format.
    fn parse_keyword_difficulty_response(
        &self,
        data: &Value,
        keyword: &str,
    ) -> Result<KeywordDifficultyResult> {
        let tasks = data
            .get("tasks")
            .and_then(|t| t.as_array())
            .ok_or_else(|| {
                Error::Other("Invalid DataForSEO response: missing tasks".to_string())
            })?;

        let mut serp: Vec<SerpEntry> = vec![];
        let mut difficulty: Option<f64> = None;
        let mut last_update = String::new();

        for task in tasks {
            let empty_result = vec![];
            let result = task
                .get("result")
                .and_then(|r| r.as_array())
                .unwrap_or(&empty_result);

            for item in result {
                // Extract difficulty from competition_index
                if let Some(comp_idx) = item.get("competition_index").and_then(|v| v.as_f64()) {
                    difficulty = Some(comp_idx);
                }

                // Parse SERP results
                if let Some(items) = item.get("items").and_then(|i| i.as_array()) {
                    for (idx, serp_item) in items.iter().enumerate() {
                        let url = serp_item
                            .get("url")
                            .and_then(|u| u.as_str())
                            .unwrap_or("")
                            .to_string();

                        let domain = serp_item
                            .get("domain")
                            .and_then(|d| d.as_str())
                            .unwrap_or("")
                            .to_string();

                        let title = serp_item
                            .get("title")
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .to_string();

                        let traffic = serp_item
                            .get("etv") // estimated traffic value
                            .and_then(|v| v.as_f64());

                        let top_volume = serp_item
                            .get("keyword_data")
                            .and_then(|kd| kd.get("search_volume"))
                            .and_then(|v| v.as_f64());

                        if !url.is_empty() {
                            serp.push(SerpEntry {
                                title,
                                url,
                                domain,
                                position: (idx + 1) as i64,
                                traffic,
                                top_volume,
                            });
                        }
                    }
                }

                // Get check date as last_update
                if let Some(check_date) = item.get("check_date").and_then(|v| v.as_str()) {
                    last_update = check_date.to_string();
                }
            }
        }

        Ok(KeywordDifficultyResult {
            keyword: keyword.to_string(),
            difficulty,
            shortage: None, // DataForSEO doesn't provide this
            last_update,
            serp,
        })
    }

    /// Call the `related_keywords` endpoint (semantic discovery via Google "searches related to").
    async fn fetch_related_keywords(
        &self,
        keyword: &str,
        location_code: &str,
    ) -> Result<KeywordIdeasResult> {
        let payload = serde_json::json!([{
            "keyword": keyword,
            "location_code": location_code.parse::<i64>().unwrap_or(2840),
            "language_code": "en",
            "include_seed_keyword": true,
            "ignore_synonyms": true,
            "depth": 2,
            "limit": 100,
            "filters": [
                ["keyword_data.keyword_info.search_volume", ">", 50],
                "and",
                ["keyword_data.keyword_properties.keyword_difficulty", "<=", 30],
                "and",
                ["keyword_data.search_intent_info.main_intent", "<>", "navigational"],
                "and",
                ["keyword_data.keyword", "not_like", "%near me%"]
            ],
            "order_by": ["keyword_data.keyword_info.search_volume,desc"]
        }]);

        let data = self
            .post_dataforseo("/v3/dataforseo_labs/google/related_keywords/live", &payload)
            .await?;
        self.parse_keyword_ideas_response(&data, keyword, "us", "google")
    }

    /// Call the `keyword_suggestions` endpoint (substring matching against keyword database).
    async fn fetch_keyword_suggestions(
        &self,
        keyword: &str,
        location_code: &str,
    ) -> Result<KeywordIdeasResult> {
        let payload = serde_json::json!([{
            "keyword": keyword,
            "location_code": location_code.parse::<i64>().unwrap_or(2840),
            "language_code": "en",
            "include_seed_keyword": true,
            "ignore_synonyms": true,
            "depth": 3,
            "limit": 100,
            "filters": [
                ["keyword_info.search_volume", ">", 50],
                "and",
                ["keyword_properties.keyword_difficulty", "<=", 30],
                "and",
                ["search_intent_info.main_intent", "<>", "navigational"],
                "and",
                ["keyword", "not_like", "%near me%"]
            ],
            "order_by": ["keyword_info.search_volume,desc"]
        }]);

        let data = self
            .post_dataforseo(
                "/v3/dataforseo_labs/google/keyword_suggestions/live",
                &payload,
            )
            .await?;
        self.parse_keyword_ideas_response(&data, keyword, "us", "google")
    }

    /// Shared POST + error handling for DataForSEO Labs endpoints.
    async fn post_dataforseo(&self, path: &str, payload: &Value) -> Result<Value> {
        let resp = self
            .client
            .post(self.api_url(path))
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .json(payload)
            .send()
            .await
            .map_err(Error::Http)?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Other(format!(
                "DataForSEO API returned status {}: {}",
                status, body
            )));
        }

        let data: Value = resp.json().await.map_err(Error::Http)?;

        if let Some(status_code) = data.get("status_code").and_then(|v| v.as_i64()) {
            if status_code != 20000 {
                let status_msg = data
                    .get("status_message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error");
                return Err(Error::Other(format!(
                    "DataForSEO API error {}: {}",
                    status_code, status_msg
                )));
            }
        }

        Ok(data)
    }

    /// Build an empty result for fallback when one endpoint fails.
    fn empty_result(keyword: &str, country: &str, search_engine: &str) -> KeywordIdeasResult {
        KeywordIdeasResult {
            keyword: keyword.to_string(),
            country: country.to_string(),
            search_engine: search_engine.to_string(),
            ideas: vec![],
            question_ideas: vec![],
        }
    }

    /// Merge two keyword idea results, deduplicating by keyword (case-insensitive).
    /// When a keyword appears in both, keeps the one with higher exact volume.
    fn merge_results(
        keyword: &str,
        country: &str,
        search_engine: &str,
        a: KeywordIdeasResult,
        b: KeywordIdeasResult,
    ) -> KeywordIdeasResult {
        let mut seen: HashMap<String, KeywordIdea> = HashMap::new();

        for idea in a.ideas.into_iter().chain(a.question_ideas.into_iter()) {
            seen.insert(idea.keyword.to_lowercase(), idea);
        }

        for idea in b.ideas.into_iter().chain(b.question_ideas.into_iter()) {
            let key = idea.keyword.to_lowercase();
            if let Some(existing) = seen.get(&key) {
                let existing_vol = existing.volume_exact.unwrap_or(0);
                let new_vol = idea.volume_exact.unwrap_or(0);
                if new_vol > existing_vol {
                    seen.insert(key, idea);
                }
            } else {
                seen.insert(key, idea);
            }
        }

        let mut ideas = Vec::new();
        let mut question_ideas = Vec::new();
        for (_, idea) in seen {
            if idea.idea_type == "question" {
                question_ideas.push(idea);
            } else {
                ideas.push(idea);
            }
        }

        // Sort by volume descending
        ideas.sort_by(|a, b| {
            let av = a.volume_exact.unwrap_or(0);
            let bv = b.volume_exact.unwrap_or(0);
            bv.cmp(&av)
        });
        question_ideas.sort_by(|a, b| {
            let av = a.volume_exact.unwrap_or(0);
            let bv = b.volume_exact.unwrap_or(0);
            bv.cmp(&av)
        });

        KeywordIdeasResult {
            keyword: keyword.to_string(),
            country: country.to_string(),
            search_engine: search_engine.to_string(),
            ideas,
            question_ideas,
        }
    }
}

#[async_trait]
impl SeoDataProvider for DataForSeoProvider {
    async fn keyword_ideas(
        &self,
        keyword: &str,
        country: &str,
        search_engine: &str,
    ) -> Result<KeywordIdeasResult> {
        // Map country code to DataForSEO location code
        let location_code = match country.to_lowercase().as_str() {
            "us" | "usa" => "2840",
            "uk" | "gb" => "2826",
            "ca" => "2124",
            "au" => "2036",
            "de" => "2276",
            "fr" => "2250",
            _ => "2840", // Default to US
        };

        // Call both endpoints concurrently for broader coverage.
        // related_keywords = semantic discovery (Google "searches related to")
        // keyword_suggestions = substring matching (finds variations)
        let (related, suggestions) = tokio::join!(
            self.fetch_related_keywords(keyword, location_code),
            self.fetch_keyword_suggestions(keyword, location_code)
        );

        // If both failed, return the first error so the user sees what went wrong.
        let related = match related {
            Ok(r) => r,
            Err(e) => {
                return match suggestions {
                    Ok(s) => Ok(Self::merge_results(keyword, country, search_engine, Self::empty_result(keyword, country, search_engine), s)),
                    Err(_) => Err(e),
                };
            }
        };

        let suggestions = match suggestions {
            Ok(s) => s,
            Err(_) => return Ok(related),
        };

        Ok(Self::merge_results(keyword, country, search_engine, related, suggestions))
    }

    async fn keyword_difficulty(
        &self,
        keyword: &str,
        country: &str,
    ) -> Result<KeywordDifficultyResult> {
        // Map country code to DataForSEO location code
        let location_code = match country.to_lowercase().as_str() {
            "us" | "usa" => "2840",
            "uk" | "gb" => "2826",
            "ca" => "2124",
            "au" => "2036",
            "de" => "2276",
            "fr" => "2250",
            _ => "2840", // Default to US
        };

        let payload = serde_json::json!([
            {
                "keywords": [keyword],
                "location_code": location_code.parse::<i64>().unwrap_or(2840),
                "language_code": "en"
            }
        ]);

        let resp = self
            .client
            .post(self.api_url("/v3/dataforseo_labs/google/bulk_keyword_difficulty/live"))
            .header("Authorization", self.auth_header())
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .map_err(Error::Http)?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Other(format!(
                "DataForSEO keyword difficulty API returned status {}: {}",
                status, body
            )));
        }

        let data: Value = resp.json().await.map_err(Error::Http)?;

        // Check for API-level errors
        if let Some(status_code) = data.get("status_code").and_then(|v| v.as_i64()) {
            if status_code != 20000 {
                let status_msg = data
                    .get("status_message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error");
                return Err(Error::Other(format!(
                    "DataForSEO API error {}: {}",
                    status_code, status_msg
                )));
            }
        }

        self.parse_keyword_difficulty_response(&data, keyword)
    }

    async fn batch_keyword_difficulty(
        &self,
        keywords: &[String],
        country: &str,
    ) -> Result<Vec<KeywordDifficultyResult>> {
        if keywords.is_empty() {
            return Ok(vec![]);
        }

        // Map country code to DataForSEO location code
        let location_code = match country.to_lowercase().as_str() {
            "us" | "usa" => 2840,
            "uk" | "gb" => 2826,
            "ca" => 2124,
            "au" => 2036,
            "de" => 2276,
            "fr" => 2250,
            _ => 2840,
        };

        // DataForSEO supports up to 1000 keywords per request
        let mut results = Vec::with_capacity(keywords.len());

        for chunk in keywords.chunks(1000) {
            let kw_list: Vec<&str> = chunk.iter().map(|s| s.as_str()).collect();
            let payload_data = serde_json::json!([{
                "keywords": kw_list,
                "location_code": location_code,
                "language_code": "en"
            }]);

            let resp = self
                .client
                .post(self.api_url("/v3/dataforseo_labs/google/bulk_keyword_difficulty/live"))
                .header("Authorization", self.auth_header())
                .header("Content-Type", "application/json")
                .json(&payload_data)
                .send()
                .await
                .map_err(Error::Http)?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(Error::Other(format!(
                    "DataForSEO batch keyword difficulty API returned status {}: {}",
                    status, body
                )));
            }

            let data: Value = resp.json().await.map_err(Error::Http)?;

            // Check for API-level errors
            if let Some(status_code) = data.get("status_code").and_then(|v| v.as_i64()) {
                if status_code != 20000 {
                    let status_msg = data
                        .get("status_message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("Unknown error");
                    return Err(Error::Other(format!(
                        "DataForSEO API error {}: {}",
                        status_code, status_msg
                    )));
                }
            }

            // Parse results
            let empty_tasks = vec![];
            let tasks = data
                .get("tasks")
                .and_then(|t| t.as_array())
                .unwrap_or(&empty_tasks);

            for task in tasks {
                let empty_result = vec![];
                let task_result = task
                    .get("result")
                    .and_then(|r| r.as_array())
                    .unwrap_or(&empty_result);

                for item in task_result {
                    let keyword_text = item
                        .get("keyword")
                        .and_then(|k| k.as_str())
                        .unwrap_or("")
                        .to_string();

                    let difficulty = item.get("competition_index").and_then(|v| v.as_f64());

                    let last_update = item
                        .get("check_date")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    results.push(KeywordDifficultyResult {
                        keyword: keyword_text,
                        difficulty,
                        shortage: None,
                        last_update,
                        serp: vec![],
                    });
                }
            }
        }

        Ok(results)
    }

    async fn search_intent(&self, keywords: &[String]) -> Result<Vec<IntentClassification>> {
        if keywords.is_empty() {
            return Ok(vec![]);
        }

        // DataForSEO has a search intent endpoint, but for now we'll use pattern matching
        // as a fallback since the API endpoint may not be available in all plans
        // TODO: Implement actual DataForSEO search_intent API call if needed
        // POST /v3/dataforseo_labs/google/search_intent/live

        // For now, use pattern matching as fallback
        Ok(classify_batch_by_pattern(keywords))
    }

    fn name(&self) -> &'static str {
        "dataforseo"
    }
}
