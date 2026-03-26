//! Internal models for the social module
//!
//! These types are used internally and may differ from the
//! public API models in `crate::models::social`.

use serde::{Deserialize, Serialize};

/// Content source discovered in a project
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContentSource {
    pub source_type: crate::models::social::SourceType,
    pub source_id: String,
    pub path: std::path::PathBuf,
    pub content: String,
    pub metadata: ContentMetadata,
    /// Engagement score (0-100), computed deterministically
    pub engagement_score: u32,
    /// Content themes/tags for clustering
    pub themes: Vec<String>,
    /// Suggested template type based on content
    pub suggested_template: String,
    /// Best platform for this content
    pub suggested_platform: crate::models::social::Platform,
}

impl ContentSource {
    /// Create a new content source with computed scores
    pub fn new(
        source_type: crate::models::social::SourceType,
        source_id: String,
        path: std::path::PathBuf,
        content: String,
        metadata: ContentMetadata,
    ) -> Self {
        let engagement_score = compute_engagement_score(&source_type, &metadata, &content);
        let themes = extract_themes(&content);
        let suggested_template = suggest_template(&source_type, &content);
        let suggested_platform = suggest_platform(&source_type, &content);
        
        Self {
            source_type,
            source_id,
            path,
            content,
            metadata,
            engagement_score,
            themes,
            suggested_template,
            suggested_platform,
        }
    }
    
    /// Generate a summary of this content for agent prompts
    pub fn content_summary(&self) -> String {
        match self.source_type {
            crate::models::social::SourceType::Article => {
                format!(
                    "Title: {}\nExcerpt: {}",
                    self.metadata.title.as_deref().unwrap_or("Untitled"),
                    &self.content.chars().take(500).collect::<String>()
                )
            }
            crate::models::social::SourceType::Screenshot => {
                format!(
                    "Screenshot: {}\nDescription: {}",
                    self.source_id,
                    self.metadata.description.as_deref().unwrap_or("App screenshot")
                )
            }
            crate::models::social::SourceType::Spec => {
                format!(
                    "Spec: {}\nContent: {}",
                    self.source_id,
                    &self.content.chars().take(500).collect::<String>()
                )
            }
        }
    }
}

/// Compute engagement score (0-100) deterministically
/// Formula: visual_appeal × 0.4 + educational_value × 0.3 + recency × 0.2 + uniqueness × 0.1
fn compute_engagement_score(
    source_type: &crate::models::social::SourceType,
    metadata: &ContentMetadata,
    content: &str,
) -> u32 {
    // Visual appeal: screenshots score high, text lower
    let visual_appeal = match source_type {
        crate::models::social::SourceType::Screenshot => 90u32,
        crate::models::social::SourceType::Article => 40,
        crate::models::social::SourceType::Spec => 30,
    };
    
    // Educational value: based on content length and structure
    let word_count = metadata.word_count.unwrap_or(0);
    let educational_value = if word_count > 500 {
        80
    } else if word_count > 200 {
        60
    } else {
        40
    };
    
    // Recency: default to middle score (can't easily get file mod time here)
    let recency = 50u32;
    
    // Uniqueness: based on content diversity signals
    let has_headings = content.contains("##") || content.contains("###");
    let has_lists = content.contains("- ") || content.contains("1.");
    let uniqueness = if has_headings && has_lists { 80 } else { 50 };
    
    // Weighted average
    let score = (visual_appeal * 40 
        + educational_value * 30 
        + recency * 20 
        + uniqueness * 10) / 100;
    
    score.min(100)
}

