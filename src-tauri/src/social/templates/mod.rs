//! Template system for social media content generation
//!
//! This module provides a hybrid deterministic/agentic approach:
//! - Template selection is deterministic (rule-based)
//! - Content transformation is agentic (creative writing)

use crate::models::social::{Platform, PostFormat, SourceType};
use crate::social::models::ContentSource;

/// A content template defines how to transform source content into a social post
#[derive(Debug, Clone)]
pub struct ContentTemplate {
    /// Template identifier
    pub id: String,
    /// Template identifier
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Description of when to use this template
    pub description: String,
    /// Target platform
    pub platform: Platform,
    /// Post format
    pub format: PostFormat,
    /// The prompt template for the agent
    pub prompt_template: String,
    /// Output schema validation
    pub output_schema: OutputSchema,
    /// Ideal content source types for this template
    pub source_types: Vec<SourceType>,
    /// Engagement score weight (0-100)
    pub engagement_weight: u32,
}

/// Output schema for validation
#[derive(Debug, Clone)]
pub struct OutputSchema {
    pub required_fields: Vec<String>,
    pub max_hook_length: usize,
    pub max_caption_length: usize,
    pub hashtag_count_range: (usize, usize),
}

/// Template registry - all available templates
pub struct TemplateRegistry {
    pub templates: Vec<ContentTemplate>,
}

impl TemplateRegistry {
    /// Create a new registry with default templates
    pub fn default() -> Self {
        Self {
            templates: vec![
                Self::feature_hook_template(),
                Self::educational_carousel_template(),
                Self::technical_explainer_template(),
                Self::quick_tip_template(),
                Self::behind_scenes_template(),
            ],
        }
    }

    /// Select the best template for a content source (deterministic)
    pub fn select_template(&self, source: &ContentSource) -> Option<&ContentTemplate> {
        // First, try to match by suggested template
        let suggested = self.templates.iter()
            .find(|t| t.id == source.suggested_template);
        
        if suggested.is_some() {
            return suggested;
        }
        
        // Fallback: score all templates and pick best match
        self.templates.iter()
            .filter(|t| t.source_types.contains(&source.source_type))
            .max_by_key(|t| {
                // Score based on:
                // 1. Source type match (already filtered)
                // 2. Engagement weight alignment with source score
                // 3. Platform match with suggested platform
                let platform_bonus = if t.platform == source.suggested_platform { 20 } else { 0 };
                t.engagement_weight + platform_bonus
            })
    }

    /// Get all templates for a specific platform
    pub fn for_platform(&self, platform: Platform) -> Vec<&ContentTemplate> {
        self.templates.iter()
            .filter(|t| t.platform == platform)
            .collect()
    }

