use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Section presence for keyword distribution.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SectionPresence {
    pub section: String,  // "intro", "body", "conclusion"
    pub present: bool,
    pub count: usize,
}

/// Consecutive keyword violation.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ConsecutiveViolation {
    pub sentence_start: usize,
    pub sentence_count: usize,
}

/// Keyword density analysis report.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct KeywordDensityReport {
    pub keyword: String,
    pub total_words: usize,
    pub keyword_count: usize,
    pub density_percent: f64,
    pub is_stuffed: bool,       // density > 3%
    pub is_underused: bool,     // density < 0.5%
    pub section_distribution: Vec<SectionPresence>,
    pub consecutive_violations: Vec<ConsecutiveViolation>,
}

/// Analyze keyword density in content.
pub fn analyze_keyword_density(content: &str, keyword: &str) -> KeywordDensityReport {
    let cleaned = clean_content_for_analysis(content);
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    let total_words = words.len();
    
    // Count keyword occurrences
    let keyword_lower = keyword.to_lowercase();
    let keyword_count = words.iter()
        .filter(|w| w.to_lowercase().contains(&keyword_lower))
        .count();
    
    // Calculate density
    let density_percent = if total_words > 0 {
        (keyword_count as f64 / total_words as f64) * 100.0
    } else {
        0.0
    };
    
    // Check thresholds
    let is_stuffed = density_percent > 3.0;
    let is_underused = density_percent < 0.5;
    
    // Analyze section distribution
    let section_distribution = analyze_section_distribution(&cleaned, &keyword_lower);
    
    // Check for consecutive sentence violations
    let consecutive_violations = detect_consecutive_violations(&cleaned, &keyword_lower);
    
    KeywordDensityReport {
        keyword: keyword.to_string(),
        total_words,
        keyword_count,
        density_percent,
        is_stuffed,
        is_underused,
        section_distribution,
        consecutive_violations,
    }
}

/// Clean content for analysis.
fn clean_content_for_analysis(content: &str) -> String {
    let mut cleaned = content.to_string();
    
    // Remove code blocks
    cleaned = regex::Regex::new(r"```.*?```")
        .unwrap()
        .replace_all(&cleaned, " ")
        .to_string();
    
    // Remove inline code
    cleaned = regex::Regex::new(r"`[^`]+`")
        .unwrap()
        .replace_all(&cleaned, " ")
        .to_string();
    
    // Remove HTML tags
    cleaned = regex::Regex::new(r"<[^>]+>")
        .unwrap()
        .replace_all(&cleaned, " ")
        .to_string();
    
    // Remove markdown headings
    cleaned = regex::Regex::new(r"^#+\s*")
        .unwrap()
        .replace_all(&cleaned, " ")
        .to_string();
    
    // Normalize whitespace
    cleaned = regex::Regex::new(r"\s+")
        .unwrap()
        .replace_all(&cleaned, " ")
        .to_string();
    
    cleaned.to_lowercase()
}

/// Analyze keyword distribution across sections.
fn analyze_section_distribution(content: &str, keyword: &str) -> Vec<SectionPresence> {
    let mut sections = Vec::new();
    
    // Split content into sentences (filter empty like detect_consecutive_violations)
    let sentences: Vec<&str> = content.split(|c: char| c == '.' || c == '!' || c == '?')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let total_sentences = sentences.len();
    
    if total_sentences == 0 {
        return sections;
    }
    
    // Define sections by sentence position
    let intro_end = (total_sentences as f64 * 0.2).ceil() as usize;
    let body_end = (total_sentences as f64 * 0.8).ceil() as usize;
    
    let intro_sentences = &sentences[..intro_end.min(total_sentences)];
    let body_sentences = &sentences[intro_end..body_end.min(total_sentences)];
    let conclusion_sentences = &sentences[body_end.min(total_sentences)..];
    
    // Count keyword in each section
    let intro_count = count_keyword_in_sentences(intro_sentences, keyword);
    let body_count = count_keyword_in_sentences(body_sentences, keyword);
    let conclusion_count = count_keyword_in_sentences(conclusion_sentences, keyword);
    
    sections.push(SectionPresence {
        section: "intro".to_string(),
        present: intro_count > 0,
        count: intro_count,
    });
    
    sections.push(SectionPresence {
        section: "body".to_string(),
        present: body_count > 0,
        count: body_count,
    });
    
    sections.push(SectionPresence {
        section: "conclusion".to_string(),
        present: conclusion_count > 0,
        count: conclusion_count,
    });
    
    sections
}

/// Count keyword occurrences in sentences.
fn count_keyword_in_sentences(sentences: &[&str], keyword: &str) -> usize {
    sentences.iter()
        .flat_map(|s| s.split_whitespace())
        .filter(|w| w.to_lowercase().contains(keyword))
        .count()
}

/// Detect consecutive sentence keyword violations.
fn detect_consecutive_violations(content: &str, keyword: &str) -> Vec<ConsecutiveViolation> {
    let mut violations = Vec::new();
    let sentences: Vec<&str> = content.split(|c: char| c == '.' || c == '!' || c == '?')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    
    let mut consecutive_count = 0;
    let mut start_idx = 0;
    
    for (idx, sentence) in sentences.iter().enumerate() {
        let has_keyword = sentence.to_lowercase().contains(keyword);
        
        if has_keyword {
            if consecutive_count == 0 {
                start_idx = idx;
            }
            consecutive_count += 1;
            
            // Flag if 3+ consecutive sentences contain keyword
            if consecutive_count == 3 {
                violations.push(ConsecutiveViolation {
                    sentence_start: start_idx,
                    sentence_count: consecutive_count,
                });
            } else if consecutive_count > 3 {
                // Update the last violation
                if let Some(last) = violations.last_mut() {
                    last.sentence_count = consecutive_count;
                }
            }
        } else {
            consecutive_count = 0;
        }
    }
    
    violations
}

/// Check if content has good keyword distribution.
#[allow(dead_code)]
fn has_good_distribution(report: &KeywordDensityReport) -> bool {
    // Should appear in all three sections
    report.section_distribution.iter().all(|s| s.present)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_density_calculation() {
        let content = "SEO is important for websites. Good SEO helps rankings. SEO requires effort.";
        let report = analyze_keyword_density(content, "seo");
        
        assert_eq!(report.keyword_count, 3);
        assert!(report.density_percent > 0.0);
    }

    #[test]
    fn test_stuffed_detection() {
        // Create content with >3% density
        let mut content = String::new();
        for _ in 0..50 {
            content.push_str("seo ");
        }
        content.push_str("other words here");
        
        let report = analyze_keyword_density(&content, "seo");
        assert!(report.is_stuffed);
    }

    #[test]
    fn test_underused_detection() {
        let content = "This is a very long article with many words and topics discussed extensively.";
        let report = analyze_keyword_density(content, "rareword");
        
        assert!(report.is_underused);
        assert_eq!(report.keyword_count, 0);
    }

    #[test]
    fn test_consecutive_violations() {
        let content = "SEO is great. SEO helps sites. SEO boosts rankings. Other topics here.";
        let report = analyze_keyword_density(content, "seo");
        
        assert!(!report.consecutive_violations.is_empty());
    }

    #[test]
    fn test_section_distribution() {
        let content = "Intro with keyword. More intro. Body section continues with keyword here. \
                      More body text. Conclusion with keyword.";
        let report = analyze_keyword_density(content, "keyword");
        
        assert!(has_good_distribution(&report));
    }
}
