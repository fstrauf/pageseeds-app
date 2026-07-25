use crate::error::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use ts_rs::TS;

// Compiled regex patterns for HTML cleaning
static SCRIPT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<script[^>]*>.*?</script>").unwrap());
static STYLE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"<style[^>]*>.*?</style>").unwrap());
static HTML_TAGS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static WHITESPACE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").unwrap());

/// Competitor word count data.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CompetitorWordCount {
    pub url: String,
    pub domain: String,
    pub position: i32,
    pub word_count: usize,
}

/// Section of a competitor article.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CompetitorSection {
    pub heading: String,
    pub level: u8, // 1, 2, or 3
    pub word_count: usize,
    pub is_thin: bool, // < 150 words
}

/// Full structure analysis of a competitor page.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct CompetitorStructure {
    pub url: String,
    pub domain: String,
    pub sections: Vec<CompetitorSection>,
    pub total_word_count: usize,
}

/// Word count comparison result.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct WordCountComparison {
    pub keyword: String,
    pub competitors: Vec<CompetitorWordCount>,
    pub median: usize,
    pub p75: usize,
    pub recommended_min: usize,
    pub user_word_count: Option<usize>,
    pub gap: Option<i64>, // user_count - recommended_min (negative = needs more)
}

/// Fetch and analyze a competitor page.
pub async fn analyze_competitor_page(
    url: &str,
    position: i32,
) -> Result<(CompetitorWordCount, CompetitorStructure)> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(crate::error::Error::Http)?;

    let resp = client
        .get(url)
        .send()
        .await
        .map_err(crate::error::Error::Http)?;

    let html = resp.text().await.map_err(crate::error::Error::Http)?;

    // Parse HTML
    let document = scraper::Html::parse_document(&html);

    // Extract domain
    let domain = extract_domain(url);

    // Extract main content
    let text_content = extract_main_content(&document);
    let word_count = crate::content::ops::count_words(&text_content);

    // Extract headings and sections
    let sections = extract_sections(&document, &text_content);

    let word_count_data = CompetitorWordCount {
        url: url.to_string(),
        domain: domain.clone(),
        position,
        word_count,
    };

    let structure = CompetitorStructure {
        url: url.to_string(),
        domain,
        sections,
        total_word_count: word_count,
    };

    Ok((word_count_data, structure))
}

/// Compare word counts for a keyword's SERP competitors.
pub async fn compare_word_counts(
    keyword: &str,
    competitor_urls: &[String],
    user_url: Option<&str>,
) -> Result<WordCountComparison> {
    let mut competitors = Vec::with_capacity(competitor_urls.len());

    for (idx, url) in competitor_urls.iter().enumerate() {
        match analyze_competitor_page(url, (idx + 1) as i32).await {
            Ok((word_count, _)) => {
                competitors.push(word_count);
            }
            Err(e) => {
                log::warn!("[competitor] Failed to analyze {}: {}", url, e);
            }
        }
    }

    if competitors.is_empty() {
        return Err(crate::error::Error::Other(
            "No competitor pages could be analyzed".to_string(),
        ));
    }

    // Sort by word count for percentile calculations
    let mut word_counts: Vec<usize> = competitors.iter().map(|c| c.word_count).collect();
    word_counts.sort_unstable();

    let median = calculate_median(&word_counts);
    let p75 = calculate_percentile(&word_counts, 75);
    let recommended_min = p75;

    // Get user's word count if provided
    let user_word_count = if let Some(url) = user_url {
        match analyze_competitor_page(url, 0).await {
            Ok((word_count, _)) => Some(word_count.word_count),
            Err(_) => None,
        }
    } else {
        None
    };

    let gap = user_word_count.map(|u| u as i64 - recommended_min as i64);

    Ok(WordCountComparison {
        keyword: keyword.to_string(),
        competitors,
        median,
        p75,
        recommended_min,
        user_word_count,
        gap,
    })
}