    /// Get template by ID
    pub fn get(&self, id: &str) -> Option<&ContentTemplate> {
        self.templates.iter().find(|t| t.id == id)
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // Template Definitions
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Template: Feature Hook (TikTok/Reels short-form)
    fn feature_hook_template() -> ContentTemplate {
        ContentTemplate {
            id: "feature_hook".to_string(),
            name: "Feature Hook".to_string(),
            description: "Scroll-stopping hook showcasing a feature".to_string(),
            platform: Platform::TikTok,
            format: PostFormat::SingleImage,
            prompt_template: feature_hook_prompt(),
            output_schema: OutputSchema {
                required_fields: vec!["hook".to_string(), "caption".to_string(), 
                    "hashtags".to_string(), "cta".to_string(), "overlay_text".to_string()],
                max_hook_length: 100,
                max_caption_length: 300,
                hashtag_count_range: (3, 8),
            },
            source_types: vec![SourceType::Screenshot],
            engagement_weight: 90,
        }
    }

    /// Template: Educational Carousel (Instagram Feed)
    fn educational_carousel_template() -> ContentTemplate {
        ContentTemplate {
            id: "educational_carousel".to_string(),
            name: "Educational Carousel".to_string(),
            description: "Multi-slide educational content".to_string(),
            platform: Platform::InstagramFeed,
            format: PostFormat::Carousel,
            prompt_template: educational_carousel_prompt(),
            output_schema: OutputSchema {
                required_fields: vec!["slides".to_string(), "caption".to_string(), 
                    "hashtags".to_string(), "cta".to_string()],
                max_hook_length: 150,
                max_caption_length: 500,
                hashtag_count_range: (5, 10),
            },
            source_types: vec![SourceType::Article, SourceType::Spec],
            engagement_weight: 80,
        }
    }

    /// Template: Technical Explainer (Instagram Feed)
    fn technical_explainer_template() -> ContentTemplate {
        ContentTemplate {
            id: "technical_explainer".to_string(),
            name: "Technical Explainer".to_string(),
            description: "Explain technical concepts with single image".to_string(),
            platform: Platform::InstagramFeed,
            format: PostFormat::SingleImage,
            prompt_template: technical_explainer_prompt(),
            output_schema: OutputSchema {
                required_fields: vec!["hook".to_string(), "caption".to_string(), 
                    "hashtags".to_string(), "cta".to_string()],
                max_hook_length: 150,
                max_caption_length: 800,
                hashtag_count_range: (5, 10),
            },
            source_types: vec![SourceType::Spec, SourceType::Article],
            engagement_weight: 70,
        }
    }

    /// Template: Quick Tip (TikTok/Reels)
    fn quick_tip_template() -> ContentTemplate {
        ContentTemplate {
            id: "quick_tip".to_string(),
            name: "Quick Tip".to_string(),
            description: "Fast, actionable SEO tip".to_string(),
            platform: Platform::TikTok,
            format: PostFormat::SingleImage,
            prompt_template: quick_tip_prompt(),
            output_schema: OutputSchema {
                required_fields: vec!["hook".to_string(), "caption".to_string(), 
                    "hashtags".to_string(), "cta".to_string(), "overlay_text".to_string()],
                max_hook_length: 80,
                max_caption_length: 200,
                hashtag_count_range: (3, 6),
            },
            source_types: vec![SourceType::Article, SourceType::Screenshot],
            engagement_weight: 85,
        }
    }

    /// Template: Behind the Scenes (Instagram Reels/Stories)
    fn behind_scenes_template() -> ContentTemplate {
        ContentTemplate {
            id: "behind_scenes".to_string(),
            name: "Behind the Scenes".to_string(),
            description: "Build-in-public style content".to_string(),
            platform: Platform::InstagramReel,
            format: PostFormat::SingleImage,
            prompt_template: behind_scenes_prompt(),
            output_schema: OutputSchema {
                required_fields: vec!["hook".to_string(), "caption".to_string(), 
                    "hashtags".to_string(), "cta".to_string()],
                max_hook_length: 120,
                max_caption_length: 400,
                hashtag_count_range: (3, 8),
            },
            source_types: vec![SourceType::Screenshot, SourceType::Spec],
            engagement_weight: 75,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Prompt Templates (Agentic Steps)
// ═══════════════════════════════════════════════════════════════════════════════

fn feature_hook_prompt() -> String {
    r##"You are a TikTok content strategist. Create a scroll-stopping post.

## Source
{source_summary}

## Task
Create a TikTok-style post that showcases this feature in 3 seconds.

## Rules
1. Hook must stop the scroll in ≤ 8 words
2. Use Problem → Solution structure
3. Show, don't tell (reference the visual)
4. CTA should be soft (not salesy)
5. Tone: energetic, casual, confident

## Output Contract (JSON only)
```json
{
  "hook": "Stop doing X (do Y instead)",
  "caption": "Brief explanation with personality",
  "hashtags": ["#seo", "#productivity", "#indiehackers"],
  "cta": "Link in bio to try it",
  "overlay_text": "Text for image overlay (≤ 6 words)",
  "visual_description": "What to show in the video/image"
}
```

Requirements:
- Hook: punchy, curiosity-gap or contrarian
- Caption: max 2 sentences, conversational
- Hashtags: mix of niche (#pageseeds) and broad (#seo)
- Overlay text: big, bold, readable at glance"##.to_string()
}

fn educational_carousel_prompt() -> String {
    r##"You are an Instagram educator. Transform this content into a carousel.

## Source
{source_summary}

## Task
Create a 5-slide Instagram carousel that teaches one key concept.

## Rules
1. Slide 1: Hook + problem (make them save it)
2. Slides 2-4: One insight per slide (actionable)
3. Slide 5: CTA + benefit
4. Each slide: 1 sentence max, big text
5. Progressive disclosure: don't give everything on slide 1

## Output Contract (JSON only)
```json
{
  "slides": [
    {"text": "Most people do X wrong", "visual": "Problem visualization"},
    {"text": "Here's the better way", "visual": "Solution preview"},
    {"text": "Step 1: Do this", "visual": "Action screenshot"},
    {"text": "Step 2: Then this", "visual": "Result screenshot"},
    {"text": "Save this + follow for more", "visual": "Branded outro"}
  ],
  "caption": "Longer-form caption with context",
  "hashtags": ["#seotips", "#contentstrategy", "#digitalmarketing"],
  "cta": "Follow for daily SEO tips"
}
```

Requirements:
- Each slide should work standalone
- Text must be readable on mobile
- Visual descriptions should reference actual UI/screenshots"##.to_string()
}

fn technical_explainer_prompt() -> String {
    r##"You are a technical writer for Instagram. Explain a concept clearly.

## Source
{source_summary}

## Task
Explain this technical concept in a single Instagram post.

## Rules
1. First line: hook that mentions the benefit
2. Body: Explain like I'm 5 (ELI5 style)
3. Use analogies and examples
4. No jargon without explanation
5. End with "how to apply this"

## Output Contract (JSON only)
```json
{
  "hook": "Why we built our own task queue (and you should too)",
  "caption": "Full explanation with formatting...",
  "hashtags": ["#rustlang", "#buildinpublic", "#systemdesign"],
  "cta": "What's your take? Comment below",
  "key_takeaway": "One-sentence summary for image overlay"
}
```

Requirements:
- Caption: 150-400 characters optimal
- Use line breaks for readability
- Position as educational, not promotional"##.to_string()
}

fn quick_tip_prompt() -> String {
    r##"You are a short-form content creator. Share a quick SEO tip.

## Source
{source_summary}

## Task
Create a 15-second worth of content tip.

## Rules
1. Hook: "This one thing changed my SEO"
2. Tip must be actionable in 1 sentence
3. Show before/after or process
4. Make it feel like a secret

## Output Contract (JSON only)
```json
{
  "hook": "The SEO hack nobody talks about",
  "caption": "The tip + why it works",
  "hashtags": ["#seohack", "#quicktip"],
  "cta": "Follow for more",
  "overlay_text": "Do this → Get that"
}
```"##.to_string()
}

fn behind_scenes_prompt() -> String {
    r##"You are a build-in-public creator. Share your process.

## Source
{source_summary}

## Task
Create authentic behind-the-scenes content.

## Rules
1. Show the mess, not just the success
2. Share a specific decision or challenge
3. Ask for community input
4. Humanize the technical stuff

## Output Contract (JSON only)
```json
{
  "hook": "What went wrong this week",
  "caption": "The story + lesson learned",
  "hashtags": ["#buildinpublic", "#indiehackers"],
  "cta": "What would you do?"
}
```"##.to_string()
}

/// Render a prompt template with source context
pub fn render_prompt(template: &ContentTemplate, source: &ContentSource) -> String {
    template.prompt_template.replace("{source_summary}", &source.content_summary())
}

/// Validate agent output against template schema
pub fn validate_output(template: &ContentTemplate, output: &serde_json::Value) -> Vec<String> {
    let mut errors = Vec::new();
    
    // Check required fields
    for field in &template.output_schema.required_fields {
        if output.get(field).is_none() {
            errors.push(format!("Missing required field: {}", field));
        }
    }
    
    // Check hook length
    if let Some(hook) = output.get("hook").and_then(|v| v.as_str()) {
        if hook.len() > template.output_schema.max_hook_length {
            errors.push(format!(
                "Hook too long: {} > {} chars", 
                hook.len(), 
                template.output_schema.max_hook_length
            ));
        }
    }
    
    // Check hashtag count
    if let Some(hashtags) = output.get("hashtags").and_then(|v| v.as_array()) {
        let count = hashtags.len();
        let (min, max) = template.output_schema.hashtag_count_range;
        if count < min || count > max {
            errors.push(format!(
                "Hashtag count {} not in range {}-{}", 
                count, min, max
            ));
        }
    }
    
    errors
}
