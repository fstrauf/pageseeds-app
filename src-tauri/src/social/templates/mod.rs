#![allow(dead_code)]
//! Template system for social media content generation
//!
//! This module provides a hybrid deterministic/agentic approach:
//! - Template selection is deterministic (rule-based)
//! - Content transformation is agentic (creative writing)

use crate::models::social::{Platform, PostFormat, SourceType};
use crate::social::models::ContentSource;

/// Output schema for validation
#[derive(Debug, Clone)]
pub struct OutputSchema {
    pub required_fields: Vec<String>,
    pub max_hook_length: usize,
    pub max_caption_length: usize,
    pub hashtag_count_range: (usize, usize),
}

/// Template definition with prompt and validation rules
#[derive(Debug, Clone)]
pub struct TemplateDef {
    /// Template identifier (matches ContentTemplate.id)
    pub id: String,
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

/// Template registry - all available templates
pub struct TemplateRegistry {
    templates: Vec<TemplateDef>,
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

    /// Get template definition by ID
    pub fn get(&self, id: &str) -> Option<&TemplateDef> {
        self.templates.iter().find(|t| t.id == id)
    }

    /// Get all templates
    pub fn all(&self) -> &[TemplateDef] {
        &self.templates
    }

    /// Select the best template definition for a content source (deterministic)
    pub fn select_for_source(&self, source: &ContentSource) -> Option<&TemplateDef> {
        // Score all templates and pick best match
        self.templates.iter()
            .filter(|t| t.source_types.contains(&source.source_type))
            .max_by_key(|t| {
                // Score based on:
                // 1. Source type match (already filtered)
                // 2. Engagement weight alignment with source score
                // 3. Template ID match with suggested template
                let suggested_bonus = if t.id == source.suggested_template { 30 } else { 0 };
                t.engagement_weight + suggested_bonus
            })
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // Template Definitions
    // ═══════════════════════════════════════════════════════════════════════════════

    /// Template: Feature Hook (TikTok/Reels short-form)
    fn feature_hook_template() -> TemplateDef {
        TemplateDef {
            id: "feature_hook".to_string(),
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
    fn educational_carousel_template() -> TemplateDef {
        TemplateDef {
            id: "educational_carousel".to_string(),
            platform: Platform::InstagramFeed,
            format: PostFormat::Carousel,
            prompt_template: educational_carousel_prompt(),
            output_schema: OutputSchema {
                required_fields: vec!["hook".to_string(), "caption".to_string(), 
                    "hashtags".to_string(), "cta".to_string(), "visual_description".to_string(), "overlay_text".to_string()],
                max_hook_length: 150,
                max_caption_length: 500,
                hashtag_count_range: (5, 10),
            },
            source_types: vec![SourceType::Article, SourceType::Spec],
            engagement_weight: 80,
        }
    }

    /// Template: Technical Explainer (Instagram Feed)
    fn technical_explainer_template() -> TemplateDef {
        TemplateDef {
            id: "technical_explainer".to_string(),
            platform: Platform::InstagramFeed,
            format: PostFormat::SingleImage,
            prompt_template: technical_explainer_prompt(),
            output_schema: OutputSchema {
                required_fields: vec!["hook".to_string(), "caption".to_string(), 
                    "hashtags".to_string(), "cta".to_string(), "visual_description".to_string(), "overlay_text".to_string()],
                max_hook_length: 150,
                max_caption_length: 800,
                hashtag_count_range: (5, 10),
            },
            source_types: vec![SourceType::Spec, SourceType::Article],
            engagement_weight: 70,
        }
    }

    /// Template: Quick Tip (TikTok/Reels)
    fn quick_tip_template() -> TemplateDef {
        TemplateDef {
            id: "quick_tip".to_string(),
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
    fn behind_scenes_template() -> TemplateDef {
        TemplateDef {
            id: "behind_scenes".to_string(),
            platform: Platform::InstagramReel,
            format: PostFormat::SingleImage,
            prompt_template: behind_scenes_prompt(),
            output_schema: OutputSchema {
                required_fields: vec!["hook".to_string(), "caption".to_string(), 
                    "hashtags".to_string(), "cta".to_string(), "visual_description".to_string(), "overlay_text".to_string()],
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
  "hook": "The #1 mistake costing you money (save this)",
  "caption": "5-slide breakdown:\n\n1️⃣ The problem most people miss\n2️⃣ Why conventional wisdom fails\n3️⃣ The data-driven approach\n4️⃣ How to implement it\n5️⃣ Results you can expect\n\nSave this for your next trade 👆",
  "hashtags": ["#optionstrading", "#investing", "#stockmarket", "#tradingtips", "#financialeducation"],
  "cta": "Follow for daily trading insights",
  "visual_description": "5-slide carousel with bold text, charts, and clean design",
  "overlay_text": "Save this carousel 📌"
}
```

Requirements:
- Hook should mention the benefit + encourage saving
- Caption summarizes the carousel slides
- Visual description references the carousel format
- Overlay text is short and punchy"##.to_string()
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
  "visual_description": "Clean diagram or screenshot illustrating the concept",
  "overlay_text": "The key insight in one line"
}
```

Requirements:
- Caption: 150-400 characters optimal
- Use line breaks for readability
- Position as educational, not promotional
- Visual description helps create the image asset"##.to_string()
}

fn quick_tip_prompt() -> String {
    r##"You are a short-form content creator. Share a quick trading tip.

## Source
{source_summary}

## Task
Create a 15-second worth of content tip.

## Rules
1. Hook: "This one thing changed my trading"
2. Tip must be actionable in 1 sentence
3. Show before/after or process
4. Make it feel like a secret

## Output Contract (JSON only)
```json
{
  "hook": "The trading hack nobody talks about",
  "caption": "The tip + why it works",
  "hashtags": ["#tradingtips", "#quicktip", "#options"],
  "cta": "Follow for more",
  "visual_description": "Before/after comparison or process screenshot",
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
  "hashtags": ["#buildinpublic", "#indiehackers", "#transparency"],
  "cta": "What would you do?",
  "visual_description": "Behind-the-scenes photo or screenshot showing the process",
  "overlay_text": "Real talk 💬"
}
```"##.to_string()
}

/// Render a prompt template with source context
pub fn render_prompt(template: &TemplateDef, source: &ContentSource) -> String {
    template.prompt_template.replace("{source_summary}", &source.content_summary())
}

/// Validate agent output against template schema
pub fn validate_output(template: &TemplateDef, output: &serde_json::Value) -> Vec<String> {
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