/// Extract main content text from HTML document.
fn extract_main_content(document: &scraper::Html) -> String {
    // Try to find main content area
    let selectors = [
        "article",
        "main",
        "[role='main']",
        ".content",
        ".post-content",
        ".entry-content",
        "#content",
        "body",
    ];

    for selector_str in &selectors {
        if let Ok(selector) = scraper::Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                let text = element.text().collect::<Vec<_>>().join(" ");
                let cleaned = clean_text(&text);
                if !cleaned.is_empty() {
                    return cleaned;
                }
            }
        }
    }

    // Fallback: get all body text
    if let Ok(body_selector) = scraper::Selector::parse("body") {
        if let Some(body) = document.select(&body_selector).next() {
            return clean_text(&body.text().collect::<Vec<_>>().join(" "));
        }
    }

    String::new()
}

/// Extract headings and their section word counts.
fn extract_sections(document: &scraper::Html, full_text: &str) -> Vec<CompetitorSection> {
    let mut sections = Vec::new();

    // Try to find H1, H2, H3 headings
    let heading_selector = match scraper::Selector::parse("h1, h2, h3") {
        Ok(s) => s,
        Err(_) => return sections,
    };

    let headings: Vec<_> = document.select(&heading_selector).collect();

    for (idx, heading) in headings.iter().enumerate() {
        let heading_text = heading.text().collect::<String>().trim().to_string();
        let level = heading.value().name().parse().unwrap_or(2);

        // Estimate word count for this section (rough approximation)
        let section_word_count =
            estimate_section_words(full_text, &heading_text, idx, headings.len());

        sections.push(CompetitorSection {
            heading: heading_text,
            level,
            word_count: section_word_count,
            is_thin: section_word_count < 150,
        });
    }

    sections
}

/// Estimate word count for a section based on heading position.
fn estimate_section_words(full_text: &str, _heading: &str, idx: usize, total: usize) -> usize {
    let words: Vec<&str> = full_text.split_whitespace().collect();
    let total_words = words.len();

    if total <= 1 {
        return total_words;
    }

    // Roughly divide text by number of sections
    let avg_section_size = total_words / total;
    let section_words = if idx == total - 1 {
        // Last section gets remaining words
        total_words - (idx * avg_section_size)
    } else {
        avg_section_size
    };

    section_words
}

/// Clean and normalize text.
fn clean_text(text: &str) -> String {
    let mut cleaned = text.to_string();

    // Remove script and style content
    cleaned = SCRIPT_RE.replace_all(&cleaned, " ").to_string();
    cleaned = STYLE_RE.replace_all(&cleaned, " ").to_string();

    // Remove HTML tags
    cleaned = HTML_TAGS_RE.replace_all(&cleaned, " ").to_string();

    // Normalize whitespace
    cleaned = WHITESPACE_RE.replace_all(&cleaned, " ").to_string();

    cleaned.trim().to_string()
}

/// Extract domain from URL.
fn extract_domain(url: &str) -> String {
    url.parse::<url::Url>()
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
        .unwrap_or_else(|| url.to_string())
}

/// Calculate median of sorted values.
fn calculate_median(sorted: &[usize]) -> usize {
    let len = sorted.len();
    if len == 0 {
        return 0;
    }
    if len % 2 == 1 {
        sorted[len / 2]
    } else {
        (sorted[len / 2 - 1] + sorted[len / 2]) / 2
    }
}

/// Calculate percentile of sorted values.
fn calculate_percentile(sorted: &[usize], percentile: usize) -> usize {
    let len = sorted.len();
    if len == 0 {
        return 0;
    }

    let index = ((percentile as f64 / 100.0) * (len - 1) as f64).round() as usize;
    sorted[index.min(len - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_median() {
        assert_eq!(calculate_median(&[1, 2, 3, 4, 5]), 3);
        assert_eq!(calculate_median(&[1, 2, 3, 4]), 2);
        assert_eq!(calculate_median(&[5]), 5);
        assert_eq!(calculate_median(&[]), 0);
    }

    #[test]
    fn test_calculate_percentile() {
        let data = vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100];
        assert_eq!(calculate_percentile(&data, 50), 60); // index = round(0.5 * 9) = 5
        assert_eq!(calculate_percentile(&data, 75), 80); // 75th percentile
        assert_eq!(calculate_percentile(&data, 0), 10); // min
        assert_eq!(calculate_percentile(&data, 100), 100); // max
    }

    #[test]
    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("https://example.com/path"), "example.com");
        assert_eq!(extract_domain("https://www.example.com"), "www.example.com");
    }
}
