//! Deterministic content discovery and scoring system
//!
//! This module provides deterministic content discovery from:
//! 1. Documentation files (docs/*.md)
//! 2. Spec files (.github/automation/*.md)
//! 3. Code architecture (src-tauri/src/**/mod.rs)
//!
//! NO screenshots required - we extract content from text and generate visuals.

use std::path::{Path, PathBuf};
use std::collections::HashMap;

use crate::error::Result;
use crate::social::models::{ContentMetadata, ContentSource, SourceManifest};
use crate::models::social::SourceType;

/// Content asset discovered from the repository
#[derive(Debug, Clone)]
pub struct ContentAsset {
    /// Unique identifier
    pub id: String,
    /// Asset type
    pub asset_type: AssetType,
    /// Source path
    pub path: PathBuf,
    /// Title/headline
    pub title: String,
    /// Content body (extracted text)
    pub content: String,
    /// Key points extracted (for carousels)
    pub key_points: Vec<String>,
    /// Engagement score (0-100)
    pub engagement_score: u32,
    /// Suggested content pillar
    pub pillar: ContentPillar,
    /// Suggested template
    pub suggested_template: String,
    /// Suggested platform
    pub suggested_platform: String,
}

/// Types of content assets we can discover
#[derive(Debug, Clone, PartialEq)]
pub enum AssetType {
    /// Documentation article
    DocArticle,
    /// Technical specification
    SpecDocument,
    /// Architecture/module explanation
    CodeArchitecture,
    /// Process/workflow description
    ProcessDoc,
}

/// Content pillars for social strategy
#[derive(Debug, Clone, PartialEq)]
pub enum ContentPillar {
    /// Educational content (teach SEO/concepts)
    Educational,
    /// Behind the scenes (build in public)
    BehindTheScenes,
    /// Technical deep dive
    Technical,
    /// Process/workflow showcase
    Process,
}

/// Discover all content assets deterministically
pub fn discover_content_assets(project_path: &Path) -> Result<Vec<ContentAsset>> {
    let mut assets = Vec::new();
    
    // 1. Discover documentation
    assets.extend(discover_docs(project_path)?);
    
    // 2. Discover specs
    assets.extend(discover_specs(project_path)?);
    
    // 3. Discover code architecture
    assets.extend(discover_architecture(project_path)?);
    
    // 4. Score and rank all assets
    for asset in &mut assets {
        asset.engagement_score = compute_engagement_score(asset);
    }
    
    // 5. Sort by engagement score (highest first)
    assets.sort_by(|a, b| b.engagement_score.cmp(&a.engagement_score));
    
    Ok(assets)
}

/// Discover documentation files (docs/*.md)
fn discover_docs(project_path: &Path) -> Result<Vec<ContentAsset>> {
    let mut assets = Vec::new();
    let docs_dir = project_path.join("docs");
    
    if !docs_dir.exists() {
        return Ok(assets);
    }
    
    let entries = std::fs::read_dir(&docs_dir)?;
    
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        
        if let Ok(asset) = parse_doc_asset(&path) {
            assets.push(asset);
        }
    }
    
    Ok(assets)
}

/// Parse a documentation file into a content asset
fn parse_doc_asset(path: &Path) -> Result<ContentAsset> {
    let content = std::fs::read_to_string(path)?;
    let filename = path.file_stem().unwrap_or_default().to_string_lossy();
    
    // Parse frontmatter and body
    let (frontmatter, body) = parse_frontmatter(&content);
    
    // Extract title
    let title = frontmatter.get("title")
        .cloned()
        .or_else(|| extract_first_heading(&body))
        .unwrap_or_else(|| filename.to_string());
    
    // Extract key points from headings
    let key_points = extract_headings(&body, 5);
    
    // Determine pillar based on content
    let pillar = determine_pillar(&title, &body);
    
    // Determine template and platform based on content type
    let (template, platform) = determine_template_and_platform(&pillar, &key_points);
    
    Ok(ContentAsset {
        id: format!("doc:{}", filename),
        asset_type: AssetType::DocArticle,
        path: path.to_path_buf(),
        title,
        content: body,
        key_points,
        engagement_score: 0, // Computed later
        pillar,
        suggested_template: template,
        suggested_platform: platform,
    })
}

