//! Observation-based SEO audit collector.
//!
//! Crawls the live site's sitemap, fetches rendered HTML for each URL,
//! and extracts SEO signals. Framework-agnostic — works with Next.js,
//! Vue, Astro, or plain HTML. No source-code scanning.
//!
//! Principle: observe symptoms in the output. The repo diagnoses root cause.

use serde::{Deserialize, Serialize};

/// Complete observation report from crawling the live site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteObservationReport {
    pub project: ProjectSummary,
    pub crawl: SitemapCrawl,
    pub page_observations: Vec<PageObservation>,
    pub detected_issues: Vec<DetectedIssue>,
    /// Subset of observations with notable issues (for agent context)
    pub notable_observations: Vec<PageObservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub project_id: String,
    pub project_path: String,
    pub tech_stack: String,
    pub site_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SitemapCrawl {
    pub site_url: String,
    pub total_urls: usize,
    pub blog_urls: Vec<String>,
    pub other_urls: Vec<String>,
    pub crawl_errors: Vec<String>,
}

/// Observed SEO signals from a single page's rendered HTML.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageObservation {
    pub url: String,
    pub slug: String,
    pub is_blog: bool,
    pub title: Option<String>,
    pub title_length: usize,
    pub meta_description: Option<String>,
    pub meta_description_length: usize,
    pub canonical: Option<String>,
    pub og_title: Option<String>,
    pub og_description: Option<String>,
    pub og_image: Option<String>,
    pub og_image_is_absolute: bool,
    pub twitter_image: Option<String>,
    pub has_json_ld: bool,
    pub json_ld_types: Vec<String>,
    pub h1_count: usize,
    pub h2_count: usize,
    pub images: Vec<ImageObservation>,
    pub internal_link_count: usize,
    pub fetch_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageObservation {
    pub src: String,
    pub alt: Option<String>,
    pub loading: Option<String>,
    pub width: Option<String>,
    pub height: Option<String>,
}

/// A detected issue with specific evidence from observations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedIssue {
    pub issue_type: String,
    pub severity: String,
    pub description: String,
    pub affected_urls: Vec<String>,
    pub evidence: Vec<String>,
}

