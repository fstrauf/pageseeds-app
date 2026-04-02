/// Google Autocomplete API integration for keyword research.
/// Uses the undocumented but stable suggestqueries.google.com endpoint.
/// No authentication required, no rate limits (within reason).

use serde_json::Value;
use crate::error::{Error, Result};

/// A keyword suggestion from Google Autocomplete.
#[derive(Debug, Clone)]
pub struct AutocompleteSuggestion {
    pub keyword: String,
    pub suggestion_type: SuggestionType,
}

/// Type of autocomplete suggestion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SuggestionType {
    Regular,
    Question,
}

/// Fetch autocomplete suggestions from Google for a seed keyword.
/// 
/// # Arguments
/// * `keyword` - The seed keyword to expand
/// * `country` - Country code (e.g., "us", "uk", "au")
/// * `language` - Language code (e.g., "en", "es")
pub async fn fetch_suggestions(
    keyword: &str,
    country: &str,
    language: &str,
) -> Result<Vec<AutocompleteSuggestion>> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(Error::Http)?;

    let url = format!(
        "https://suggestqueries.google.com/complete/search?output=firefox&hl={}&gl={}&q={}",
        language,
        country,
        urlencoding::encode(keyword)
    );

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(Error::Http)?;

    if !resp.status().is_success() {
        return Err(Error::Other(format!(
            "Google autocomplete returned status {}",
            resp.status()
        )));
    }

    let text = resp.text().await.map_err(Error::Http)?;
    parse_firefox_response(&text, keyword)
}

/// Fetch autocomplete suggestions with question prefixes to get question-based keywords.
pub async fn fetch_question_suggestions(
    keyword: &str,
    country: &str,
    language: &str,
) -> Result<Vec<AutocompleteSuggestion>> {
    let mut all = vec![];
    
    // Common question prefixes
    let prefixes = [
        format!("what is {}", keyword),
        format!("what are {}", keyword),
        format!("how to {}", keyword),
        format!("how does {} work", keyword),
        format!("why {}", keyword),
        format!("{} guide", keyword),
        format!("{} tutorial", keyword),
        format!("{} for beginners", keyword),
    ];
    
    for prefixed in &prefixes {
        match fetch_suggestions(prefixed, country, language).await {
            Ok(suggestions) => {
                for mut s in suggestions {
                    s.suggestion_type = SuggestionType::Question;
                    all.push(s);
                }
            }
            Err(e) => {
                log::warn!(
                    "[google_autocomplete] Failed for prefix '{}': {}",
                    prefixed, e
                );
            }
        }
        
        // Small delay to be polite
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
    
    Ok(all)
}

/// Parse the Firefox-format JSON response from Google.
/// 
/// Response format:
/// ```json
/// [
///   "query",
///   ["suggestion1", "suggestion2", ...],
///   [],
///   {"google:suggestsubtypes": [...]}
/// ]
/// ```
fn parse_firefox_response(text: &str, source_keyword: &str) -> Result<Vec<AutocompleteSuggestion>> {
    let parsed: Value = serde_json::from_str(text)
        .map_err(|e| Error::Other(format!("Failed to parse Google response: {}", e)))?;
    
    let suggestions = parsed
        .as_array()
        .and_then(|arr| arr.get(1))
        .and_then(|v| v.as_array())
        .ok_or_else(|| Error::Other("Invalid Google autocomplete response format".to_string()))?;
    
    let mut result = vec![];
    for item in suggestions {
        if let Some(keyword) = item.as_str() {
            // Skip exact matches of the source keyword
            if keyword.to_lowercase() == source_keyword.to_lowercase() {
                continue;
            }
            
            result.push(AutocompleteSuggestion {
                keyword: keyword.to_string(),
                suggestion_type: SuggestionType::Regular,
            });
        }
    }
    
    Ok(result)
}

/// Get keyword ideas by combining regular and question suggestions.
/// This mirrors the Ahrefs get_keyword_ideas API structure.
pub async fn get_keyword_ideas_google(
    keyword: &str,
    country: &str,
    _search_engine: &str, // ignored, Google only
) -> Result<GoogleKeywordIdeasResult> {
    let language = match country {
        "us" | "uk" | "au" | "ca" | "nz" => "en",
        "de" => "de",
        "fr" => "fr",
        "es" => "es",
        "it" => "it",
        "nl" => "nl",
        "br" => "pt",
        "jp" => "ja",
        _ => "en",
    };
    
    log::info!(
        "[google_autocomplete] Fetching ideas for '{}' (country: {})",
        keyword, country
    );
    
    // Fetch regular suggestions
    let regular = fetch_suggestions(keyword, country, language).await?;
    
    // Fetch question-based suggestions
    let questions = fetch_question_suggestions(keyword, country, language).await?;
    
    log::info!(
        "[google_autocomplete] Found {} regular + {} question suggestions for '{}'",
        regular.len(),
        questions.len(),
        keyword
    );
    
    Ok(GoogleKeywordIdeasResult {
        keyword: keyword.to_string(),
        country: country.to_string(),
        ideas: regular,
        question_ideas: questions,
    })
}

/// Result structure matching the old Ahrefs API for easy integration.
#[derive(Debug, Clone)]
pub struct GoogleKeywordIdeasResult {
    pub keyword: String,
    pub country: String,
    pub ideas: Vec<AutocompleteSuggestion>,
    pub question_ideas: Vec<AutocompleteSuggestion>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_firefox_response() {
        let json = r#"["options trading",["options trading","options trading for beginners","options trading strategies"],[],{"google:suggestsubtypes":[[512],[512],[512]]}]"#;
        
        let result = parse_firefox_response(json, "options trading").unwrap();
        
        assert_eq!(result.len(), 2); // excluding the exact match
        assert_eq!(result[0].keyword, "options trading for beginners");
        assert_eq!(result[1].keyword, "options trading strategies");
    }

    #[test]
    fn test_parse_empty_response() {
        let json = r#"["xyz123",[],[],{}]"#;
        
        let result = parse_firefox_response(json, "xyz123").unwrap();
        
        assert!(result.is_empty());
    }
}
