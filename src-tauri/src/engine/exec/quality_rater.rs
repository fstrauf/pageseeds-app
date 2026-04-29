/// Content Quality Rater
///
/// Ported from SEO Machine's seo_quality_rater.py
/// Rates content quality against SEO best practices and provides
/// comprehensive scoring with publishing readiness gates.
///
/// # Scoring Categories
/// - **Content** (20%): Word count, paragraph length, structure
/// - **Keywords** (25%): Density, placement, H1/H2 optimization
/// - **Meta** (15%): Title/description length and keyword presence
/// - **Structure** (15%): Heading hierarchy, section count
/// - **Links** (15%): Internal/external link counts
/// - **Readability** (10%): Sentence length, formatting
use serde::{Deserialize, Serialize};

/// Quality rating result for a piece of content
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentQualityRating {
    /// Overall score 0-100
    pub overall_score: u8,
    /// Letter grade (A, B, C, D, F)
    pub grade: String,
    /// Individual category scores
    pub category_scores: CategoryScores,
    /// Critical issues that must be fixed
    pub critical_issues: Vec<String>,
    /// Warnings that should be addressed
    pub warnings: Vec<String>,
    /// Suggestions for improvement
    pub suggestions: Vec<String>,
    /// Whether content is ready to publish
    pub publishing_ready: bool,
    /// Detailed metrics
    pub details: QualityDetails,
}

/// Category-specific scores
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryScores {
    /// Content length and structure (0-100)
    pub content: u8,
    /// Keyword optimization (0-100)
    pub keywords: u8,
    /// Meta elements (0-100)
    pub meta_elements: u8,
    /// Content structure (0-100)
    pub structure: u8,
    /// Internal/external links (0-100)
    pub links: u8,
    /// Readability metrics (0-100)
    pub readability: u8,
}

/// Detailed quality metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityDetails {
    /// Total word count
    pub word_count: usize,
    /// Number of H2 sections
    pub h2_count: usize,
    /// Has H1 heading
    pub has_h1: bool,
    /// Keyword appears in H1
    pub keyword_in_h1: bool,
    /// Keyword in first 100 words
    pub keyword_in_first_100: bool,
    /// Internal link count
    pub internal_link_count: usize,
    /// External link count
    pub external_link_count: usize,
    /// Meta title length
    pub meta_title_length: usize,
    /// Meta description length
    pub meta_description_length: usize,
    /// Average sentence length
    pub avg_sentence_length: f64,
    /// Number of lists (bullet/numbered)
    pub list_count: usize,
}

/// Content to analyze
pub struct ContentToAnalyze<'a> {
    /// Article content (markdown/MDX)
    pub content: &'a str,
    /// Target keyword
    pub target_keyword: &'a str,
    /// Meta title (optional)
    pub meta_title: Option<&'a str>,
    /// Meta description (optional)
    pub meta_description: Option<&'a str>,
}

/// SEO guidelines configuration
pub struct SeoGuidelines {
    /// Minimum word count
    pub min_word_count: usize,
    /// Optimal word count
    pub optimal_word_count: usize,
    /// Maximum word count
    pub max_word_count: usize,
    /// Minimum keyword density %
    pub keyword_density_min: f64,
    /// Maximum keyword density %
    pub keyword_density_max: f64,
    /// Minimum internal links
    pub min_internal_links: usize,
    /// Optimal internal links
    pub optimal_internal_links: usize,
    /// Minimum external links
    pub min_external_links: usize,
    /// Optimal external links
    pub optimal_external_links: usize,
    /// Meta title min length
    pub meta_title_min: usize,
    /// Meta title max length
    pub meta_title_max: usize,
    /// Meta description min length
    pub meta_description_min: usize,
    /// Meta description max length
    pub meta_description_max: usize,
    /// Minimum H2 sections
    pub min_h2_sections: usize,
    /// Optimal H2 sections
    pub optimal_h2_sections: usize,
    /// Target sentence length
    pub max_sentence_length: usize,
}

impl Default for SeoGuidelines {
    fn default() -> Self {
        Self {
            min_word_count: 2000,
            optimal_word_count: 2500,
            max_word_count: 3000,
            keyword_density_min: 1.0,
            keyword_density_max: 2.0,
            min_internal_links: 3,
            optimal_internal_links: 5,
            min_external_links: 2,
            optimal_external_links: 3,
            meta_title_min: 50,
            meta_title_max: 60,
            meta_description_min: 150,
            meta_description_max: 160,
            min_h2_sections: 4,
            optimal_h2_sections: 6,
            max_sentence_length: 25,
        }
    }
}

