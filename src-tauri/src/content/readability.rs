use serde::{Deserialize, Serialize};
use ts_rs::TS;
use crate::error::Result;
use regex::Regex;
use once_cell::sync::Lazy;

// Compiled regex patterns for MDX cleaning
static IMPORT_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new("(?m)^import\\s+.*?from\\s+['\"].*?['\"];?\\s*$").unwrap()
});
static JSX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[A-Z][^>]*>.*?</[A-Z][^>]*>").unwrap());
static SELF_CLOSING_JSX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[A-Z][^>]*/>").unwrap());
static HTML_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());
static MD_LINKS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\[([^\]]+)\]\([^)]+\)").unwrap());
static MD_IMAGES_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"!\[[^\]]*\]\([^)]+\)").unwrap());
static WHITESPACE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

/// Readability analysis report for an article.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ReadabilityReport {
    pub flesch_reading_ease: f64,
    pub flesch_kincaid_grade: f64,
    pub smog_index: f64,
    pub coleman_liau_index: f64,
    pub automated_readability_index: f64,
    pub passive_voice_percentage: f64,
    pub sentence_variety_score: f64,
    pub avg_sentence_length: f64,
    pub cliche_count: usize,
    pub filter_word_percentage: f64,
}

/// Analyze readability of text content.
pub fn analyze_readability(text: &str) -> Result<ReadabilityReport> {
    let result = writing_analysis::analyze_all(text)
        .map_err(|e| crate::error::Error::Other(format!("Readability analysis failed: {e}")))?;
    
    Ok(ReadabilityReport {
        flesch_reading_ease: result.readability.flesch_reading_ease,
        flesch_kincaid_grade: result.readability.flesch_kincaid_grade,
        smog_index: result.readability.smog_index,
        coleman_liau_index: result.readability.coleman_liau_index,
        automated_readability_index: result.readability.automated_readability_index,
        passive_voice_percentage: result.passive_voice.percentage,
        sentence_variety_score: result.sentence_variety.structure_variety,
        avg_sentence_length: result.sentence_variety.avg_length,
        cliche_count: result.cliches.count,
        filter_word_percentage: result.filter_words.percentage,
    })
}

/// Strip MDX/JSX components from text for readability analysis.
pub fn clean_mdx_for_readability(content: &str) -> String {
    let mut cleaned = content.to_string();
    
    // Remove import statements
    cleaned = IMPORT_RE.replace_all(&cleaned, "").to_string();
    
    // Remove JSX components (tags starting with capital letters)
    cleaned = JSX_RE.replace_all(&cleaned, "").to_string();
    
    // Remove self-closing JSX components
    cleaned = SELF_CLOSING_JSX_RE.replace_all(&cleaned, "").to_string();
    
    // Remove HTML tags
    cleaned = HTML_RE.replace_all(&cleaned, "").to_string();
    
    // Remove markdown links, keeping the text
    cleaned = MD_LINKS_RE.replace_all(&cleaned, "$1").to_string();
    
    // Remove markdown images
    cleaned = MD_IMAGES_RE.replace_all(&cleaned, "").to_string();
    
    // Normalize whitespace
    cleaned = WHITESPACE_RE.replace_all(&cleaned, " ").to_string();
    
    cleaned.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_mdx_for_readability() {
        let input = r#"---
title: "Test Article"
---

import { Component } from './components'

# Hello World

This is a paragraph with <Component>nested content</Component> inside.

![alt text](/image.png)

[Link text](https://example.com)

More text here."#;

        let cleaned = clean_mdx_for_readability(input);
        
        assert!(!cleaned.contains("import"));
        assert!(!cleaned.contains("<Component>"));
        assert!(!cleaned.contains("![alt text]"));
        assert!(cleaned.contains("Hello World"));
        assert!(cleaned.contains("Link text"));
        assert!(cleaned.contains("More text here"));
    }

    #[test]
    fn test_analyze_readability() {
        let text = "The quick brown fox jumps over the lazy dog. This is a simple sentence for testing readability analysis.";
        
        let report = analyze_readability(text).expect("Should analyze readability");
        
        // Basic sanity checks
        assert!(report.flesch_reading_ease > 0.0);
        assert!(report.flesch_kincaid_grade >= 0.0);
        assert!(report.avg_sentence_length > 0.0);
    }
}
