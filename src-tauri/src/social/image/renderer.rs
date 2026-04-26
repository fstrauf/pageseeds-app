#![allow(dead_code)]
//! High-level image rendering for social posts

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::models::social::{PostFormat, SocialPost, VisualAsset};

use super::overlay;

/// Render all visual assets for a social post
pub fn render_post_assets(
    post: &SocialPost,
    project_path: &Path,
    output_dir: &Path,
) -> Result<Vec<VisualAsset>> {
    let mut rendered_assets = Vec::new();
    
    for (i, asset) in post.visual_assets.iter().enumerate() {
        let input_path = project_path.join(&asset.path);
        
        // Generate output filename
        let output_filename = format!("{}_{}.png", post.id, i);
        let output_path = output_dir.join(&output_filename);
        
        // Apply overlay if text is specified
        if let Some(ref overlay_text) = asset.overlay_text {
            // Get template config for overlay settings
            // For now, use default config
            let config = default_overlay_config(&post.format);
            
            overlay::apply_text_overlay(
                &input_path,
                &output_path,
                overlay_text,
                &config,
            )?;
        } else {
            // Just copy/rescale the image
            std::fs::copy(&input_path, &output_path)?;
        }
        
        // Add to rendered assets with updated path
        let relative_output = output_path
            .strip_prefix(project_path)
            .unwrap_or(&output_path)
            .to_string_lossy()
            .to_string();
        
        rendered_assets.push(VisualAsset {
            path: relative_output,
            asset_type: asset.asset_type.clone(),
            description: asset.description.clone(),
            overlay_text: asset.overlay_text.clone(),
        });
    }
    
    Ok(rendered_assets)
}

/// Build a carousel (multiple images as one post)
pub fn build_carousel(
    post: &SocialPost,
    project_path: &Path,
    output_dir: &Path,
    slides: &[String], // Text for each slide
) -> Result<Vec<VisualAsset>> {
    let mut carousel_assets = Vec::new();
    
    for (i, slide_text) in slides.iter().enumerate() {
        let output_filename = format!("{}_slide_{}.png", post.id, i);
        let output_path = output_dir.join(&output_filename);
        
        // If there's a base image, use it; otherwise create text-only
        if let Some(base_asset) = post.visual_assets.first() {
            let input_path = project_path.join(&base_asset.path);
            
            let config = default_overlay_config(&post.format);
            
            overlay::apply_text_overlay(
                &input_path,
                &output_path,
                slide_text,
                &config,
            )?;
        } else {
            // Create text-only slide
            let config = default_overlay_config(&post.format);
            overlay::create_text_image(
                &output_path,
                slide_text,
                &config,
                (30, 30, 30), // Dark background
            )?;
        }
        
        let relative_output = output_path
            .strip_prefix(project_path)
            .unwrap_or(&output_path)
            .to_string_lossy()
            .to_string();
        
        carousel_assets.push(VisualAsset {
            path: relative_output,
            asset_type: crate::models::social::AssetType::Image,
            description: format!("Carousel slide {}", i + 1),
            overlay_text: Some(slide_text.clone()),
        });
    }
    
    Ok(carousel_assets)
}

/// Get the preview path for a post
pub fn get_preview_path(post_id: &str, project_path: &Path) -> PathBuf {
    project_path.join(".github/automation/social/previews").join(format!("{}.png", post_id))
}

/// Generate a preview image for a post
pub fn generate_preview(
    post: &SocialPost,
    project_path: &Path,
) -> Result<PathBuf> {
    let preview_dir = project_path.join(".github/automation/social/previews");
    std::fs::create_dir_all(&preview_dir)?;
    
    let preview_path = preview_dir.join(format!("{}.png", post.id));
    
    // If post has visual assets, use the first one as base
    if let Some(asset) = post.visual_assets.first() {
        let asset_path = project_path.join(&asset.path);
        
        // For preview, just copy the asset (in real impl, might add watermark)
        if asset_path.exists() {
            std::fs::copy(&asset_path, &preview_path)?;
        } else {
            // Create placeholder preview
            create_placeholder_preview(&preview_path, &post.hook)?;
        }
    } else {
        // Create placeholder preview with hook text
        create_placeholder_preview(&preview_path, &post.hook)?;
    }
    
    Ok(preview_path)
}

/// Create a placeholder preview image
fn create_placeholder_preview(path: &Path, _text: &str) -> Result<()> {
    // Placeholder - would create an image with the text
    // For now, just create an empty file
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(path, b"")?;
    Ok(())
}

/// Default overlay configuration based on format
fn default_overlay_config(format: &PostFormat) -> crate::models::social::OverlayConfig {
    use crate::models::social::{CanvasSize, TextPosition};
    
    let canvas_size = match format {
        PostFormat::SingleImage => CanvasSize::Square,
        PostFormat::Carousel => CanvasSize::Portrait,
        PostFormat::VideoHook => CanvasSize::TikTok,
    };
    
    crate::models::social::OverlayConfig {
        canvas_size,
        font_family: "Inter".to_string(),
        primary_color: "#FFFFFF".to_string(),
        secondary_color: "#000000".to_string(),
        text_position: TextPosition::Center,
        max_text_length: 100,
    }
}
