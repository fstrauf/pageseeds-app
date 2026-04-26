//! Discover and manage content sources in a project

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::social::models::{ContentMetadata, ContentSource, SourceManifest};
use crate::models::social::{SourceConfig, SourceType};
use crate::content::locator;

use super::extractor;

/// Discover all content sources in a project based on configuration
pub fn discover_sources(
    project_path: &Path,
    config: &SourceConfig,
) -> Result<SourceManifest> {
    let mut manifest = SourceManifest {
        articles: Vec::new(),
        screenshots: Vec::new(),
        specs: Vec::new(),
    };
    
    // Discover articles
    if config.include_articles {
        manifest.articles = discover_articles(project_path, &config.article_slugs)?;
    }
    
    // Discover screenshots
    if config.include_screenshots {
        manifest.screenshots = discover_screenshots(project_path, &config.screenshot_dirs)?;
    }
    
    // Discover specs
    if config.include_specs {
        manifest.specs = discover_specs(project_path)?;
    }
    
    Ok(manifest)
}

/// Discover MDX articles in the project
fn discover_articles(
    project_path: &Path,
    specific_slugs: &[String],
) -> Result<Vec<crate::social::models::ContentSource>> {
    let mut articles = Vec::new();
    
    log::info!("[discover_articles] project_path: {:?}", project_path);
    
    // Use the content locator to find the actual content directory
    let resolution = locator::resolve(project_path, None);
    log::info!("[discover_articles] content resolution: source={}, has_markdown={}, selected={:?}",
        resolution.source, resolution.has_markdown, resolution.selected);
    
    let content_dirs: Vec<PathBuf> = if let Some(selected) = resolution.selected {
        vec![selected]
    } else {
        // Fallback to common locations if locator doesn't find anything
        vec![
            project_path.join("content"),
            project_path.join("content/blog"),
            project_path.join("src/content"),
            project_path.join("src/content/blog"),
        ]
    };
    
    let mut checked_dirs = 0;
    let mut found_dirs = 0;
    for content_dir in &content_dirs {
        checked_dirs += 1;
        let exists = content_dir.exists();
        if exists {
            found_dirs += 1;
            log::info!("[discover_articles] scanning dir: {:?}", content_dir);
        } else {
            continue;
        }
        
        let entries = walkdir::WalkDir::new(&content_dir)
            .max_depth(3)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let path = e.path();
                let ext = path.extension().and_then(|e| e.to_str());
                matches!(ext, Some("mdx") | Some("md"))
            });
        
        for entry in entries {
            let path = entry.path();
            
            // If specific slugs requested, filter by them
            if !specific_slugs.is_empty() {
                let slug = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("");
                if !specific_slugs.iter().any(|s| slug.contains(s)) {
                    continue;
                }
            }
            
            match extractor::extract_from_article(path) {
                Ok(source) => {
                    // Re-wrap with computed scores
                    let source_with_scores = ContentSource::new(
                        source.source_type,
                        source.source_id,
                        source.path,
                        source.content,
                        source.metadata,
                    );
                    articles.push(source_with_scores);
                }
                Err(e) => log::warn!("Failed to extract article {:?}: {}", path, e),
            }
        }
    }
    
    log::info!("[discover_articles] checked {} dirs, found {} existing dirs, {} articles total", 
        checked_dirs, found_dirs, articles.len());
    Ok(articles)
}

/// Discover screenshots in the project
fn discover_screenshots(
    project_path: &Path,
    screenshot_dirs: &[String],
) -> Result<Vec<crate::social::models::ContentSource>> {
    let mut screenshots = Vec::new();
    
    // Use specified directories or default locations
    let dirs_to_search: Vec<PathBuf> = if screenshot_dirs.is_empty() {
        vec![
            project_path.join("screenshots"),
            project_path.join("public/screenshots"),
            project_path.join("assets/screenshots"),
            project_path.join("images"),
        ]
    } else {
        screenshot_dirs.iter().map(|d| project_path.join(d)).collect()
    };
    
    log::info!("[discover_screenshots] checking {} dirs", dirs_to_search.len());
    
    let mut checked_screenshot_dirs = 0;
    let mut found_screenshot_dirs = 0;
    for dir in &dirs_to_search {
        checked_screenshot_dirs += 1;
        let exists = dir.exists();
        if exists {
            found_screenshot_dirs += 1;
            log::info!("[discover_screenshots] scanning dir: {:?}", dir);
        }
        if !exists {
            continue;
        }
        
        let entries = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let path = e.path();
                let ext = path.extension().and_then(|e| e.to_str());
                matches!(ext, Some("png") | Some("jpg") | Some("jpeg") | Some("webp"))
            });
        
        for entry in entries {
            let path = entry.path();
            let filename = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("screenshot")
                .to_string();
            
            // Get relative path from project root
            let relative_path = path
                .strip_prefix(project_path)
                .unwrap_or(&path)
                .to_path_buf();
            
            screenshots.push(ContentSource::new(
                SourceType::Screenshot,
                filename.clone(),
                relative_path,
                filename.clone(),
                ContentMetadata {
                    description: Some(format!("Screenshot: {}", filename)),
                    ..Default::default()
                },
            ));
        }
    }
    
    log::info!("[discover_screenshots] checked {} dirs, found {} existing dirs, {} screenshots total",
        checked_screenshot_dirs, found_screenshot_dirs, screenshots.len());
    Ok(screenshots)
}

/// Discover spec documents in the project
fn discover_specs(project_path: &Path) -> Result<Vec<crate::social::models::ContentSource>> {
    let mut specs = Vec::new();
    
    // Look for specs in automation directory
    let spec_dirs = vec![
        project_path.join(".github/automation"),
        project_path.join("automation"),
        project_path.join("specs"),
    ];
    
    for dir in spec_dirs {
        if !dir.exists() {
            continue;
        }
        
        let entries = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                let name = e.file_name();
                let name_str = name.to_string_lossy().to_lowercase();
                name_str.contains("spec") && name_str.ends_with(".md")
            });
        
        for entry in entries {
            let path = entry.path();
            
            match extractor::extract_from_spec(&path) {
                Ok(source) => {
                    let source_with_scores = ContentSource::new(
                        source.source_type,
                        source.source_id,
                        source.path,
                        source.content,
                        source.metadata,
                    );
                    specs.push(source_with_scores);
                }
                Err(e) => log::warn!("Failed to extract spec {:?}: {}", path, e),
            }
        }
    }
    
    Ok(specs)
}

/// Get the output directory for generated social assets
pub fn get_social_output_dir(project_path: &Path) -> PathBuf {
    project_path.join(".github/automation/social")
}

/// Ensure the social output directory exists
pub fn ensure_output_dir(project_path: &Path) -> Result<PathBuf> {
    let dir = get_social_output_dir(project_path);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}
