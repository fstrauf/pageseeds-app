//! Internal models for the social module
//!
//! These types are used internally and may differ from the
//! public API models in `crate::models::social`.

use serde::{Deserialize, Serialize};

/// Content source discovered in a project
#[derive(Debug, Clone)]
pub struct ContentSource {
    pub source_type: crate::models::social::SourceType,
    pub source_id: String,
    pub path: std::path::PathBuf,
    pub content: String,
    pub metadata: ContentMetadata,
}

impl ContentSource {
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

/// Metadata for content sources
#[derive(Debug, Clone, Default)]
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
    pub template: crate::models::social::ContentTemplate,
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
#[derive(Debug, Clone, Deserialize)]
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