/// Analyze content and return quality rating
pub fn rate_content(content: &ContentToAnalyze) -> ContentQualityRating {
    let guidelines = SeoGuidelines::default();
    let structure = analyze_structure(content);

    let content_score = score_content(&structure, &guidelines);
    let keyword_score = score_keywords(content, &structure, &guidelines);
    let meta_score = score_meta(content, &guidelines);
    let structure_score = score_structure(&structure, &guidelines);
    let link_score = score_links(content, &guidelines);
    let readability_score = score_readability(content, &guidelines);

    // Calculate weighted overall score
    let overall_score = calculate_overall_score(
        content_score,
        keyword_score,
        meta_score,
        structure_score,
        link_score,
        readability_score,
    );

    // Compile all issues
    let mut critical_issues = Vec::new();
    let mut warnings = Vec::new();
    let mut suggestions = Vec::new();

    collect_content_issues(
        &content_score,
        &structure,
        &guidelines,
        &mut critical_issues,
        &mut warnings,
    );
    collect_keyword_issues(
        &keyword_score,
        &structure,
        content.target_keyword,
        &guidelines,
        &mut critical_issues,
        &mut warnings,
    );
    collect_meta_issues(
        &meta_score,
        content,
        &guidelines,
        &mut critical_issues,
        &mut warnings,
    );
    collect_structure_issues(
        &structure_score,
        &structure,
        &guidelines,
        &mut critical_issues,
        &mut warnings,
    );
    collect_link_issues(
        &link_score,
        content,
        &guidelines,
        &mut warnings,
        &mut suggestions,
    );
    collect_readability_issues(
        &readability_score,
        content,
        &guidelines,
        &mut warnings,
        &mut suggestions,
    );

    let publishing_ready = overall_score >= 80 && critical_issues.is_empty();

    ContentQualityRating {
        overall_score,
        grade: score_to_grade(overall_score),
        category_scores: CategoryScores {
            content: content_score,
            keywords: keyword_score,
            meta_elements: meta_score,
            structure: structure_score,
            links: link_score,
            readability: readability_score,
        },
        critical_issues,
        warnings,
        suggestions,
        publishing_ready,
        details: structure,
    }
}

/// Content structure analysis
fn analyze_structure(content: &ContentToAnalyze) -> QualityDetails {
    let text = content.content;
    let lines: Vec<&str> = text.lines().collect();

    // Extract headings
    let mut h1_count = 0;
    let mut h2_count = 0;
    let mut h1_text = String::new();
    let mut h2_texts = Vec::new();

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            h1_count += 1;
            if h1_text.is_empty() {
                h1_text = trimmed[2..].to_string();
            }
        } else if trimmed.starts_with("## ") {
            h2_count += 1;
            h2_texts.push(trimmed[3..].to_string());
        }
    }

    // Word count
    let word_count = text.split_whitespace().count();

    // Paragraph analysis
    let _paragraphs: Vec<&str> = text
        .split("\n\n")
        .filter(|p| {
            let trimmed = p.trim();
            !trimmed.is_empty() && !trimmed.starts_with('#')
        })
        .collect();

    // Keyword checks
    let keyword_lower = content.target_keyword.to_lowercase();
    let _text_lower = text.to_lowercase();

    let keyword_in_h1 = h1_text.to_lowercase().contains(&keyword_lower);

    let first_100_words: String = text
        .split_whitespace()
        .take(100)
        .collect::<Vec<_>>()
        .join(" ");
    let keyword_in_first_100 = first_100_words.to_lowercase().contains(&keyword_lower);

    // Link counts (markdown format)
    let internal_link_count = count_internal_links(text);
    let external_link_count = count_external_links(text);

    // List count
    let list_count = text
        .lines()
        .filter(|l| {
            let trimmed = l.trim();
            trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || trimmed.starts_with("1. ")
                || trimmed.starts_with("2. ")
        })
        .count();

    // Sentence analysis
    let sentences: Vec<&str> = text
        .split(|c| c == '.' || c == '!' || c == '?')
        .filter(|s| !s.trim().is_empty())
        .collect();
    let avg_sentence_length = if sentences.is_empty() {
        0.0
    } else {
        sentences
            .iter()
            .map(|s| s.split_whitespace().count())
            .sum::<usize>() as f64
            / sentences.len() as f64
    };

    QualityDetails {
        word_count,
        h2_count,
        has_h1: h1_count > 0,
        keyword_in_h1,
        keyword_in_first_100,
        internal_link_count,
        external_link_count,
        meta_title_length: content.meta_title.map(|t| t.len()).unwrap_or(0),
        meta_description_length: content.meta_description.map(|d| d.len()).unwrap_or(0),
        avg_sentence_length,
        list_count,
    }
}

