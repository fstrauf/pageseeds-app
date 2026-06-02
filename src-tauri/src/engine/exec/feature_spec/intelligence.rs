//! Deterministic infrastructure audit collector for the feature spec generator.
//!
//! Gathers ONLY repo-level infrastructure signals:
//! - Template/component SEO implementation
//! - Build output quality (prerendering, sitemap, robots)
//! - URL architecture (temporal URLs, trailing slashes)
//! - Performance signals (lazy loading, image optimization)
//! - Source verification (ground truth from reading actual files)
//!
//! CRITICAL: This report is for WEBSITE DEVELOPERS only.
//! NO content-quality metrics (word counts, readability, keyword gaps).
//! NO PageSeeds-internal state.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

/// Infrastructure audit report. Developer-facing only.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InfrastructureAuditReport {
    pub project: ProjectSummary,
    pub template_audit: TemplateAudit,
    pub build_output_audit: BuildOutputAudit,
    pub url_architecture: UrlArchitecture,
    pub title_quality: TitleQualityAudit,
    pub og_image_audit: OgImageAudit,
    pub performance_signals: PerformanceSignals,
    pub source_verification: SourceVerification,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub project_id: String,
    pub project_path: String,
    pub tech_stack: String,
    pub article_count: i64,
}

/// Deep inspection of SEO template components.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateAudit {
    /// Whether the SEO head component exists
    pub seo_head_exists: bool,
    /// Whether the SEO head component path
    pub seo_head_path: Option<String>,
    /// Capabilities detected from reading source
    pub capabilities_detected: Vec<String>,
    /// Missing capabilities that SHOULD be present
    pub gaps_detected: Vec<String>,
    /// Raw source summary (first 500 chars of key functions)
    pub source_summary: String,
}

/// Inspection of build output (dist/ directory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildOutputAudit {
    /// Whether dist/ exists and has HTML files
    pub has_built_output: bool,
    /// Whether sitemap.xml exists in dist/
    pub has_sitemap: bool,
    /// Whether robots.txt exists in dist/
    pub has_robots_txt: bool,
    /// Whether a 404.html exists
    pub has_404_page: bool,
    /// Number of HTML files in dist/
    pub html_file_count: i64,
    /// Sampled built pages and their meta tag coverage
    pub sampled_pages: Vec<BuiltPageSample>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuiltPageSample {
    pub slug: String,
    pub has_title: bool,
    pub has_meta_description: bool,
    pub has_canonical: bool,
    pub has_og_tags: bool,
    pub has_json_ld: bool,
    pub has_viewport: bool,
    pub title_text: String,
    pub description_text: String,
}

/// URL architecture issues requiring developer intervention.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UrlArchitecture {
    /// Count of slugs with year suffixes (e.g. "-2025")
    pub temporal_url_count: i64,
    /// Sample temporal URLs
    pub sample_temporal_urls: Vec<String>,
    /// Whether trailing slashes are consistent across the site
    pub trailing_slash_consistent: Option<bool>,
    /// How the blog parser derives slugs from filenames
    pub slug_derivation_logic: String,
}

/// Performance and optimization signals from source inspection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSignals {
    /// Whether images use lazy loading
    pub has_lazy_loading: bool,
    /// Whether WebP or AVIF images are used
    pub has_modern_image_formats: bool,
    /// Whether scripts use async/defer
    pub has_script_optimization: bool,
    /// Whether a performance budget or bundle analyzer exists
    pub has_bundle_analysis: bool,
    /// Key performance-related config found
    pub config_notes: Vec<String>,
}