/// Discover specification files
fn discover_specs(project_path: &Path) -> Result<Vec<ContentAsset>> {
    let mut assets = Vec::new();
    
    let spec_dirs = vec![
        project_path.join(".github/automation"),
        project_path.join("automation"),
    ];
    
    for dir in spec_dirs {
        if !dir.exists() {
            continue;
        }
        
        let entries = std::fs::read_dir(&dir)?;
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            
            let name = path.file_stem().unwrap_or_default().to_string_lossy();
            if !name.contains("spec") && !name.contains("config") {
                continue;
            }
            
            if let Ok(asset) = parse_spec_asset(&path) {
                assets.push(asset);
            }
        }
    }
    
    Ok(assets)
}

/// Parse a spec file into a content asset
fn parse_spec_asset(path: &Path) -> Result<ContentAsset> {
    let content = std::fs::read_to_string(path)?;
    let filename = path.file_stem().unwrap_or_default().to_string_lossy();
    
    // Extract title from first heading
    let title = extract_first_heading(&content)
        .unwrap_or_else(|| format!("Spec: {}", filename));
    
    // Extract key points (usually from "##" headings)
    let key_points = extract_headings(&content, 5);
    
    Ok(ContentAsset {
        id: format!("spec:{}", filename),
        asset_type: AssetType::SpecDocument,
        path: path.to_path_buf(),
        title,
        content: content.clone(),
        key_points,
        engagement_score: 0,
        pillar: ContentPillar::Technical,
        suggested_template: "technical_explainer".to_string(),
        suggested_platform: "instagram_feed".to_string(),
    })
}

/// Discover code architecture (module descriptions)
fn discover_architecture(project_path: &Path) -> Result<Vec<ContentAsset>> {
    let mut assets = Vec::new();
    let src_dir = project_path.join("src-tauri/src");
    
    if !src_dir.exists() {
        return Ok(assets);
    }
    
    // Find all mod.rs files
    for entry in walkdir::WalkDir::new(&src_dir).max_depth(3) {
        let entry = entry?;
        let path = entry.path();
        
        if path.file_name() != Some(std::ffi::OsStr::new("mod.rs")) {
            continue;
        }
        
        if let Ok(asset) = parse_module_asset(path, &src_dir) {
            assets.push(asset);
        }
    }
    
    Ok(assets)
}