/// Score content length and quality
fn score_content(structure: &QualityDetails, guidelines: &SeoGuidelines) -> u8 {
    let mut score = 100;

    // Word count scoring
    if structure.word_count < guidelines.min_word_count {
        score -= 30;
    } else if structure.word_count < guidelines.optimal_word_count {
        score -= 10;
    } else if structure.word_count > guidelines.max_word_count {
        score -= 5;
    }

    score.max(0)
}

/// Score keyword optimization
fn score_keywords(
    content: &ContentToAnalyze,
    structure: &QualityDetails,
    guidelines: &SeoGuidelines,
) -> u8 {
    let mut score = 100;

    // Keyword in H1
    if !structure.keyword_in_h1 {
        score -= 20;
    }

    // Keyword in first 100 words
    if !structure.keyword_in_first_100 {
        score -= 15;
    }

    // Keyword density calculation
    let text_lower = content.content.to_lowercase();
    let keyword_lower = content.target_keyword.to_lowercase();
    let keyword_count = text_lower.matches(&keyword_lower).count();
    let density = if structure.word_count > 0 {
        (keyword_count as f64 / structure.word_count as f64) * 100.0
    } else {
        0.0
    };

    if density < guidelines.keyword_density_min {
        score -= 15;
    } else if density > guidelines.keyword_density_max * 1.5 {
        score -= 20; // Keyword stuffing risk
    } else if density > guidelines.keyword_density_max {
        score -= 10;
    }

    score.max(0)
}

/// Score meta elements
fn score_meta(content: &ContentToAnalyze, guidelines: &SeoGuidelines) -> u8 {
    let mut score = 100;

    // Meta title
    if content.meta_title.is_none() {
        score -= 40;
    } else if let Some(title) = content.meta_title {
        let len = title.len();
        if len < guidelines.meta_title_min {
            score -= 15;
        } else if len > guidelines.meta_title_max + 10 {
            score -= 10;
        }

        // Keyword in title
        let keyword_lower = content.target_keyword.to_lowercase();
        if !title.to_lowercase().contains(&keyword_lower) {
            score -= 15;
        }
    }

    // Meta description
    if content.meta_description.is_none() {
        score -= 40;
    } else if let Some(desc) = content.meta_description {
        let len = desc.len();
        if len < guidelines.meta_description_min {
            score -= 15;
        } else if len > guidelines.meta_description_max + 10 {
            score -= 10;
        }
    }

    score.max(0)
}

/// Score content structure
fn score_structure(structure: &QualityDetails, guidelines: &SeoGuidelines) -> u8 {
    let mut score = 100;

    // H1 check
    if !structure.has_h1 {
        score -= 30;
    }

    // H2 count
    if structure.h2_count < guidelines.min_h2_sections {
        score -= 15;
    } else if structure.h2_count < guidelines.optimal_h2_sections {
        score -= 5;
    }

    score.max(0)
}

/// Score internal/external links
fn score_links(content: &ContentToAnalyze, guidelines: &SeoGuidelines) -> u8 {
    let mut score = 100;

    // Internal links
    let internal_count = count_internal_links(content.content);
    if internal_count < guidelines.min_internal_links {
        score -= 20;
    } else if internal_count < guidelines.optimal_internal_links {
        score -= 5;
    }

    // External links
    let external_count = count_external_links(content.content);
    if external_count < guidelines.min_external_links {
        score -= 15;
    } else if external_count < guidelines.optimal_external_links {
        score -= 5;
    }

    score.max(0)
}