/// Title quality issues that affect rendered HTML output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleQualityAudit {
    /// Number of articles with truncated titles (cut off mid-sentence)
    pub truncated_title_count: i64,
    /// Sample truncated titles with their slugs
    pub truncated_samples: Vec<TitleIssue>,
    /// Number of duplicate titles (same title used by multiple articles)
    pub duplicate_title_count: i64,
    /// Sample duplicate titles
    pub duplicate_samples: Vec<DuplicateTitle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TitleIssue {
    pub slug: String,
    pub title: String,
    pub issue: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateTitle {
    pub title: String,
    pub slugs: Vec<String>,
}

/// OG image URL audit — checks for relative OG:image URLs in built HTML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OgImageAudit {
    /// Articles with relative OG image URLs
    pub relative_og_image_count: i64,
    /// Sample slugs with relative OG images
    pub sample_relative_og_images: Vec<OgImageIssue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OgImageIssue {
    pub slug: String,
    pub og_image_url: String,
}

/// Ground-truth verification from reading actual source/build files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceVerification {
    /// Key source files found and their purposes
    pub key_source_files: Vec<SourceFileNote>,
    /// Frontmatter sample check from MDX files
    pub frontmatter_sample: FrontmatterSampleCheck,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFileNote {
    pub path: String,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontmatterSampleCheck {
    pub files_checked: i64,
    pub all_have_title: bool,
    pub all_have_description: bool,
    pub malformed_count: i64,
}

pub fn collect_infrastructure_audit(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
) -> Result<InfrastructureAuditReport, String> {
    Ok(InfrastructureAuditReport {
        project: collect_project_summary(conn, project_id, project_path)?,
        template_audit: collect_template_audit(project_path)?,
        build_output_audit: collect_build_output_audit(project_path)?,
        url_architecture: collect_url_architecture(conn, project_id, project_path)?,
        title_quality: collect_title_quality(conn, project_id, project_path)?,
        og_image_audit: collect_og_image_audit(project_path)?,
        performance_signals: collect_performance_signals(project_path)?,
        source_verification: collect_source_verification(project_path)?,
    })
}

fn collect_project_summary(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
) -> Result<ProjectSummary, String> {
    let articles = crate::engine::task_store::list_articles(conn, project_id)
        .map_err(|e| format!("list_articles: {e}"))?;

    let tech_stack = crate::content::ops::detect_tech_stack(std::path::Path::new(project_path));

    Ok(ProjectSummary {
        project_id: project_id.to_string(),
        project_path: project_path.to_string(),
        tech_stack,
        article_count: articles.len() as i64,
    })
}

fn collect_template_audit(project_path: &str) -> Result<TemplateAudit, String> {
    let root = std::path::Path::new(project_path);

    // Find SEO head component
    let seo_head_paths = [
        "src/components/SEOHead.vue",
        "src/components/SeoHead.vue",
        "src/components/SEO.tsx",
        "src/components/SEO.jsx",
        "src/components/Head.tsx",
        "app/components/SEOHead.vue",
        "components/SEOHead.vue",
    ];

    let mut seo_head_path = None;
    for path in &seo_head_paths {
        if root.join(path).exists() {
            seo_head_path = Some(path.to_string());
            break;
        }
    }

    let mut capabilities = Vec::new();
    let mut gaps = Vec::new();
    let mut source_summary = String::new();

    if let Some(ref path) = seo_head_path {
        let content = std::fs::read_to_string(root.join(path)).unwrap_or_default();
        source_summary = summarize_source(&content, 800);

        // Detect capabilities
        if content.contains("useHead") || content.contains("useSeoMeta") || content.contains("@unhead/vue") || content.contains("next/head") {
            capabilities.push("head_injection_library".to_string());
        }
        if content.contains("title") {
            capabilities.push("title".to_string());
        } else {
            gaps.push("missing_title_handling".to_string());
        }
        if content.contains("description") || content.contains("meta") {
            capabilities.push("meta_description".to_string());
        } else {
            gaps.push("missing_meta_description".to_string());
        }
        if content.contains("canonical") || content.contains("rel: 'canonical'") {
            capabilities.push("canonical".to_string());
        } else {
            gaps.push("missing_canonical".to_string());
        }
        if content.contains("og:") || content.contains("openGraph") || content.contains("ogImage") || content.contains("og:image") {
            capabilities.push("open_graph".to_string());
        } else {
            gaps.push("missing_open_graph".to_string());
        }
        if content.contains("twitter:") || content.contains("twitterImage") {
            capabilities.push("twitter_cards".to_string());
        }
        if content.contains("json") || content.contains("ld+json") || content.contains("schema") || content.contains("structuredData") {
            capabilities.push("json_ld".to_string());
        } else {
            gaps.push("missing_json_ld".to_string());
        }
        if content.contains("lastModified") || content.contains("last-modified") || content.contains("modified_time") || content.contains("dateModified") {
            capabilities.push("last_modified".to_string());
        }
        if content.contains("hreflang") || content.contains("alternate") {
            capabilities.push("hreflang".to_string());
        }
        if content.contains("viewport") || content.contains("width=device-width") {
            capabilities.push("viewport".to_string());
        } else {
            gaps.push("missing_viewport".to_string());
        }
        if content.contains("robots") || content.contains("noindex") {
            capabilities.push("robots_meta".to_string());
        }
        if content.contains("Article") || content.contains("BlogPosting") {
            capabilities.push("article_schema".to_string());
        }
    } else {
        gaps.push("missing_seo_head_component".to_string());
    }

    // Also check index.html and App.vue for viewport if not in SEOHead
    if !capabilities.contains(&"viewport".to_string()) {
        for alt_path in ["src/App.vue", "index.html", "public/index.html"] {
            let alt = root.join(alt_path);
            if alt.exists() {
                let content = std::fs::read_to_string(&alt).unwrap_or_default();
                if content.contains("viewport") || content.contains("width=device-width") {
                    capabilities.push("viewport".to_string());
                    gaps.retain(|g| g != "missing_viewport");
                    break;
                }
            }
        }
    }

    Ok(TemplateAudit {
        seo_head_exists: seo_head_path.is_some(),
        seo_head_path,
        capabilities_detected: capabilities,
        gaps_detected: gaps,
        source_summary,
    })
}

fn collect_build_output_audit(project_path: &str) -> Result<BuildOutputAudit, String> {
    let root = std::path::Path::new(project_path);
    let dist = root.join("dist");

    if !dist.exists() {
        return Ok(BuildOutputAudit {
            has_built_output: false,
            has_sitemap: false,
            has_robots_txt: false,
            has_404_page: false,
            html_file_count: 0,
            sampled_pages: Vec::new(),
        });
    }

    // Check for sitemap, robots, 404
    let has_sitemap = dist.join("sitemap.xml").exists()
        || dist.join("sitemap-index.xml").exists()
        || root.join("public/sitemap.xml").exists();
    let has_robots_txt = dist.join("robots.txt").exists()
        || root.join("public/robots.txt").exists();
    let has_404 = dist.join("404.html").exists()
        || dist.join("404/index.html").exists();

    // Count HTML files
    let mut html_count = 0i64;
    fn count_html(dir: &std::path::Path, count: &mut i64) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    count_html(&path, count);
                } else if path.extension().and_then(|e| e.to_str()) == Some("html") {
                    *count += 1;
                }
            }
        }
    }
    count_html(&dist, &mut html_count);

    // Sample built pages for meta coverage
    let sampled = sample_built_pages(root, &dist);

    Ok(BuildOutputAudit {
        has_built_output: html_count > 0,
        has_sitemap,
        has_robots_txt,
        has_404_page: has_404,
        html_file_count: html_count,
        sampled_pages: sampled,
    })
}