/// Extract themes from content (deterministic keyword matching)
fn extract_themes(content: &str) -> Vec<String> {
    let content_lower = content.to_lowercase();
    let mut themes = Vec::new();
    
    let theme_keywords: Vec<(&str, &[&str])> = vec![
        ("seo", &["seo", "search", "ranking", "google", "keywords"]),
        ("automation", &["automation", "automated", "workflow", "task", "queue"]),
        ("content", &["content", "article", "blog", "writing", "mdx"]),
        ("technical", &["rust", "tauri", "architecture", "performance"]),
        ("analytics", &["analytics", "metrics", "gsc", "search console", "data"]),
        ("reddit", &["reddit", "community", "engagement", "opportunities"]),
    ];
    
    for (theme, keywords) in theme_keywords {
        if keywords.iter().any(|kw| content_lower.contains(kw)) {
            themes.push(theme.to_string());
        }
    }
    
    themes
}

/// Suggest template based on content type
fn suggest_template(
    source_type: &crate::models::social::SourceType,
    _content: &str,
) -> String {
    match source_type {
        crate::models::social::SourceType::Screenshot => "feature_hook".to_string(),
        crate::models::social::SourceType::Article => "educational_carousel".to_string(),
        crate::models::social::SourceType::Spec => "technical_explainer".to_string(),
    }
}

/// Suggest platform based on content type
fn suggest_platform(
    source_type: &crate::models::social::SourceType,
    _content: &str,
) -> crate::models::social::Platform {
    use crate::models::social::Platform;
    match source_type {
        crate::models::social::SourceType::Screenshot => Platform::TikTok,
        crate::models::social::SourceType::Article => Platform::InstagramFeed,
        crate::models::social::SourceType::Spec => Platform::InstagramFeed,
    }
}

/// Metadata for content sources
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ContentMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub url_slug: Option<String>,
    pub published_date: Option<String>,
    pub word_count: Option<u32>,
}

/// A content source manifest collected before generation
#[derive(Debug, Clone)]
pub struct SourceManifest {
    pub articles: Vec<ContentSource>,
    pub screenshots: Vec<ContentSource>,
    pub specs: Vec<ContentSource>,
}

impl SourceManifest {
    pub fn is_empty(&self) -> bool {
        self.articles.is_empty() && self.screenshots.is_empty() && self.specs.is_empty()
    }

    pub fn total_sources(&self) -> usize {
        self.articles.len() + self.screenshots.len() + self.specs.len()
    }

    /// Iterate over all sources
    pub fn all_sources(&self) -> impl Iterator<Item = &ContentSource> {
        self.articles
            .iter()
            .chain(self.screenshots.iter())
            .chain(self.specs.iter())
    }
}

/// Generation job for a single post
#[derive(Debug, Clone)]
pub struct PostGenerationJob {
    pub source: ContentSource,
    pub template: crate::social::templates::TemplateDef,
    pub platform: crate::models::social::Platform,
}

/// Result of a post generation attempt
#[derive(Debug, Clone)]
pub struct GenerationResult {
    pub success: bool,
    pub post: Option<crate::models::social::SocialPost>,
    pub error: Option<String>,
}

/// Agent response for post generation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPostOutput {
    pub hook: String,
    pub caption: String,
    pub hashtags: Vec<String>,
    pub cta: String,
    pub visual_description: String,
    pub overlay_text: Option<String>,
}

/// Agent response for template creation
#[derive(Debug, Clone, Deserialize)]
pub struct AgentTemplateOutput {
    pub creation_prompt: String,
    pub overlay_config: crate::models::social::OverlayConfig,
    pub default_hashtags: Vec<String>,
    pub example: TemplateExampleOutput,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TemplateExampleOutput {
    pub hook: String,
    pub caption: String,
    pub visual_description: String,
}

/// Configuration for the social module
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SocialConfig {
    /// Directory for generated social media assets (relative to project)
    pub output_dir: String,
    /// Default platforms to target
    pub default_platforms: Vec<crate::models::social::Platform>,
    /// Maximum posts per campaign
    pub max_posts_per_campaign: u32,
}

impl Default for SocialConfig {
    fn default() -> Self {
        Self {
            output_dir: ".github/automation/social".to_string(),
            default_platforms: vec![
                crate::models::social::Platform::InstagramFeed,
                crate::models::social::Platform::TikTok,
            ],
            max_posts_per_campaign: 50,
        }
    }
}
