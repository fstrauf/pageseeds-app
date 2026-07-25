//! Social media marketing module
//!
//! Provides functionality for generating, managing, and scheduling
//! social media content across TikTok and Instagram platforms.

pub mod db;
pub mod models;
pub mod prompts;
pub mod templates;

pub mod content {
    pub mod extractor;
    pub mod sources;
}

pub mod image;

// Re-export commonly used types
