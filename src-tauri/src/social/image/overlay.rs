//! Text overlay rendering on images

use std::path::Path;

use crate::error::Result;
use crate::models::social::{CanvasSize, OverlayConfig, TextPosition};

/// Dimensions for a canvas size
pub struct CanvasDimensions {
    pub width: u32,
    pub height: u32,
}

impl From<&CanvasSize> for CanvasDimensions {
    fn from(size: &CanvasSize) -> Self {
        match size {
            CanvasSize::TikTok => CanvasDimensions {
                width: 1080,
                height: 1920,
            },
            CanvasSize::Square => CanvasDimensions {
                width: 1080,
                height: 1080,
            },
            CanvasSize::Portrait => CanvasDimensions {
                width: 1080,
                height: 1350,
            },
            CanvasSize::Story => CanvasDimensions {
                width: 1080,
                height: 1920,
            },
        }
    }
}

/// Apply text overlay to an image
pub fn apply_text_overlay(
    input_path: &Path,
    output_path: &Path,
    text: &str,
    config: &OverlayConfig,
) -> Result<()> {
    // This is a placeholder implementation
    // In a real implementation, this would use the `image` crate to:
    // 1. Load the input image
    // 2. Resize/crop to canvas dimensions
    // 3. Render text overlay at specified position
    // 4. Save to output path
    
    log::info!(
        "Applying overlay to {:?}: '{}' (position: {:?}, size: {:?})",
        input_path,
        text,
        config.text_position,
        config.canvas_size
    );
    
    // For now, just copy the image (placeholder)
    std::fs::copy(input_path, output_path)?;
    
    Ok(())
}

/// Create a text-only image with background
pub fn create_text_image(
    output_path: &Path,
    text: &str,
    config: &OverlayConfig,
    background_color: (u8, u8, u8),
) -> Result<()> {
    log::info!(
        "Creating text image at {:?} with text: '{}'",
        output_path,
        text
    );
    
    // Placeholder - would use image crate to create a solid color image
    // with rendered text
    
    Ok(())
}

/// Wrap text to fit within a maximum width
pub fn wrap_text(text: &str, max_chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();
    
    for word in text.split_whitespace() {
        if current_line.len() + word.len() + 1 > max_chars {
            if !current_line.is_empty() {
                lines.push(current_line.clone());
                current_line.clear();
            }
        }
        if !current_line.is_empty() {
            current_line.push(' ');
        }
        current_line.push_str(word);
    }
    
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    
    lines
}

/// Calculate font size based on text length and canvas size
pub fn calculate_font_size(text_len: usize, canvas_height: u32) -> u32 {
    let base_size = canvas_height / 20; // 5% of canvas height
    
    // Reduce size for longer text
    if text_len > 100 {
        base_size * 2 / 3
    } else if text_len > 50 {
        base_size * 3 / 4
    } else {
        base_size
    }
}