/// Parse a Rust module file into a content asset
fn parse_module_asset(path: &Path, src_root: &Path) -> Result<ContentAsset> {
    let content = std::fs::read_to_string(path)?;
    
    // Extract module name from path
    let relative = path.strip_prefix(src_root).unwrap_or(path);
    let module_path = relative.parent()
        .map(|p| p.to_string_lossy().replace('/', "::"))
        .unwrap_or_else(|| "root".to_string());
    
    // Extract doc comments (lines starting with //! or ///)
    let doc_lines: Vec<&str> = content.lines()
        .take(20) // First 20 lines only
        .filter(|l| l.trim_start().starts_with("//!"))
        .map(|l| l.trim_start().trim_start_matches("//!").trim())
        .collect();
    
    let description = if doc_lines.is_empty() {
        format!("Rust module: {}", module_path)
    } else {
        doc_lines.join(" ")
    };
    
    // Extract key exports (pub mod lines)
    let exports: Vec<String> = content.lines()
        .filter(|l| l.contains("pub mod") || l.contains("pub fn"))
        .take(5)
        .map(|l| l.trim().to_string())
        .collect();
    
    let title = format!("Architecture: {}", module_path);
    
    Ok(ContentAsset {
        id: format!("arch:{}", module_path.replace("::", "-")),
        asset_type: AssetType::CodeArchitecture,
        path: path.to_path_buf(),
        title,
        content: description,
        key_points: exports,
        engagement_score: 0,
        pillar: ContentPillar::BehindTheScenes,
        suggested_template: "behind_scenes".to_string(),
        suggested_platform: "instagram_reel".to_string(),
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Deterministic Scoring
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute engagement score deterministically
/// Formula based on measurable content attributes
fn compute_engagement_score(asset: &ContentAsset) -> u32 {
    let mut score = 50u32; // Base score
    
    // 1. Content length bonus (more substance = more engaging)
    let content_len = asset.content.len();
    if content_len > 5000 {
        score += 20;
    } else if content_len > 2000 {
        score += 15;
    } else if content_len > 1000 {
        score += 10;
    }
    
    // 2. Key points bonus (structured content is more engaging)
    let key_points_count = asset.key_points.len();
    if key_points_count >= 5 {
        score += 15;
    } else if key_points_count >= 3 {
        score += 10;
    } else if key_points_count >= 1 {
        score += 5;
    }
    
    // 3. Pillar-specific scoring
    score += match asset.pillar {
        ContentPillar::Educational => 10, // Educational performs well
        ContentPillar::BehindTheScenes => 15, // BTS is trending
        ContentPillar::Technical => 5,    // Niche but valuable
        ContentPillar::Process => 8,      // Practical value
    };
    
    // 4. Asset type bonus
    score += match asset.asset_type {
        AssetType::DocArticle => 10,
        AssetType::SpecDocument => 8,
        AssetType::CodeArchitecture => 12, // Unique content
        AssetType::ProcessDoc => 10,
    };
    
    // 5. Title quality
    if asset.title.len() > 20 && asset.title.len() < 80 {
        score += 5; // Sweet spot for titles
    }
    
    score.min(100)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helper Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Parse YAML frontmatter from markdown
fn parse_frontmatter(content: &str) -> (HashMap<String, String>, String) {
    let mut frontmatter = HashMap::new();
    
    if let Some(start) = content.find("---") {
        if let Some(end) = content[start + 3..].find("---") {
            let fm_text = &content[start + 3..start + 3 + end];
            
            for line in fm_text.lines() {
                if let Some(colon_pos) = line.find(':') {
                    let key = line[..colon_pos].trim().to_string();
                    let value = line[colon_pos + 1..].trim().to_string();
                    frontmatter.insert(key, value);
                }
            }
            
            let body = content[start + 3 + end + 3..].trim().to_string();
            return (frontmatter, body);
        }
    }
    
    (frontmatter, content.to_string())
}

/// Extract first markdown heading
fn extract_first_heading(content: &str) -> Option<String> {
    content.lines()
        .find(|l| l.trim().starts_with("# "))
        .map(|l| l.trim().trim_start_matches("# ").to_string())
}

/// Extract headings from content
fn extract_headings(content: &str, max: usize) -> Vec<String> {
    content.lines()
        .filter(|l| l.trim().starts_with("## ") || l.trim().starts_with("### "))
        .take(max)
        .map(|l| {
            l.trim()
                .trim_start_matches("## ")
                .trim_start_matches("### ")
                .to_string()
        })
        .collect()
}

/// Determine content pillar based on title and content
fn determine_pillar(title: &str, content: &str) -> ContentPillar {
    let text = format!("{} {}", title, content).to_lowercase();
    
    if text.contains("how we") || text.contains("build in public") || text.contains("behind") {
        ContentPillar::BehindTheScenes
    } else if text.contains("how to") || text.contains("guide") || text.contains("tutorial") {
        ContentPillar::Educational
    } else if text.contains("spec") || text.contains("architecture") || text.contains("design") {
        ContentPillar::Technical
    } else if text.contains("process") || text.contains("workflow") || text.contains("automation") {
        ContentPillar::Process
    } else {
        ContentPillar::Educational // Default
    }
}

/// Determine best template and platform for content
fn determine_template_and_platform(
    pillar: &ContentPillar,
    key_points: &[String],
) -> (String, String) {
    match pillar {
        ContentPillar::Educational => {
            if key_points.len() >= 4 {
                ("educational_carousel".to_string(), "instagram_feed".to_string())
            } else {
                ("quick_tip".to_string(), "tiktok".to_string())
            }
        }
        ContentPillar::BehindTheScenes => {
            ("behind_scenes".to_string(), "instagram_reel".to_string())
        }
        ContentPillar::Technical => {
            ("technical_explainer".to_string(), "instagram_feed".to_string())
        }
        ContentPillar::Process => {
            ("feature_hook".to_string(), "tiktok".to_string())
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Export Functions
// ═══════════════════════════════════════════════════════════════════════════════

/// Convert ContentAssets to ContentSources for the workflow
pub fn assets_to_sources(assets: Vec<ContentAsset>) -> Vec<ContentSource> {
    assets.into_iter().map(|asset| {
        ContentSource::new(
            match asset.asset_type {
                AssetType::DocArticle | AssetType::SpecDocument | AssetType::ProcessDoc => {
                    crate::models::social::SourceType::Article
                }
                AssetType::CodeArchitecture => {
                    crate::models::social::SourceType::Spec
                }
            },
            asset.id,
            asset.path,
            asset.content,
            ContentMetadata {
                title: Some(asset.title),
                description: Some(asset.key_points.join(" | ")),
                url_slug: None,
                published_date: None,
                word_count: Some(asset.content.split_whitespace().count() as u32),
            },
        )
    }).collect()
}

/// Get top N assets by engagement score
pub fn get_top_assets(assets: Vec<ContentAsset>, n: usize) -> Vec<ContentAsset> {
    let mut sorted = assets;
    sorted.sort_by(|a, b| b.engagement_score.cmp(&a.engagement_score));
    sorted.into_iter().take(n).collect()
}
