//! Extract content from MDX files and other sources

use std::path::Path;

use crate::error::Result;
use crate::social::models::{ContentMetadata, ContentSource};
use crate::models::social::SourceType;

/// Extract content from an MDX article file
pub fn extract_from_article(path: &Path) -> Result<ContentSource> {
    let content = std::fs::read_to_string(path)?;
    
    // Parse frontmatter
    let (frontmatter, body) = parse_frontmatter(&content);
    
    // Extract metadata from frontmatter
    let metadata = ContentMetadata {
        title: frontmatter.get("title").cloned(),
        url_slug: frontmatter.get("slug").cloned(),
        published_date: frontmatter.get("date").cloned(),
        word_count: Some(count_words(&body)),
        ..Default::default()
    };
    
    let source_id = metadata
        .url_slug
        .clone()
        .or_else(|| path.file_stem()?.to_str().map(String::from))
        .unwrap_or_else(|| "unknown".to_string());
    
    Ok(ContentSource {
        source_type: SourceType::Article,
        source_id,
        path: path.to_path_buf(),
        content: body,
        metadata,
    })
}

/// Extract content from a spec document
pub fn extract_from_spec(path: &Path) -> Result<ContentSource> {
    let content = std::fs::read_to_string(path)?;
    
    let source_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("spec")
        .to_string();
    
    Ok(ContentSource {
        source_type: SourceType::Spec,
        source_id,
        path: path.to_path_buf(),
        content,
        metadata: ContentMetadata {
            title: Some("Landing Page Spec".to_string()),
            ..Default::default()
        },
    })
}

/// Parse YAML frontmatter from markdown content
fn parse_frontmatter(content: &str) -> (std::collections::HashMap<String, String>, String) {
    let mut frontmatter = std::collections::HashMap::new();
    
    // Check for --- delimited frontmatter
    if let Some(start) = content.find("---") {
        if let Some(end) = content[start + 3..].find("---") {
            let fm_text = &content[start + 3..start + 3 + end];
            
            // Simple line-by-line parsing (key: value)
            for line in fm_text.lines() {
                if let Some(colon_pos) = line.find(':') {
                    let key = line[..colon_pos].trim().to_string();
                    let value = line[colon_pos + 1..].trim().to_string();
                    frontmatter.insert(key, value);
                }
            }
            
            let body = content[start + 3 + end + 3..].trim().to_string();
            return (frontmatter, body);
        }
    }
    
    // No frontmatter found, return all as body
    (frontmatter, content.to_string())
}

/// Count words in text
fn count_words(text: &str) -> u32 {
    text.split_whitespace().count() as u32
}

/// Extract key points from article content for carousel slides
pub fn extract_key_points(content: &str, max_points: usize) -> Vec<String> {
    let mut points = Vec::new();
    
    // Extract headings as key points
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            let point = trimmed
                .trim_start_matches("## ")
                .trim_start_matches("### ")
                .to_string();
            if !point.is_empty() && points.len() < max_points {
                points.push(point);
            }
        }
    }
    
    // If not enough headings, extract first sentences of paragraphs
    if points.len() < max_points {
        for paragraph in content.split("\n\n") {
            let trimmed = paragraph.trim();
            if !trimmed.starts_with('#') && !trimmed.is_empty() {
                if let Some(sentence_end) = trimmed.find('.') {
                    let sentence = &trimmed[..sentence_end + 1];
                    if sentence.len() > 20 && points.len() < max_points {
                        points.push(sentence.to_string());
                    }
                }
            }
        }
    }
    
    points
}