fn sample_built_pages(root: &std::path::Path, dist: &std::path::Path) -> Vec<BuiltPageSample> {
    let mut samples = Vec::new();
    let blog_dir = dist.join("blog");

    let target_dir = if blog_dir.exists() { &blog_dir } else { dist };
    if !target_dir.exists() {
        return samples;
    }

    if let Ok(entries) = std::fs::read_dir(target_dir) {
        for entry in entries.flatten() {
            if samples.len() >= 3 {
                break;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("html") {
                continue;
            }

            let content = std::fs::read_to_string(&path).unwrap_or_default();
            let slug = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();

            let title_re = regex::Regex::new(r"<title>(.*?)</title>").unwrap();
            let has_title = title_re.is_match(&content);
            let title_text = title_re.captures(&content)
                .and_then(|c| c.get(1))
                .map(|m| char_limit(m.as_str(), 80).to_string())
                .unwrap_or_default();

            let desc_re = regex::Regex::new(r#"<meta[^>]*name=["']description["'][^>]*content=["']([^"']*)["'][^>]*>"#).unwrap();
            let has_desc = desc_re.is_match(&content);
            let desc_text = desc_re.captures(&content)
                .and_then(|c| c.get(1))
                .map(|m| char_limit(m.as_str(), 80).to_string())
                .unwrap_or_default();

            let canonical_re = regex::Regex::new(r#"<link[^>]*rel=["']canonical["'][^>]*>"#).unwrap();
            let has_canonical = canonical_re.is_match(&content);

            let og_re = regex::Regex::new(r#"<meta[^>]*property=["']og:"#).unwrap();
            let has_og = og_re.is_match(&content);

            let jsonld_re = regex::Regex::new(r#"<script[^>]*type=["']application/ld\+json["']"#).unwrap();
            let has_jsonld = jsonld_re.is_match(&content);

            let viewport_re = regex::Regex::new(r#"<meta[^>]*name=["']viewport["']"#).unwrap();
            let has_viewport = viewport_re.is_match(&content);

            samples.push(BuiltPageSample {
                slug,
                has_title,
                has_meta_description: has_desc,
                has_canonical,
                has_og_tags: has_og,
                has_json_ld: has_jsonld,
                has_viewport,
                title_text,
                description_text: desc_text,
            });
        }
    }

    samples
}

fn collect_url_architecture(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
) -> Result<UrlArchitecture, String> {
    let articles = crate::engine::task_store::list_articles(conn, project_id)
        .map_err(|e| format!("list_articles: {e}"))?;

    // Temporal URL detection: match year tokens ANYWHERE in the slug.
    // Year suffixes (-2025 at end) are the most critical — they force annual
    // URL migration. Year tokens in the middle are still dated but less urgent.
    let temporal_re = regex::Regex::new(r"-\d{4}").unwrap();
    let prefix_re = regex::Regex::new(r"^\d+-").unwrap();

    let mut temporal_count = 0i64;
    let mut sample_temporal: Vec<String> = Vec::new();

    for a in &articles {
        let slug_no_prefix = prefix_re.replace(&a.url_slug, "");
        if temporal_re.is_match(&slug_no_prefix) {
            temporal_count += 1;
            if sample_temporal.len() < 3 {
                sample_temporal.push(a.url_slug.clone());
            }
        }
    }

    // Check blog parser slug logic
    let slug_logic = check_blog_parser(project_path);

    Ok(UrlArchitecture {
        temporal_url_count: temporal_count,
        sample_temporal_urls: sample_temporal,
        trailing_slash_consistent: None, // Would need router config analysis
        slug_derivation_logic: slug_logic,
    })
}

fn collect_title_quality(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
) -> Result<TitleQualityAudit, String> {
    let articles = crate::engine::task_store::list_articles(conn, project_id)
        .map_err(|e| format!("list_articles: {e}"))?;

    let content_dirs = crate::content::article_resolver::discover_content_dirs(std::path::Path::new(project_path));
    let content_dirs_refs: Vec<&str> = content_dirs.iter().map(|s| s.as_str()).collect();
    let repo_root = std::path::Path::new(project_path);

    let mut truncated: Vec<TitleIssue> = Vec::new();
    let mut title_counts: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();

    // Truncation indicators: titles ending mid-word with these patterns
    let truncation_patterns = [
        ":", " vs", " and", " or", " -", "—", "–", " with", " for", " by",
    ];

    for a in &articles {
        let resolved = crate::content::article_resolver::resolve_article_file(
            repo_root, &a.file, &content_dirs_refs,
        );
        if !resolved.found {
            continue;
        }
        let content = std::fs::read_to_string(&resolved._absolute_path).unwrap_or_default();
        let Some((fm_raw, _)) = crate::content::frontmatter::split_mdx(&content) else {
            continue;
        };
        let Ok(fm) = crate::content::frontmatter::parse(fm_raw) else {
            continue;
        };

        if let Some(title) = fm.parsed.get("title").and_then(|v| v.as_str()) {
            let trimmed = title.trim();

            // Check for truncation: ends with a pattern that suggests mid-sentence cutoff
            let is_truncated = truncation_patterns.iter().any(|pat| trimmed.ends_with(pat));
            if is_truncated && truncated.len() < 5 {
                truncated.push(TitleIssue {
                    slug: a.url_slug.clone(),
                    title: trimmed.to_string(),
                    issue: "title appears truncated (ends mid-sentence)".to_string(),
                });
            }

            // Track for duplicate detection
            let lower = trimmed.to_lowercase();
            if !lower.is_empty() {
                title_counts.entry(lower).or_default().push(a.url_slug.clone());
            }
        }
    }

    // Find duplicates
    let mut duplicate_samples: Vec<DuplicateTitle> = Vec::new();
    for (title, slugs) in &title_counts {
        if slugs.len() > 1 {
            duplicate_samples.push(DuplicateTitle {
                title: title.clone(),
                slugs: slugs.clone(),
            });
        }
    }
    duplicate_samples.sort_by(|a, b| b.slugs.len().cmp(&a.slugs.len()));
    duplicate_samples.truncate(3);

    Ok(TitleQualityAudit {
        truncated_title_count: truncated.len() as i64,
        truncated_samples: truncated,
        duplicate_title_count: duplicate_samples.len() as i64,
        duplicate_samples,
    })
}

fn collect_og_image_audit(project_path: &str) -> Result<OgImageAudit, String> {
    let root = std::path::Path::new(project_path);
    let dist_blog = root.join("dist/blog");

    let mut relative_count = 0i64;
    let mut samples: Vec<OgImageIssue> = Vec::new();

    if !dist_blog.exists() {
        return Ok(OgImageAudit {
            relative_og_image_count: 0,
            sample_relative_og_images: Vec::new(),
        });
    }

    let og_re = regex::Regex::new(r#"<meta[^>]*property=["']og:image["'][^>]*content=["']([^"']*)["']"#).unwrap();

    if let Ok(entries) = std::fs::read_dir(&dist_blog) {
        for entry in entries.flatten() {
            if samples.len() >= 5 {
                break;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("html") {
                continue;
            }
            let content = std::fs::read_to_string(&path).unwrap_or_default();

            if let Some(cap) = og_re.captures(&content) {
                let url = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                // Relative URL: starts with / but not //, or is just a path
                let is_relative = !url.is_empty()
                    && !url.starts_with("http://")
                    && !url.starts_with("https://")
                    && !url.starts_with("//");
                if is_relative {
                    relative_count += 1;
                    let slug = path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string();
                    samples.push(OgImageIssue {
                        slug,
                        og_image_url: url.to_string(),
                    });
                }
            }
        }
    }

    Ok(OgImageAudit {
        relative_og_image_count: relative_count,
        sample_relative_og_images: samples,
    })
}

fn collect_performance_signals(project_path: &str) -> Result<PerformanceSignals, String> {
    let root = std::path::Path::new(project_path);
    let mut config_notes = Vec::new();

    // Check for lazy loading in components
    let mut has_lazy = false;
    let mut has_modern_images = false;
    let mut has_script_opt = false;
    let mut has_bundle = false;

    // Scan src/ for lazy loading patterns
    let entries = walkdir::WalkDir::new(root.join("src"))
        .max_depth(3)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name().to_string_lossy();
            name.ends_with(".vue") || name.ends_with(".tsx") || name.ends_with(".jsx") || name.ends_with(".ts")
        })
        .take(50);
    for entry in entries {
            if let Ok(content) = std::fs::read_to_string(entry.path()) {
                if content.contains("loading=\"lazy\"") || content.contains("loading=\"lazy\"") {
                    has_lazy = true;
                }
                if content.contains(".webp") || content.contains(".avif") || content.contains("format=webp") {
                    has_modern_images = true;
                }
                if content.contains("async") || content.contains("defer") {
                    has_script_opt = true;
                }
            }
        }

    // Check vite config for bundle analysis
    let vite_config = root.join("vite.config.ts");
    if vite_config.exists() {
        if let Ok(content) = std::fs::read_to_string(&vite_config) {
            if content.contains("visualizer") || content.contains("bundle") || content.contains("analyzer") {
                has_bundle = true;
                config_notes.push("vite.config.ts has bundle analyzer plugin".to_string());
            }
            if content.contains("compress") || content.contains("gzip") || content.contains("brotli") {
                config_notes.push("vite.config.ts has compression config".to_string());
            }
        }
    }

    // Check next config
    let next_config = root.join("next.config.js");
    if next_config.exists() {
        config_notes.push("Next.js config found — check for image optimization settings".to_string());
    }

    // Check for image optimization libraries
    let package_json = root.join("package.json");
    if package_json.exists() {
        if let Ok(content) = std::fs::read_to_string(&package_json) {
            if content.contains("@nuxt/image") || content.contains("next/image") || content.contains("vite-imagetools") {
                has_modern_images = true;
                config_notes.push("Image optimization library in dependencies".to_string());
            }
        }
    }

    Ok(PerformanceSignals {
        has_lazy_loading: has_lazy,
        has_modern_image_formats: has_modern_images,
        has_script_optimization: has_script_opt,
        has_bundle_analysis: has_bundle,
        config_notes,
    })
}

fn collect_source_verification(project_path: &str) -> Result<SourceVerification, String> {
    let root = std::path::Path::new(project_path);

    // Key source files
    let mut key_files = Vec::new();
    for (path, purpose) in [
        ("src/components/SEOHead.vue", "Injects meta tags, titles, JSON-LD"),
        ("src/views/BlogPostPage.vue", "Renders blog posts, passes SEO data"),
        ("src/blog/index.ts", "Parses MDX, derives slugs"),
        ("src/App.vue", "Root layout"),
        ("vite.config.ts", "Build configuration"),
        ("src/router/index.ts", "URL routing rules"),
    ] {
        if root.join(path).exists() {
            key_files.push(SourceFileNote {
                path: path.to_string(),
                purpose: purpose.to_string(),
            });
        }
    }

    // Sample frontmatter
    let fm_check = sample_frontmatter(root);

    Ok(SourceVerification {
        key_source_files: key_files,
        frontmatter_sample: fm_check,
    })
}

fn sample_frontmatter(root: &std::path::Path) -> FrontmatterSampleCheck {
    let mut checked = 0i64;
    let mut malformed = 0i64;
    let mut all_have_title = true;
    let mut all_have_desc = true;

    // Try common content directories
    let content_dirs = [
        root.join("src/blog/posts"),
        root.join("content"),
        root.join("src/content"),
        root.join("src/posts"),
    ];

    for content_dir in &content_dirs {
        if !content_dir.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(content_dir) {
            for entry in entries.flatten() {
                if checked >= 3 {
                    break;
                }
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("mdx") {
                    continue;
                }
                checked += 1;
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                match crate::content::frontmatter::split_mdx(&content) {
                    Some((fm_raw, _)) => match crate::content::frontmatter::parse(fm_raw) {
                        Ok(fm) => {
                            let parsed = &fm.parsed;
                            let has_title = parsed.get("title").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false);
                            let has_desc = parsed.get("description").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false);
                            if !has_title {
                                all_have_title = false;
                            }
                            if !has_desc {
                                all_have_desc = false;
                            }
                        }
                        Err(_) => {
                            malformed += 1;
                        }
                    },
                    None => {
                        malformed += 1;
                    }
                }
            }
        }
        if checked > 0 {
            break; // Stop after first valid content dir
        }
    }

    FrontmatterSampleCheck {
        files_checked: checked,
        all_have_title,
        all_have_description: all_have_desc,
        malformed_count: malformed,
    }
}

fn check_blog_parser(project_path: &str) -> String {
    let root = std::path::Path::new(project_path);
    let parser = root.join("src/blog/index.ts");
    if !parser.exists() {
        let alt = root.join("src/content/blog.ts");
        if alt.exists() {
            return "Blog parser found at src/content/blog.ts".to_string();
        }
        return "Blog parser not found at expected location".to_string();
    }
    let content = std::fs::read_to_string(&parser).unwrap_or_default();

    if content.contains("replace") && content.contains("YYYY") {
        return "Strips date prefixes from filenames when deriving slugs".to_string();
    }
    if content.contains("slug") && content.contains("replace") {
        return "Applies transformations when deriving slugs from filenames".to_string();
    }
    if content.contains("slug") {
        return "Has custom slug derivation logic".to_string();
    }
    "Found but slug logic could not be determined".to_string()
}

fn summarize_source(content: &str, max_chars: usize) -> String {
    let limited = char_limit(content, max_chars);
    // Remove excessive whitespace
    limited.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn char_limit(s: &str, max_chars: usize) -> &str {
    if let Some((idx, _)) = s.char_indices().nth(max_chars) {
        &s[..idx]
    } else {
        s
    }
}
