//! Image processing and rendering

pub mod assets;
pub mod generator;
pub mod overlay;
pub mod renderer;

// Re-export commonly used functions
pub use assets::{resolve_post_images, generate_ai_image_prompt};
