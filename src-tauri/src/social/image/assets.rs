//! Image asset resolution for social posts
//!
//! Tries multiple sources in order:
//! 1. Article's own images (from article directory, content/frontmatter images)
//! 2. User-provided screenshots (screenshots/ folder)
//! 3. Generated branded graphics (deterministic fallback)

use std::path::{Path, PathBuf};
use crate::error::Result;
use crate::models::article::Article;
use crate::models::social::{VisualAsset, AssetType};

/// Resolve image assets for a social post
pub fn resolve_post_images(
    article: &Article,
    project_path: &Path,
    output_dir: &Path,
    hook_text: &str,
) -> Result<Vec<VisualAsset>> {
    let mut assets = Vec::new();
    
    // 1. Try article's content directory for images (using url_slug)
    let article_images = find_images_for_article(article, project_path);
    if !article_images.is_empty() {
        for img_path in article_images {
            assets.push(VisualAsset {
                path: img_path.to_string_lossy().to_string(),
                asset_type: AssetType::Image,
                description: "Article image".to_string(),
                overlay_text: None,
            });
        }
        return Ok(assets);
    }
    
    // 2. Try user-provided screenshots
    let screenshots = find_screenshots(project_path);
    if !screenshots.is_empty() {
        // Use first screenshot as main image
        assets.push(VisualAsset {
            path: screenshots[0].to_string_lossy().to_string(),
            asset_type: AssetType::Image,
            description: "App screenshot".to_string(),
            overlay_text: Some(hook_text.chars().take(60).collect()),
        });
        return Ok(assets);
    }
    
    // 3. Generate branded graphic as fallback
    let generated = generate_branded_graphic(output_dir, hook_text)?;
    assets.push(generated);
    
    Ok(assets)
}

/// Find images associated with an article
fn find_images_for_article(article: &Article, project_path: &Path) -> Vec<PathBuf> {
    let mut images = Vec::new();
    
    // Use url_slug for directory lookup
    let slug = &article.url_slug;
    
    // Common locations for article images
    let possible_dirs = vec![
        project_path.join("public").join(slug),
        project_path.join("content").join(slug),
        project_path.join("src/content").join(slug),
    ];
    
    for dir in possible_dirs {
        if !dir.exists() {
            continue;
        }
        
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if is_image_file(&path) {
                    images.push(path);
                }
            }
        }
    }
    
    images
}

/// Find user-provided screenshots
fn find_screenshots(project_path: &Path) -> Vec<PathBuf> {
    let mut screenshots = Vec::new();
    
    let screenshot_dirs = vec![
        project_path.join("screenshots"),
        project_path.join("public/screenshots"),
        project_path.join("assets/screenshots"),
    ];
    
    for dir in screenshot_dirs {
        if !dir.exists() {
            continue;
        }
        
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if is_image_file(&path) {
                    screenshots.push(path);
                }
            }
        }
    }
    
    // Sort by modification time (newest first)
    screenshots.sort_by(|a, b| {
        let meta_a = std::fs::metadata(a).ok();
        let meta_b = std::fs::metadata(b).ok();
        match (meta_a, meta_b) {
            (Some(ma), Some(mb)) => {
                let time_a = ma.modified().ok();
                let time_b = mb.modified().ok();
                time_b.cmp(&time_a) // Reverse for newest first
            }
            _ => std::cmp::Ordering::Equal,
        }
    });
    
    screenshots
}

/// Check if file is an image
fn is_image_file(path: &Path) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some("png") | Some("jpg") | Some("jpeg") | Some("webp") | Some("gif") => true,
        _ => false,
    }
}

/// Generate a branded graphic as fallback
pub fn generate_branded_graphic(output_dir: &Path, text: &str) -> Result<VisualAsset> {
    use image::{ImageBuffer, Rgb};
    
    // PageSeeds brand colors
    let forest_green = Rgb([33, 54, 41]);     // #213629
    let clay = Rgb([181, 101, 43]);           // #b5652b
    let cream = Rgb([250, 243, 232]);         // #faf3e8
    
    // Create 1080x1080 square image (Instagram optimal)
    let width = 1080u32;
    let height = 1080u32;
    
    // Create gradient from forest green to clay
    let mut img = ImageBuffer::new(width, height);
    for y in 0..height {
        let ratio = y as f32 / height as f32;
        let r = (forest_green.0[0] as f32 * (1.0 - ratio) + clay.0[0] as f32 * ratio) as u8;
        let g = (forest_green.0[1] as f32 * (1.0 - ratio) + clay.0[1] as f32 * ratio) as u8;
        let b = (forest_green.0[2] as f32 * (1.0 - ratio) + clay.0[2] as f32 * ratio) as u8;
        
        for x in 0..width {
            img.put_pixel(x, y, Rgb([r, g, b]));
        }
    }
    
    // Note: Text rendering requires font library. For now, we create the background.
    // Text overlay can be added when we integrate ab_glyph or similar.
    
    // Save the image
    std::fs::create_dir_all(output_dir)?;
    let filename = format!("generated_{}.png", generate_id(text));
    let output_path = output_dir.join(&filename);
    img.save(&output_path)?;
    
    let relative_path = output_path.to_string_lossy().to_string();
    
    Ok(VisualAsset {
        path: relative_path,
        asset_type: AssetType::Image,
        description: format!("Generated graphic: {}", text.chars().take(50).collect::<String>()),
        overlay_text: Some(text.chars().take(80).collect()),
    })
}

/// Generate simple ID from text
fn generate_id(text: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    format!("{:x}", hasher.finish())[..8].to_string()
}

/// Generate AI image prompt (for external APIs like DALL-E)
pub fn generate_ai_image_prompt(hook: &str, caption: &str, platform: &str) -> String {
    format!(
        "Create a {} social media image for this post:\n\nHook: {}\nCaption: {}\n\nRequirements:\n- Style: Modern, minimalist, professional\n- Mood: Helpful, innovative, trustworthy\n- No text in the image (text will be overlaid separately)\n- Abstract representation related to content/SEO/coffee (match the topic)\n- Colors: Deep forest green, warm clay orange, cream white",
        platform,
        hook,
        caption.chars().take(100).collect::<String>()
    )
}
