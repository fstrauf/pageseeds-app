#![allow(dead_code)]
//! Image generation for social media posts
//!
//! Generates images programmatically when no screenshots are available:
//! 1. Text-on-solid-color backgrounds (deterministic)
//! 2. Code snippet images (deterministic)
//! 3. Gradient/pattern backgrounds (deterministic)

use std::path::Path;

use crate::error::Result;
use crate::models::social::{CanvasSize, VisualAsset, AssetType};

use super::overlay::CanvasDimensions;

/// Generate a visual asset for a post when no source image exists
pub fn generate_visual_asset(
    output_path: &Path,
    text: &str,
    canvas_size: &CanvasSize,
    style: ImageStyle,
) -> Result<VisualAsset> {
    let dims = CanvasDimensions::from(canvas_size);
    
    // Generate the image
    match style {
        ImageStyle::SolidColor(color) => {
            generate_solid_color_image(output_path, dims, color, text)?;
        }
        ImageStyle::Gradient(start_color, end_color) => {
            generate_gradient_image(output_path, dims, start_color, end_color, text)?;
        }
        ImageStyle::CodeSnippet => {
            generate_code_image(output_path, dims, text)?;
        }
    }
    
    let relative_path = output_path.to_string_lossy().to_string();
    
    Ok(VisualAsset {
        path: relative_path,
        asset_type: AssetType::Image,
        description: format!("Generated image: {}", text.chars().take(50).collect::<String>()),
        overlay_text: Some(text.to_string()),
    })
}

/// Image style for generation
#[derive(Debug, Clone)]
pub enum ImageStyle {
    /// Solid color background (R, G, B)
    SolidColor((u8, u8, u8)),
    /// Gradient from one color to another
    Gradient((u8, u8, u8), (u8, u8, u8)),
    /// Code snippet style
    CodeSnippet,
}

/// Brand colors for PageSeeds
pub mod brand {
    // Primary colors
    pub const FOREST: (u8, u8, u8) = (33, 54, 41);      // #213629
    pub const CLAY: (u8, u8, u8) = (181, 101, 43);      // #b5652b
    pub const SEED: (u8, u8, u8) = (214, 160, 71);      // #d6a047
    pub const CREAM: (u8, u8, u8) = (250, 243, 232);    // #faf3e8
    pub const PAPER: (u8, u8, u8) = (255, 253, 248);    // #fffdf8
    
    // Accent colors
    pub const DARK_BG: (u8, u8, u8) = (22, 23, 24);     // #161718
    pub const WARM_GRAY: (u8, u8, u8) = (83, 98, 91);   // #53625b
}

/// Generate a solid color image with centered text
fn generate_solid_color_image(
    output_path: &Path,
    dims: CanvasDimensions,
    color: (u8, u8, u8),
    text: &str,
) -> Result<()> {
    use image::{ImageBuffer, Rgb};
    
    // Create image buffer
    let img = ImageBuffer::from_pixel(
        dims.width,
        dims.height,
        Rgb([color.0, color.1, color.2]),
    );
    
    // Note: Text rendering requires a font library like `ab_glyph` or `fontdue`
    // For now, we create the background and save it
    // Text overlay can be added later when we integrate a font renderer
    
    // Ensure output directory exists
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    
    // Save the image
    img.save(output_path)?;
    
    log::info!(
        "Generated solid color image ({}x{}) at {:?} with text: '{}'",
        dims.width,
        dims.height,
        output_path,
        text
    );
    
    Ok(())
}

/// Generate a gradient background image
fn generate_gradient_image(
    output_path: &Path,
    dims: CanvasDimensions,
    start_color: (u8, u8, u8),
    end_color: (u8, u8, u8),
    text: &str,
) -> Result<()> {
    use image::{ImageBuffer, Rgb};
    
    let mut img = ImageBuffer::new(dims.width, dims.height);
    
    // Generate vertical gradient
    for y in 0..dims.height {
        let ratio = y as f32 / dims.height as f32;
        let r = (start_color.0 as f32 * (1.0 - ratio) + end_color.0 as f32 * ratio) as u8;
        let g = (start_color.1 as f32 * (1.0 - ratio) + end_color.1 as f32 * ratio) as u8;
        let b = (start_color.2 as f32 * (1.0 - ratio) + end_color.2 as f32 * ratio) as u8;
        
        for x in 0..dims.width {
            img.put_pixel(x, y, Rgb([r, g, b]));
        }
    }
    
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    
    img.save(output_path)?;
    
    log::info!(
        "Generated gradient image ({}x{}) at {:?} with text: '{}'",
        dims.width,
        dims.height,
        output_path,
        text
    );
    
    Ok(())
}

/// Generate a code snippet style image
fn generate_code_image(
    output_path: &Path,
    dims: CanvasDimensions,
    _code: &str,
) -> Result<()> {
    use image::{ImageBuffer, Rgb};
    
    // Dark background for code
    let bg_color = brand::DARK_BG;
    let img = ImageBuffer::from_pixel(
        dims.width,
        dims.height,
        Rgb([bg_color.0, bg_color.1, bg_color.2]),
    );
    
    // Note: Code syntax highlighting would require more sophisticated rendering
    // For now, we create the dark background
    
    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    
    img.save(output_path)?;
    
    log::info!(
        "Generated code image ({}x{}) at {:?}",
        dims.width,
        dims.height,
        output_path
    );
    
    Ok(())
}

/// Select the best image style for content
pub fn select_image_style(pillar: &str, template: &str) -> ImageStyle {
    match (pillar, template) {
        ("educational", "carousel") => ImageStyle::Gradient(brand::FOREST, brand::CLAY),
        ("technical", _) => ImageStyle::CodeSnippet,
        ("behind_the_scenes", _) => ImageStyle::SolidColor(brand::SEED),
        (_, "feature_hook") => ImageStyle::Gradient(brand::CLAY, brand::FOREST),
        _ => ImageStyle::SolidColor(brand::FOREST),
    }
}

/// Generate a carousel of images
pub fn generate_carousel(
    output_dir: &Path,
    post_id: &str,
    slides: &[String],
    canvas_size: &CanvasSize,
) -> Result<Vec<VisualAsset>> {
    let mut assets = Vec::new();
    
    for (i, slide_text) in slides.iter().enumerate() {
        let filename = format!("{}_slide_{}.png", post_id, i);
        let output_path = output_dir.join(&filename);
        
        // Alternate colors for visual variety
        let style = if i % 2 == 0 {
            ImageStyle::Gradient(brand::FOREST, brand::CLAY)
        } else {
            ImageStyle::Gradient(brand::CLAY, brand::FOREST)
        };
        
        let asset = generate_visual_asset(&output_path, slide_text, canvas_size, style)?;
        assets.push(asset);
    }
    
    Ok(assets)
}

/// Get suggested image prompt for AI generation (agentic)
pub fn generate_image_prompt_for_agent(post_hook: &str, post_caption: &str) -> String {
    format!(
        r##"Create a social media image for this post:

Hook: {}
Caption: {}

Requirements:
- Style: Modern, minimalist, professional
- Colors: Forest green (#213629), warm clay (#b5652b), golden seed (#d6a047)
- Mood: Helpful, innovative, trustworthy
- No text in the image (text will be overlaid)
- Abstract representation of SEO/automation/concept

Describe the visual concept:"##,
        post_hook,
        post_caption.chars().take(100).collect::<String>()
    )
}