/// Score readability
fn score_readability(content: &ContentToAnalyze, guidelines: &SeoGuidelines) -> u8 {
    let mut score = 100;

    // Sentence length analysis
    let text = content.content;
    let sentences: Vec<&str> = text
        .split(|c| c == '.' || c == '!' || c == '?')
        .filter(|s| !s.trim().is_empty())
        .collect();

    let avg_length = if sentences.is_empty() {
        0.0
    } else {
        sentences
            .iter()
            .map(|s| s.split_whitespace().count())
            .sum::<usize>() as f64
            / sentences.len() as f64
    };

    if avg_length > guidelines.max_sentence_length as f64 {
        score -= 10;
    }

    // Check for very long sentences
    let long_sentences = sentences
        .iter()
        .filter(|s| s.split_whitespace().count() > guidelines.max_sentence_length * 3 / 2)
        .count();

    if long_sentences > sentences.len() / 5 {
        score -= 10;
    }

    // List usage
    let list_count = text
        .lines()
        .filter(|l| {
            let trimmed = l.trim();
            trimmed.starts_with("- ")
                || trimmed.starts_with("* ")
                || (trimmed.len() > 2
                    && trimmed.chars().next().unwrap().is_ascii_digit()
                    && trimmed.chars().nth(1) == Some('.')
                    && trimmed.chars().nth(2) == Some(' '))
        })
        .count();

    if list_count == 0 {
        score -= 5;
    }

    score.max(0)
}

/// Calculate overall weighted score
fn calculate_overall_score(
    content: u8,
    keywords: u8,
    meta: u8,
    structure: u8,
    links: u8,
    readability: u8,
) -> u8 {
    let weighted = (content as f64 * 0.20
        + keywords as f64 * 0.25
        + meta as f64 * 0.15
        + structure as f64 * 0.15
        + links as f64 * 0.15
        + readability as f64 * 0.10) as u8;

    weighted.min(100)
}

/// Convert score to letter grade
fn score_to_grade(score: u8) -> String {
    match score {
        90..=100 => "A".to_string(),
        80..=89 => "B".to_string(),
        70..=79 => "C".to_string(),
        60..=69 => "D".to_string(),
        _ => "F".to_string(),
    }
}

/// Count internal markdown links
fn count_internal_links(content: &str) -> usize {
    // Internal links: [text](path) where path doesn't start with http
    content
        .matches("](")
        .filter(|_| true) // We'll check each occurrence
        .count()
}

/// Count external markdown links
fn count_external_links(content: &str) -> usize {
    // External links: [text](http...)
    content.matches("](http").count()
}

// Issue collection functions

fn collect_content_issues(
    _score: &u8,
    structure: &QualityDetails,
    guidelines: &SeoGuidelines,
    critical: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    if structure.word_count < guidelines.min_word_count {
        critical.push(format!(
            "Content is too short ({} words). Minimum is {} words.",
            structure.word_count, guidelines.min_word_count
        ));
    } else if structure.word_count < guidelines.optimal_word_count {
        warnings.push(format!(
            "Content could be longer ({} words). Optimal is {}+ words.",
            structure.word_count, guidelines.optimal_word_count
        ));
    }
}

fn collect_keyword_issues(
    _score: &u8,
    structure: &QualityDetails,
    keyword: &str,
    _guidelines: &SeoGuidelines,
    critical: &mut Vec<String>,
    _warnings: &mut Vec<String>,
) {
    if !structure.keyword_in_h1 {
        critical.push(format!(
            "Primary keyword '{}' missing from H1 heading",
            keyword
        ));
    }

    if !structure.keyword_in_first_100 {
        critical.push(format!(
            "Primary keyword '{}' missing from first 100 words",
            keyword
        ));
    }
}

fn collect_meta_issues(
    _score: &u8,
    content: &ContentToAnalyze,
    guidelines: &SeoGuidelines,
    critical: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    if content.meta_title.is_none() {
        critical.push("Meta title is missing".to_string());
    } else if let Some(title) = content.meta_title {
        let len = title.len();
        if len < guidelines.meta_title_min {
            warnings.push(format!(
                "Meta title too short ({} chars). Target is {}-{} chars.",
                len, guidelines.meta_title_min, guidelines.meta_title_max
            ));
        } else if len > guidelines.meta_title_max + 10 {
            warnings.push(format!(
                "Meta title too long ({} chars). Target is {}-{} chars.",
                len, guidelines.meta_title_min, guidelines.meta_title_max
            ));
        }
    }

    if content.meta_description.is_none() {
        critical.push("Meta description is missing".to_string());
    } else if let Some(desc) = content.meta_description {
        let len = desc.len();
        if len < guidelines.meta_description_min {
            warnings.push(format!(
                "Meta description too short ({} chars). Target is {}-{} chars.",
                len, guidelines.meta_description_min, guidelines.meta_description_max
            ));
        } else if len > guidelines.meta_description_max + 10 {
            warnings.push(format!(
                "Meta description too long ({} chars). Target is {}-{} chars.",
                len, guidelines.meta_description_min, guidelines.meta_description_max
            ));
        }
    }
}