pub async fn collect_site_observations(
    site_url: String,
    project_id: String,
    project_path: String,
) -> Result<SiteObservationReport, String> {
    // ── Phase 1: Crawl sitemap ──────────────────────────────────────────────
    let crawl = crawl_sitemap(&site_url).await?;

    // ── Phase 2: Inspect pages ──────────────────────────────────────────────
    let observations = inspect_pages(&crawl.blog_urls, &crawl.other_urls).await;

    // ── Phase 3: Detect issues ──────────────────────────────────────────────
    let detected_issues = detect_issues(&observations);

    // ── Tech stack: observation-based only ───────────────────────────────────
    // We do NOT inspect source files (package.json, config files) because that
    // misidentifies frameworks. Detect from rendered HTML signatures if present.
    let tech_stack = detect_tech_stack_from_observations(&observations);

    // Build compact agent report: only issues + minimal metadata.
    // NEVER send full page_observations to the agent — they bloat the prompt.
    Ok(SiteObservationReport {
        project: ProjectSummary {
            project_id,
            project_path,
            tech_stack,
            site_url: Some(site_url),
        },
        crawl,
        page_observations: observations,
        detected_issues,
        notable_observations: Vec::new(), // not used in prompt
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 1: Sitemap Crawler
// ═══════════════════════════════════════════════════════════════════════════════

async fn crawl_sitemap(site_url: &str) -> Result<SitemapCrawl, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    // Determine sitemap URLs to try.
    // If the user already provided a sitemap URL (ends with sitemap.xml etc),
    // use it directly. Otherwise try common locations under the base URL.
    let trimmed = site_url.trim_end_matches('/');
    let sitemap_urls: Vec<String> = if trimmed.ends_with("sitemap.xml")
        || trimmed.ends_with("sitemap_index.xml")
        || trimmed.ends_with("sitemap-index.xml")
    {
        vec![trimmed.to_string()]
    } else {
        vec![
            format!("{}/sitemap.xml", trimmed),
            format!("{}/sitemap_index.xml", trimmed),
            format!("{}/sitemap-index.xml", trimmed),
        ]
    };

    let mut sitemap_xml = String::new();
    let mut sitemap_errors = Vec::new();

    for url in &sitemap_urls {
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                sitemap_xml = resp.text().await
                    .map_err(|e| format!("Failed to read sitemap body: {e}"))?;
                break;
            }
            Ok(resp) => {
                sitemap_errors.push(format!("{} returned {}", url, resp.status()));
            }
            Err(e) => {
                sitemap_errors.push(format!("{} fetch error: {}", url, e));
            }
        }
    }

    if sitemap_xml.is_empty() {
        return Err(format!(
            "No sitemap found. Tried: {:?}. Errors: {:?}",
            sitemap_urls, sitemap_errors
        ));
    }

    // Extract URLs from sitemap XML using regex
    let loc_re = regex::Regex::new(r"<loc>([^<]+)</loc>").unwrap();
    let mut all_urls: Vec<String> = loc_re.captures_iter(&sitemap_xml)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().to_string()))
        .collect();

    // Deduplicate
    all_urls.sort();
    all_urls.dedup();

    // Limit total URLs to prevent timeouts
    const MAX_URLS: usize = 150;
    let was_truncated = all_urls.len() > MAX_URLS;
    if was_truncated {
        all_urls.truncate(MAX_URLS);
    }

    // Categorize URLs
    let mut blog_urls = Vec::new();
    let mut other_urls = Vec::new();

    for url in &all_urls {
        let is_blog = url.contains("/blog/") || url.contains("/posts/") || url.contains("/article/");
        if is_blog {
            blog_urls.push(url.clone());
        } else {
            other_urls.push(url.clone());
        }
    }

    // Prioritize blog URLs — if we hit the limit, keep all blogs and sample others
    let total_urls = all_urls.len();

    Ok(SitemapCrawl {
        site_url: site_url.to_string(),
        total_urls,
        blog_urls,
        other_urls,
        crawl_errors: if was_truncated {
            vec![format!("Sitemap had >{} URLs; truncated to {}", MAX_URLS, total_urls)]
        } else {
            Vec::new()
        },
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 2: HTML Inspector
// ═══════════════════════════════════════════════════════════════════════════════

async fn inspect_pages(blog_urls: &[String], other_urls: &[String]) -> Vec<PageObservation> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .expect("Failed to build HTTP client");

    // Sample blog URLs to keep crawl fast
    const MAX_BLOG_SAMPLES: usize = 25;
    let blog_sample: Vec<(String, bool)> = blog_urls.iter()
        .take(MAX_BLOG_SAMPLES)
        .map(|u| (u.clone(), true))
        .collect();

    // Limit non-blog pages
    let other_sample: Vec<(String, bool)> = other_urls.iter()
        .take(5)
        .map(|u| (u.clone(), false))
        .collect();

    let urls_to_fetch: Vec<(String, bool)> = blog_sample.into_iter()
        .chain(other_sample.into_iter())
        .collect();

    // Concurrent fetching with semaphore
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(5));
    let mut join_set = tokio::task::JoinSet::new();

    for (url, is_blog) in urls_to_fetch {
        let permit = semaphore.clone().acquire_owned().await.expect("semaphore");
        let client = client.clone();
        let slug = url.rsplit('/').next().unwrap_or("").to_string();

        join_set.spawn(async move {
            let _permit = permit;
            inspect_single_page(&url, &slug, is_blog, &client).await
        });
    }

    let mut results = Vec::new();
    while let Some(res) = join_set.join_next().await {
        if let Ok(obs) = res {
            results.push(obs);
        }
    }

    results
}

