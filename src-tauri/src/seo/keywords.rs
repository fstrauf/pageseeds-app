use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::seo::solve_ahrefs_captcha;

// ─── Data structures ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordIdea {
    pub keyword: String,
    pub idea_type: String, // "regular" | "question"
    pub difficulty: Option<String>,
    pub volume: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordIdeasResult {
    pub keyword: String,
    pub country: String,
    pub search_engine: String,
    pub ideas: Vec<KeywordIdea>,
    pub question_ideas: Vec<KeywordIdea>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SerpEntry {
    pub title: String,
    pub url: String,
    pub domain: String,
    pub position: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordDifficultyResult {
    pub keyword: String,
    pub difficulty: f64,
    pub shortage: f64,
    pub last_update: String,
    pub serp: Vec<SerpEntry>,
}

// ─── Ahrefs Option-style JSON unwrapping ─────────────────────────────────────

/// Recursively unwrap Ahrefs Option-style arrays like ["Some", {...}] or ["Ok", {...}].
fn unwrap_variant(value: &Value) -> &Value {
    let mut current = value;
    loop {
        match current {
            Value::Array(arr) if arr.len() == 2 => {
                if let Some(marker) = arr[0].as_str() {
                    match marker.to_ascii_lowercase().as_str() {
                        "some" | "ok" | "result" => {
                            current = &arr[1];
                            continue;
                        }
                        _ => break,
                    }
                }
                break;
            }
            _ => break,
        }
    }
    current
}

fn extract_payload(data: &Value) -> Option<&Value> {
    let candidate = unwrap_variant(data);
    match candidate {
        Value::Object(map) => {
            // Some responses nest the payload under a "data" key
            if let Some(inner) = map.get("data") {
                if inner.is_object() {
                    return Some(inner);
                }
            }
            Some(candidate)
        }
        Value::Array(arr) => {
            for item in arr {
                if let Some(p) = extract_payload(item) {
                    return Some(p);
                }
            }
            None
        }
        _ => None,
    }
}

fn ensure_list(value: &Value) -> Vec<&Value> {
    let unwrapped = unwrap_variant(value);
    match unwrapped {
        Value::Array(arr) => {
            // Guard: a 2-element array whose first element is a marker string is still an
            // Option variant — unwrap one more level.
            if arr.len() == 2 {
                if let Some(marker) = arr[0].as_str() {
                    match marker.to_ascii_lowercase().as_str() {
                        "some" | "ok" => {
                            if arr[1].is_array() {
                                return ensure_list(&arr[1]);
                            }
                        }
                        _ => {}
                    }
                }
            }
            arr.iter().collect()
        }
        Value::Object(_) => vec![unwrapped],
        _ => vec![],
    }
}

fn get_field_as_string(obj: &Value, keys: &[&str]) -> Option<String> {
    for &key in keys {
        if let Some(v) = obj.get(key) {
            let unwrapped = unwrap_variant(v);
            match unwrapped {
                Value::String(s) if !s.is_empty() => return Some(s.clone()),
                Value::Number(n) => return Some(n.to_string()),
                _ => {}
            }
        }
    }
    None
}

fn normalise_idea(raw: &Value, idea_type: &str) -> Option<KeywordIdea> {
    let idea = unwrap_variant(raw);

    // If still an array, recurse into items looking for a dict
    if let Value::Array(arr) = idea {
        for item in arr {
            if let Some(result) = normalise_idea(item, idea_type) {
                return Some(result);
            }
        }
        return None;
    }

    if !idea.is_object() {
        return None;
    }

    // Extract keyword
    let keyword = get_field_as_string(idea, &["keyword", "kw", "phrase", "query", "text", "value"])
        .or_else(|| {
            // Try nested keyword object
            idea.get("keyword")
                .or_else(|| idea.get("kw"))
                .and_then(|kw| {
                    get_field_as_string(
                        unwrap_variant(kw),
                        &["keyword", "kw", "phrase", "query", "text", "value"],
                    )
                })
        })?;

    if keyword.is_empty() {
        return None;
    }

    // Extract metrics sub-object (may be nested)
    let metrics_null = Value::Null;
    let metrics = idea
        .get("metrics")
        .map(|m| unwrap_variant(m))
        .unwrap_or(&metrics_null);

    let difficulty =
        get_field_as_string(idea, &["difficultyLabel", "difficulty", "difficulty_text"]).or_else(
            || {
                if metrics.is_object() {
                    get_field_as_string(
                        metrics,
                        &["difficultyLabel", "difficulty", "kd", "keywordDifficulty"],
                    )
                } else {
                    None
                }
            },
        );

    let volume =
        get_field_as_string(idea, &["volumeLabel", "volume", "searchVolume"]).or_else(|| {
            if metrics.is_object() {
                get_field_as_string(metrics, &["volumeLabel", "volume", "searchVolume"])
            } else {
                None
            }
        });

    let country =
        get_field_as_string(idea, &["country", "countryCode", "location"]);

    Some(KeywordIdea {
        keyword,
        idea_type: idea_type.to_string(),
        difficulty,
        volume,
        country,
    })
}

fn parse_ideas_response(data: &Value, keyword: &str, country: &str, search_engine: &str) -> Option<KeywordIdeasResult> {
    let payload = extract_payload(data)?;
    let obj = payload.as_object()?;

    let mut ideas: Vec<KeywordIdea> = vec![];
    let mut question_ideas: Vec<KeywordIdea> = vec![];

    if let Some(all_section) = obj.get("allIdeas") {
        for item in ensure_list(all_section) {
            if let Some(idea) = normalise_idea(item, "regular") {
                ideas.push(idea);
            }
        }
    }

    if let Some(q_section) = obj.get("questionIdeas") {
        for item in ensure_list(q_section) {
            if let Some(idea) = normalise_idea(item, "question") {
                question_ideas.push(idea);
            }
        }
    }

    if ideas.is_empty() && question_ideas.is_empty() {
        return None;
    }

    Some(KeywordIdeasResult {
        keyword: keyword.to_string(),
        country: country.to_string(),
        search_engine: search_engine.to_string(),
        ideas,
        question_ideas,
    })
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Fetch keyword ideas (suggestions + questions) for a seed keyword via the Ahrefs free API.
/// Internally acquires a CapSolver hCaptcha token first.
pub async fn get_keyword_ideas(
    capsolver_key: &str,
    keyword: &str,
    country: &str,
    search_engine: &str,
) -> Result<KeywordIdeasResult> {
    let token = solve_ahrefs_captcha(capsolver_key).await?;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36")
        .build()
        .map_err(Error::Http)?;

    let resp = client
        .post("https://ahrefs.com/v4/stGetFreeKeywordIdeas")
        .json(&serde_json::json!({
            "withQuestionIdeas": true,
            "captcha": token,
            "searchEngine": [search_engine],
            "country": country,
            "keyword": keyword
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(Error::Other(format!(
            "Ahrefs keyword ideas API returned status {}",
            resp.status()
        )));
    }

    let data: Value = resp.json().await?;

    parse_ideas_response(&data, keyword, country, search_engine)
        .ok_or_else(|| Error::Other("Unable to parse keyword ideas from Ahrefs response".to_string()))
}

/// Fetch keyword difficulty (KD score + SERP overview) for a single keyword.
/// Internally acquires a CapSolver hCaptcha token first.
pub async fn get_keyword_difficulty(
    capsolver_key: &str,
    keyword: &str,
    country: &str,
) -> Result<KeywordDifficultyResult> {
    let token = solve_ahrefs_captcha(capsolver_key).await?;

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36")
        .build()
        .map_err(Error::Http)?;

    let resp = client
        .post("https://ahrefs.com/v4/stGetFreeSerpOverviewForKeywordDifficultyChecker")
        .header(
            "referer",
            format!(
                "https://ahrefs.com/keyword-difficulty/?country={}&input={}",
                country, keyword
            ),
        )
        .json(&serde_json::json!({
            "captcha": token,
            "country": country,
            "keyword": keyword
        }))
        .send()
        .await?;

    if !resp.status().is_success() {
        return Err(Error::Other(format!(
            "Ahrefs keyword difficulty API returned status {}",
            resp.status()
        )));
    }

    let data: Value = resp.json().await?;

    // Response format: ["Ok", { difficulty, shortage, lastUpdate, serp: { results: [...] } }]
    let kd_data = match &data {
        Value::Array(arr) if arr.len() >= 2 && arr[0].as_str() == Some("Ok") => &arr[1],
        _ => {
            return Err(Error::Other(
                "Unexpected keyword difficulty response format".to_string(),
            ))
        }
    };

    let difficulty = kd_data["difficulty"].as_f64().unwrap_or(0.0);
    let shortage = kd_data["shortage"].as_f64().unwrap_or(0.0);
    let last_update = kd_data["lastUpdate"].as_str().unwrap_or("").to_string();

    // Parse organic SERP results
    let mut serp: Vec<SerpEntry> = vec![];
    if let Some(results) = kd_data["serp"]["results"].as_array() {
        for (idx, item) in results.iter().enumerate() {
            // Only organic: item["content"][0] == "organic"
            if let Some(content) = item["content"].as_array() {
                if content.first().and_then(|v| v.as_str()) != Some("organic") {
                    continue;
                }
                if let Some(organic) = content.get(1) {
                    // link field: ["Some", { title, url: ["Url", { url }], domain }]
                    if let Some(link_arr) = organic["link"].as_array() {
                        if link_arr.first().and_then(|v| v.as_str()) == Some("Some") {
                            if let Some(link_data) = link_arr.get(1) {
                                let title =
                                    link_data["title"].as_str().unwrap_or("").to_string();
                                let url = link_data["url"]
                                    .as_array()
                                    .and_then(|a| a.get(1))
                                    .and_then(|v| v["url"].as_str())
                                    .unwrap_or("")
                                    .to_string();
                                let domain =
                                    link_data["domain"].as_str().unwrap_or("").to_string();
                                serp.push(SerpEntry {
                                    title,
                                    url,
                                    domain,
                                    position: (idx + 1) as i64,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(KeywordDifficultyResult {
        keyword: keyword.to_string(),
        difficulty,
        shortage,
        last_update,
        serp,
    })
}