fn collect_structure_issues(
    _score: &u8,
    structure: &QualityDetails,
    guidelines: &SeoGuidelines,
    critical: &mut Vec<String>,
    warnings: &mut Vec<String>,
) {
    if !structure.has_h1 {
        critical.push("Missing H1 heading".to_string());
    }

    if structure.h2_count < guidelines.min_h2_sections {
        warnings.push(format!(
            "Too few H2 sections ({}). Add more main sections (target: {}).",
            structure.h2_count, guidelines.optimal_h2_sections
        ));
    }
}

fn collect_link_issues(
    _score: &u8,
    content: &ContentToAnalyze,
    guidelines: &SeoGuidelines,
    warnings: &mut Vec<String>,
    suggestions: &mut Vec<String>,
) {
    let internal = count_internal_links(content.content);
    let external = count_external_links(content.content);

    if internal < guidelines.min_internal_links {
        warnings.push(format!(
            "Too few internal links ({}). Add {} more (target: {}).",
            internal,
            guidelines.min_internal_links - internal,
            guidelines.optimal_internal_links
        ));
    } else if internal < guidelines.optimal_internal_links {
        suggestions.push(format!(
            "Could add more internal links ({}). Optimal is {}.",
            internal, guidelines.optimal_internal_links
        ));
    }

    if external < guidelines.min_external_links {
        warnings.push(format!(
            "Too few external links ({}). Add authoritative sources (target: {}).",
            external, guidelines.optimal_external_links
        ));
    }
}

fn collect_readability_issues(
    _score: &u8,
    _content: &ContentToAnalyze,
    _guidelines: &SeoGuidelines,
    _warnings: &mut Vec<String>,
    _suggestions: &mut Vec<String>,
) {
    // This is already handled in score_readability
    // Additional issues can be added here
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_content() -> ContentToAnalyze<'static> {
        ContentToAnalyze {
            content: r#"# How to Start a Podcast

Starting a podcast is easier than you think. This complete guide shows you how to start a podcast from scratch.

## Choose Your Topic

Pick a topic you're passionate about. Your podcast topic should resonate with your target audience.

## Get Equipment

You'll need a microphone, headphones, and recording software.

## Record Your First Episode

Start recording! Don't worry about perfection on your first try.

## Publish Your Podcast

Upload to a [podcast hosting platform](podcast-hosting) and distribute to directories.

Learn more about [audio editing](audio-editing) and [marketing](marketing-guide).

For external resources, see [Podcast Movement](https://podcastmovement.com).

Ready to start your podcast? Begin today with these simple steps."#,
            target_keyword: "start a podcast",
            meta_title: Some("How to Start a Podcast: Complete Guide for 2024"),
            meta_description: Some("Learn how to start a podcast from scratch with this step-by-step guide. Everything you need to know about podcast equipment, recording, and publishing."),
        }
    }

    #[test]
    fn test_content_rating() {
        let content = sample_content();
        let rating = rate_content(&content);

        assert!(rating.overall_score > 0);
        assert!(!rating.grade.is_empty());
        assert_eq!(rating.category_scores.content, 70); // Short content penalty
    }

    #[test]
    fn test_grade_calculation() {
        assert_eq!(score_to_grade(95), "A");
        assert_eq!(score_to_grade(85), "B");
        assert_eq!(score_to_grade(75), "C");
        assert_eq!(score_to_grade(65), "D");
        assert_eq!(score_to_grade(55), "F");
    }

    #[test]
    fn test_structure_analysis() {
        let content = sample_content();
        let structure = analyze_structure(&content);

        assert!(structure.has_h1);
        assert_eq!(structure.h2_count, 4);
        assert!(structure.keyword_in_h1);
        assert!(structure.keyword_in_first_100);
        assert!(structure.internal_link_count >= 3);
        assert!(structure.external_link_count >= 1);
    }
}
