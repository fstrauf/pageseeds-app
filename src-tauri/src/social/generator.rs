//! Simple social media post generator from articles
//!
//! Agentic workflow: Takes articles → generates social posts

use std::path::Path;

use crate::error::Result;
use crate::models::article::Article;
use crate::models::social::{SocialPost, PostStatus, Platform, PostFormat};
use crate::social::db;
use crate::social::image::assets::resolve_post_images;

/// Generate social media posts from articles
/// 
/// This is an AGENTIC step - the LLM transforms article content into
/// platform-native social media posts with hooks, captions, and hashtags.
pub async fn generate_posts_from_articles(
    campaign_id: &str,
    project_id: &str,
    articles: &[Article],
    platforms: &[Platform],
    agent_provider: &str,
    project_path: &Path,
    output_dir: &Path,
) -> Result<Vec<SocialPost>> {
    let mut posts = Vec::new();
    
    for article in articles {
        for platform in platforms {
            // Build prompt for this article + platform combo
            let prompt = build_post_prompt(article, platform);
            
            // Call agent
            match crate::engine::agent::run_agent(agent_provider, &prompt, Path::new(".")).await {
                Ok(output) => {
                    if let Ok(post) = parse_post_output(&output, campaign_id, project_id, article, platform, project_path, output_dir) {
                        posts.push(post);
                    }
                }
                Err(e) => {
                    log::warn!("Failed to generate post for article {}: {}", article.url_slug, e);
                }
            }
        }
    }
    
    Ok(posts)
}

/// Build the agent prompt for generating a social post
fn build_post_prompt(article: &Article, platform: &Platform) -> String {
    let platform_guidance = match platform {
        Platform::TikTok => "TikTok: Short, punchy, casual. Hook in first 3 seconds. Use trending language.",
        Platform::InstagramFeed => "Instagram Feed: Polished but authentic. Can be longer. Use emojis as bullets.",
        Platform::InstagramReel => "Instagram Reels: Fast-paced like TikTok but slightly more polished.",
        Platform::InstagramStory => "Instagram Stories: Quick, ephemeral, casual. Good for behind-the-scenes.",
    };
    
    format!(
        r##"Create a {} post from this article.

## Article
Title: {}
URL Slug: {}
Target Keyword: {}

## Platform Guidance
{}

## Your Task
Write a scroll-stopping social media post that drives traffic to this article.

## Output (JSON only)
```json
{{
  "hook": "First line that stops the scroll (max 100 chars)",
  "caption": "Main text without hashtags (engaging, conversational)",
  "hashtags": ["#relevant", "#hashtags", "#max5"],
  "cta": "Soft call to action (e.g., 'Link in bio', 'Read more')",
  "visual_description": "Describe what image/video would work best"
}}
```

Rules:
- Hook must be scroll-stopping (curiosity gap, contrarian, or benefit-driven)
- Lead with value, not "Check out this article"
- Match the platform's tone and format
- Hashtags: 3-5 relevant ones (mix of niche and broad)
- CTA should be natural, not pushy"##,
        platform,
        article.title,
        article.url_slug,
        article.target_keyword.as_deref().unwrap_or(""),
        platform_guidance
    )
}

/// Parse agent output into a SocialPost
fn parse_post_output(
    output: &str,
    campaign_id: &str,
    project_id: &str,
    article: &Article,
    platform: &Platform,
    project_path: &Path,
    output_dir: &Path,
) -> Result<SocialPost> {
    // Extract JSON from output
    let json_str = extract_json_block(output)?;
    let parsed: serde_json::Value = serde_json::from_str(&json_str)?;
    
    let hook = parsed["hook"].as_str().unwrap_or(&article.title).to_string();
    let caption = parsed["caption"].as_str().unwrap_or("").to_string();
    let hashtags: Vec<String> = parsed["hashtags"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let cta = parsed["cta"].as_str().unwrap_or("Link in bio").to_string();
    
    // Resolve images for this post
    let visual_assets = resolve_post_images(article, project_path, output_dir, &hook)?;
    
    let post_id = format!("post_{}_{}", article.url_slug, platform.as_str());
    
    Ok(SocialPost {
        id: post_id,
        campaign_id: campaign_id.to_string(),
        project_id: project_id.to_string(),
        source_type: crate::models::social::SourceType::Article,
        source_id: article.url_slug.clone(),
        source_url: Some(article.url_slug.clone()),
        platform: platform.clone(),
        format: PostFormat::SingleImage,
        hook,
        caption,
        hashtags,
        cta,
        visual_assets,
        status: PostStatus::Draft,
        scheduled_at: None,
        posted_at: None,
        platform_post_id: None,
        platform_post_url: None,
        metrics: None,
        template_id: "article_to_social".to_string(),
        generated_by: Some("kimi".to_string()),
        generation_prompt_hash: None,
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Extract JSON block from agent output
fn extract_json_block(output: &str) -> Result<String> {
    // Find JSON block between ```json and ```
    if let Some(start) = output.find("```json") {
        let after_start = &output[start + 7..];
        if let Some(end) = after_start.find("```") {
            return Ok(after_start[..end].trim().to_string());
        }
    }
    
    // Try just finding { and }
    if let Some(start) = output.find('{') {
        if let Some(end) = output.rfind('}') {
            return Ok(output[start..=end].to_string());
        }
    }
    
    Err(crate::error::Error::Other("No JSON found in output".to_string()))
}

/// Image generation prompt for external AI services
pub fn generate_image_prompt(post: &SocialPost) -> String {
    format!(
        "Create a social media image for this post:\n\nHook: {}\nPlatform: {:?}\n\nStyle: Modern, minimalist, professional. Colors: Forest green, warm clay, golden seed. No text in image. Abstract concept related to SEO/content/automation.",
        post.hook,
        post.platform
    )
}