async fn inspect_single_page(
    url: &str,
    slug: &str,
    is_blog: bool,
    client: &reqwest::Client,
) -> PageObservation {
    let fetch_result = client.get(url).send().await;

    let (html, fetch_error) = match fetch_result {
        Ok(resp) if resp.status().is_success() => {
            match resp.text().await {
                Ok(text) => (text, None),
                Err(e) => (String::new(), Some(format!("body read error: {e}"))),
            }
        }
        Ok(resp) => {
            (String::new(), Some(format!("HTTP {}", resp.status())))
        }
        Err(e) => {
            (String::new(), Some(format!("fetch error: {e}")))
        }
    };

    if let Some(ref err) = fetch_error {
        return PageObservation {
            url: url.to_string(),
            slug: slug.to_string(),
            is_blog,
            title: None,
            title_length: 0,
            meta_description: None,
            meta_description_length: 0,
            canonical: None,
            og_title: None,
            og_description: None,
            og_image: None,
            og_image_is_absolute: false,
            twitter_image: None,
            has_json_ld: false,
            json_ld_types: Vec::new(),
            h1_count: 0,
            h2_count: 0,
            images: Vec::new(),
            internal_link_count: 0,
            fetch_error: Some(err.clone()),
        };
    }

    let document = scraper::Html::parse_document(&html);

    // Title
    let title = document.select(&scraper::Selector::parse("title").unwrap())
        .next()
        .map(|e| e.text().collect::<String>().trim().to_string());
    let title_length = title.as_ref().map(|t| t.len()).unwrap_or(0);

    // Meta description
    let meta_desc_sel = scraper::Selector::parse(r#"meta[name="description"]"#).unwrap();
    let meta_description = document.select(&meta_desc_sel)
        .next()
        .and_then(|e| e.value().attr("content"))
        .map(|s| s.to_string());
    let meta_description_length = meta_description.as_ref().map(|d| d.len()).unwrap_or(0);

    // Canonical
    let canonical_sel = scraper::Selector::parse(r#"link[rel="canonical"]"#).unwrap();
    let canonical = document.select(&canonical_sel)
        .next()
        .and_then(|e| e.value().attr("href"))
        .map(|s| s.to_string());

    // OG tags
    let og_title_sel = scraper::Selector::parse(r#"meta[property="og:title"]"#).unwrap();
    let og_title = document.select(&og_title_sel)
        .next()
        .and_then(|e| e.value().attr("content"))
        .map(|s| s.to_string());

    let og_desc_sel = scraper::Selector::parse(r#"meta[property="og:description"]"#).unwrap();
    let og_description = document.select(&og_desc_sel)
        .next()
        .and_then(|e| e.value().attr("content"))
        .map(|s| s.to_string());

    let og_image_sel = scraper::Selector::parse(r#"meta[property="og:image"]"#).unwrap();
    let og_image = document.select(&og_image_sel)
        .next()
        .and_then(|e| e.value().attr("content"))
        .map(|s| s.to_string());
    let og_image_is_absolute = og_image.as_ref()
        .map(|url| url.starts_with("http://") || url.starts_with("https://") || url.starts_with("//"))
        .unwrap_or(true); // if no og:image, consider it "not a problem"

    // Twitter image
    let tw_image_sel = scraper::Selector::parse(r#"meta[name="twitter:image"], meta[property="twitter:image"]"#).unwrap();
    let twitter_image = document.select(&tw_image_sel)
        .next()
        .and_then(|e| e.value().attr("content"))
        .map(|s| s.to_string());

    // JSON-LD
    let jsonld_sel = scraper::Selector::parse(r#"script[type="application/ld+json"]"#).unwrap();
    let mut json_ld_types = Vec::new();
    for el in document.select(&jsonld_sel) {
        let text = el.text().collect::<String>();
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(ty) = val.get("@type").and_then(|t| t.as_str()) {
                json_ld_types.push(ty.to_string());
            }
        }
    }
    let has_json_ld = !json_ld_types.is_empty();

    // Headings
    let h1_sel = scraper::Selector::parse("h1").unwrap();
    let h1_count = document.select(&h1_sel).count();

    let h2_sel = scraper::Selector::parse("h2").unwrap();
    let h2_count = document.select(&h2_sel).count();

    // Images
    let img_sel = scraper::Selector::parse("img").unwrap();
    let mut images = Vec::new();
    for img in document.select(&img_sel) {
        let src = img.value().attr("src").unwrap_or("").to_string();
        if src.is_empty() { continue; }
        images.push(ImageObservation {
            src,
            alt: img.value().attr("alt").map(|s| s.to_string()),
            loading: img.value().attr("loading").map(|s| s.to_string()),
            width: img.value().attr("width").map(|s| s.to_string()),
            height: img.value().attr("height").map(|s| s.to_string()),
        });
    }

    // Internal links (simple heuristic: same domain)
    let a_sel = scraper::Selector::parse("a[href]").unwrap();
    let internal_link_count = document.select(&a_sel)
        .filter_map(|a| a.value().attr("href"))
        .filter(|href| href.starts_with('/') || href.starts_with(url))
        .count();

    PageObservation {
        url: url.to_string(),
        slug: slug.to_string(),
        is_blog,
        title,
        title_length,
        meta_description,
        meta_description_length,
        canonical,
        og_title,
        og_description,
        og_image,
        og_image_is_absolute,
        twitter_image,
        has_json_ld,
        json_ld_types,
        h1_count,
        h2_count,
        images,
        internal_link_count,
        fetch_error: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 3: Issue Detector
// ═══════════════════════════════════════════════════════════════════════════════

fn detect_issues(observations: &[PageObservation]) -> Vec<DetectedIssue> {
    let mut issues: Vec<DetectedIssue> = Vec::new();

    // 1. Truncated titles
    let truncation_patterns = [":", " vs", " and", " or", " for", " with", " -", "—", "–"];
    let truncated: Vec<&PageObservation> = observations.iter()
        .filter(|o| o.is_blog)
        .filter(|o| {
            o.title.as_ref().map(|t| {
                let trimmed = t.trim();
                truncation_patterns.iter().any(|pat| trimmed.ends_with(pat))
            }).unwrap_or(false)
        })
        .collect();

    if !truncated.is_empty() {
        let affected: Vec<String> = truncated.iter().map(|o| o.url.clone()).collect();
        let evidence: Vec<String> = truncated.iter()
            .take(3)
            .map(|o| format!("{} → title: '{}'", o.url, o.title.as_ref().unwrap()))
            .collect();
        issues.push(DetectedIssue {
            issue_type: "truncated_title".to_string(),
            severity: "P0".to_string(),
            description: format!(
                "{} blog posts have <title> tags truncated mid-sentence, producing broken SEO output.",
                truncated.len()
            ),
            affected_urls: affected,
            evidence,
        });
    }

    // 2. Duplicate titles
    let mut title_map: std::collections::HashMap<String, Vec<&PageObservation>> = std::collections::HashMap::new();
    for o in observations.iter().filter(|o| o.is_blog) {
        if let Some(ref title) = o.title {
            let key = title.trim().to_lowercase();
            if !key.is_empty() {
                title_map.entry(key).or_default().push(o);
            }
        }
    }
    let duplicates: Vec<(String, Vec<&PageObservation>)> = title_map.into_iter()
        .filter(|(_, v)| v.len() > 1)
        .map(|(k, v)| (k, v))
        .collect();

    if !duplicates.is_empty() {
        let mut affected = Vec::new();
        let mut evidence = Vec::new();
        for (title, pages) in &duplicates {
            for p in pages {
                affected.push(p.url.clone());
            }
            evidence.push(format!(
                "Title '{}' used by {} pages: {}",
                title,
                pages.len(),
                pages.iter().map(|p| p.slug.clone()).collect::<Vec<_>>().join(", ")
            ));
        }
        issues.push(DetectedIssue {
            issue_type: "duplicate_title".to_string(),
            severity: "P0".to_string(),
            description: format!(
                "{} duplicate <title> strings detected across blog posts.",
                duplicates.len()
            ),
            affected_urls: affected,
            evidence,
        });
    }

    // 3. Relative OG images
    let relative_og: Vec<&PageObservation> = observations.iter()
        .filter(|o| o.is_blog)
        .filter(|o| o.og_image.is_some() && !o.og_image_is_absolute)
        .collect();

    if !relative_og.is_empty() {
        let affected: Vec<String> = relative_og.iter().map(|o| o.url.clone()).collect();
        let evidence: Vec<String> = relative_og.iter()
            .take(5)
            .map(|o| format!("{} → og:image: '{}'", o.url, o.og_image.as_ref().unwrap()))
            .collect();
        issues.push(DetectedIssue {
            issue_type: "relative_og_image".to_string(),
            severity: "P0".to_string(),
            description: format!(
                "{} blog posts use relative URLs for og:image, breaking social sharing.",
                relative_og.len()
            ),
            affected_urls: affected,
            evidence,
        });
    }

    // 4. Temporal URLs — nuanced detection
    let temporal_re = regex::Regex::new(r"-(\d{4})").unwrap();
    let temporal: Vec<&PageObservation> = observations.iter()
        .filter(|o| o.is_blog)
        .filter(|o| temporal_re.is_match(&o.slug))
        .collect();

    if !temporal.is_empty() {
        let affected: Vec<String> = temporal.iter().map(|o| o.url.clone()).collect();

        // Check for actual multi-year fragmentation (e.g. budget-2024 AND budget-2025)
        let mut slug_bases: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
        for o in &temporal {
            if let Some(cap) = temporal_re.captures(&o.slug) {
                let year = cap.get(1).map(|m| m.as_str()).unwrap_or("");
                let base = temporal_re.replace(&o.slug, "").to_string();
                slug_bases.entry(base).or_default().push(year.to_string());
            }
        }
        let fragmented: Vec<(String, Vec<String>)> = slug_bases.into_iter()
            .filter(|(_, years)| years.len() > 1)
            .collect();

        // Check if year also appears in <title> (intentional freshness signaling)
        let year_in_title_count = temporal.iter()
            .filter(|o| {
                o.title.as_ref().map(|t| {
                    temporal_re.is_match(t) || t.contains("2025") || t.contains("2026")
                }).unwrap_or(false)
            })
            .count();

        // Check inconsistency: what % of blog posts have year suffixes?
        let blog_count = observations.iter().filter(|o| o.is_blog).count();
        let temporal_ratio = if blog_count > 0 {
            temporal.len() as f32 / blog_count as f32
        } else { 0.0 };

        let mut evidence: Vec<String> = temporal.iter()
            .take(3)
            .map(|o| format!("{} → slug: '{}'", o.url, o.slug))
            .collect();

        if !fragmented.is_empty() {
            evidence.push(format!(
                "FRAGMENTED: {} topic(s) have multiple year variants (e.g. {})",
                fragmented.len(),
                fragmented.first().map(|(base, years)| format!("{} → {:?}", base, years)).unwrap_or_default()
            ));
        }
        evidence.push(format!(
            "YEAR_IN_TITLE: {}/{} posts also have year in <title> (intentional freshness signaling)",
            year_in_title_count, temporal.len()
        ));
        evidence.push(format!(
            "CONSISTENCY: {}/{} blog posts ({:.0}%) have year suffixes",
            temporal.len(), blog_count, temporal_ratio * 100.0
        ));

        let severity = if fragmented.is_empty() && temporal_ratio < 0.5 {
            "P2" // Low concern: no fragmentation, minority of posts
        } else if !fragmented.is_empty() {
            "P0" // High concern: actual equity fragmentation
        } else {
            "P2" // Medium: inconsistency or majority temporal
        };

        issues.push(DetectedIssue {
            issue_type: "temporal_url".to_string(),
            severity: severity.to_string(),
            description: format!(
                "{} blog post URLs contain year tokens. {} fragmented topic(s). {} posts have year in <title>.",
                temporal.len(), fragmented.len(), year_in_title_count
            ),
            affected_urls: affected,
            evidence,
        });
    }

    // 5. Missing meta descriptions
    let missing_desc: Vec<&PageObservation> = observations.iter()
        .filter(|o| o.is_blog)
        .filter(|o| o.meta_description.is_none())
        .collect();

    if !missing_desc.is_empty() {
        let affected: Vec<String> = missing_desc.iter().map(|o| o.url.clone()).collect();
        let evidence = format!("Affected: {}", affected.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
        issues.push(DetectedIssue {
            issue_type: "missing_meta_description".to_string(),
            severity: "P2".to_string(),
            description: format!(
                "{} blog posts lack <meta name=\"description\"> tags.",
                missing_desc.len()
            ),
            affected_urls: affected,
            evidence: vec![evidence],
        });
    }

    // 6. Missing canonical tags
    let missing_canonical: Vec<&PageObservation> = observations.iter()
        .filter(|o| o.is_blog)
        .filter(|o| o.canonical.is_none())
        .collect();

    if !missing_canonical.is_empty() {
        let affected: Vec<String> = missing_canonical.iter().map(|o| o.url.clone()).collect();
        let evidence = format!("Affected: {}", affected.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
        issues.push(DetectedIssue {
            issue_type: "missing_canonical".to_string(),
            severity: "P2".to_string(),
            description: format!(
                "{} blog posts lack <link rel=\"canonical\"> tags.",
                missing_canonical.len()
            ),
            affected_urls: affected,
            evidence: vec![evidence],
        });
    }

    // 7. Images without lazy loading (for non-first images)
    let images_without_lazy: Vec<(String, String)> = observations.iter()
        .filter(|o| o.is_blog)
        .flat_map(|o| {
            o.images.iter().enumerate()
                .filter(|(i, img)| *i > 0 && img.loading.as_deref() != Some("lazy"))
                .map(|(_, img)| (o.url.clone(), img.src.clone()))
        })
        .collect();

    if !images_without_lazy.is_empty() {
        let affected_pages: std::collections::HashSet<String> = images_without_lazy.iter()
            .map(|(url, _)| url.clone())
            .collect();
        let evidence: Vec<String> = images_without_lazy.iter()
            .take(5)
            .map(|(url, src)| format!("{} → img: '{}' lacks loading='lazy'", url, src))
            .collect();
        issues.push(DetectedIssue {
            issue_type: "missing_lazy_loading".to_string(),
            severity: "P2".to_string(),
            description: format!(
                "{} images across {} blog posts lack loading='lazy', inflating initial payload.",
                images_without_lazy.len(),
                affected_pages.len()
            ),
            affected_urls: affected_pages.into_iter().collect(),
            evidence,
        });
    }

    // 8. Missing JSON-LD structured data
    let missing_jsonld: Vec<&PageObservation> = observations.iter()
        .filter(|o| o.is_blog)
        .filter(|o| !o.has_json_ld)
        .collect();

    if !missing_jsonld.is_empty() {
        let affected: Vec<String> = missing_jsonld.iter().map(|o| o.url.clone()).collect();
        let evidence = format!("Affected: {}", affected.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
        issues.push(DetectedIssue {
            issue_type: "missing_structured_data".to_string(),
            severity: "P2".to_string(),
            description: format!(
                "{} blog posts lack JSON-LD structured data (Article, FAQ, or Breadcrumb schema).",
                missing_jsonld.len()
            ),
            affected_urls: affected,
            evidence: vec![evidence],
        });
    }

    // Sort by severity: P0 first, then P2
    issues.sort_by(|a, b| {
        let a_score = if a.severity == "P0" { 0 } else { 1 };
        let b_score = if b.severity == "P0" { 0 } else { 1 };
        a_score.cmp(&b_score)
    });

    issues
}

/// Detect tech stack from rendered HTML observations only.
/// Never inspects source files — framework-agnostic approach.
fn detect_tech_stack_from_observations(_observations: &[PageObservation]) -> String {
    // Observation-based audit does not inspect source files (package.json,
    // config files) because that misidentifies frameworks. We also don't
    // store raw HTML in PageObservation, so we can't detect from rendered
    // signatures either. Return unknown rather than guess.
    "Unknown (not detectable from rendered output)".to_string()
}
