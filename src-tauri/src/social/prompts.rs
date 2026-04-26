#![allow(dead_code)]
//! Agent prompts for social media content generation

use crate::models::social::*;
use crate::social::models::ContentSource;

/// Generate a caption/hook for a social media post
pub fn generate_post_prompt(
    source: &ContentSource,
    template: &ContentTemplate,
    platform: &Platform,
    project_context: &str,
) -> String {
    let platform_guidance = platform_guidance(platform);
    let canvas_dimensions = canvas_size_description(&template.overlay_config.canvas_size);
    
    format!(
        r##"You are a social media content strategist. Create a {platform} post.

## Source Content
Type: {source_type}
{source_content}

## Template Instructions
{template_instructions}

## Platform: {platform}
{platform_guidance}

## Project Context
{project_context}

## Output Contract (Required)
Return ONLY one fenced JSON block and no extra prose:

```json
{{
  "hook": "Scroll-stopping first line (max 100 chars)",
  "caption": "Full caption text without hashtags",
  "hashtags": ["#relevant", "#niche", "#broad"],
  "cta": "Clear call to action",
  "visual_description": "What the image or video should show",
  "overlay_text": "Text to render on the image (optional, max 20 words)",
  "image_generation_prompt": "Detailed AI image generation prompt (200-400 chars)"
}}
```

Requirements:
- Hook must grab attention in 3 seconds
- Caption should provide value or education
- Hashtags: 5-10 relevant tags (mix of broad and niche)
- CTA: soft, not pushy
- overlay_text should be punchy and fit on an image
- image_generation_prompt: Create a detailed prompt for AI image generators (Midjourney/DALL-E/Leonardo)
  * Describe the visual style, composition, colors, mood
  * Specify "no text in image" (text will be overlaid separately)
  * Include aspect ratio hint: {canvas_dimensions}
  * Keep to 200-400 characters for optimal results
  * Make it specific enough to get consistent, on-brand results
"##,
        platform = format!("{:?}", platform),
        source_type = format!("{:?}", source.source_type),
        source_content = source.content_summary(),
        template_instructions = template.creation_prompt,
        platform_guidance = platform_guidance,
        project_context = project_context,
        canvas_dimensions = canvas_dimensions,
    )
}

/// Prompt for creating a new content template
pub fn create_template_prompt(
    name: &str,
    platform: &Platform,
    format: &PostFormat,
    description: &str,
) -> String {
    format!(
        r##"Design a social media content template.

## Template Request
Name: {name}
Platform: {platform}
Format: {format}
Description: {description}

## Output Contract (Required)
Return ONLY one fenced JSON block:

```json
{{
  "creation_prompt": "Detailed instructions for how to transform source content into a post. Be specific about tone, structure, what to emphasize, length, and style. Include examples of good hooks for this format.",
  "overlay_config": {{
    "canvas_size": "TikTok or Square or Portrait or Story",
    "font_family": "Suggested font family (e.g., Inter, SF Pro, Roboto)",
    "primary_color": "#FFFFFF",
    "secondary_color": "#000000",
    "text_position": "top or center or bottom",
    "max_text_length": 100
  }},
  "default_hashtags": ["#example1", "#example2"],
  "example": {{
    "hook": "Example hook that follows your template",
    "caption": "Example caption",
    "visual_description": "Description of example visual"
  }}
}}
```

Make the template practical and reusable across different content sources.
"##,
        name = name,
        platform = format!("{:?}", platform),
        format = format!("{:?}", format),
        description = description,
    )
}

/// Prompt for regenerating a post with feedback
pub fn regenerate_post_prompt(
    original_post: &SocialPost,
    feedback: &str,
    project_context: &str,
) -> String {
    format!(
        r##"You are a social media content strategist. Regenerate this post based on feedback.

## Original Post
Hook: {hook}
Caption: {caption}
Hashtags: {hashtags}
CTA: {cta}

## Feedback
{feedback}

## Project Context
{project_context}

## Output Contract (Required)
Return ONLY one fenced JSON block:

```json
{{
  "hook": "Improved scroll-stopping first line",
  "caption": "Revised caption text without hashtags",
  "hashtags": ["#revised", "#hashtags"],
  "cta": "Revised call to action",
  "visual_description": "Updated visual description",
  "overlay_text": "Updated overlay text (optional)",
  "image_generation_prompt": "Updated AI image generation prompt (200-400 chars)"
}}
```

Address the feedback while maintaining the core message.
If the feedback relates to the visual, update the image_generation_prompt accordingly.
"##,
        hook = original_post.hook,
        caption = original_post.caption,
        hashtags = original_post.hashtags.join(", "),
        cta = original_post.cta,
        feedback = feedback,
        project_context = project_context,
    )
}

fn platform_guidance(platform: &Platform) -> &'static str {
    match platform {
        Platform::TikTok => {
            "Vertical 9:16 format. Fast-paced, casual, conversational tone. \
             Short punchy sentences. Hook in first 3 seconds. \
             Use line breaks for readability. Emojis welcome. \
             Trending sounds or stickers implied but not mentioned."
        }
        Platform::InstagramFeed => {
            "Square (1:1) or Portrait (4:5) format. Polished but authentic. \
             Can be longer form (up to 500 chars). First line is the hook. \
             Use emojis as bullet points. 5-10 hashtags at end. \
             More professional than TikTok but not corporate."
        }
        Platform::InstagramReel => {
            "Vertical 9:16 format. Similar energy to TikTok but slightly more polished. \
             Fast cuts implied. Text-on-screen style. \
             Hook must be immediate. Trending audio implied."
        }
        Platform::InstagramStory => {
            "Vertical 9:16 format. Quick, ephemeral feel. \
             Interactive elements (polls, questions) implied. \
             More casual than Feed. Good for behind-the-scenes."
        }
    }
}

/// Get canvas size description for image generation prompts
fn canvas_size_description(canvas_size: &CanvasSize) -> &'static str {
    match canvas_size {
        CanvasSize::TikTok => "9:16 vertical (1080x1920) - TikTok/Reels format",
        CanvasSize::Square => "1:1 square (1080x1080) - Instagram Feed",
        CanvasSize::Portrait => "4:5 portrait (1080x1350) - Instagram Feed portrait",
        CanvasSize::Story => "9:16 vertical (1080x1920) - Stories format",
    }
}

/// Get default project context for prompts
pub fn default_project_context(site_url: &str) -> String {
    format!(
        "This content is for a project with website: {}. \
         Focus on driving engagement and traffic back to the site. \
         Maintain a helpful, educational tone.",
        site_url
    )
}
