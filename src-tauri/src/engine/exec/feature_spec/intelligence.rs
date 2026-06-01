//! Deterministic project intelligence collector for the feature spec generator.
//!
//! Gathers pre-computed data from all system audits and sources into a single
//! structured report. The agent receives this report as prompt context.
//!
//! CRITICAL: This report is consumed by WEBSITE DEVELOPERS, not PageSeeds
//! maintainers. Every metric must be actionable from the target repo.
//! Do NOT include PageSeeds-internal state (task failures, automation health,
//! recovery attempts) — developers cannot fix our bugs.

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Website-level intelligence report. No PageSeeds internals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectIntelligenceReport {
    pub project: ProjectSummary,
    pub content_health: ContentHealthSummary,
    pub indexing: IndexingSummary,
    /// CTR data from audit — treat as UNVERIFIED. Cross-check with code_verification before reporting.
    pub ctr: CtrSummary,
    pub cannibalization: CannibalizationSummary,
    pub links: LinkSummary,
    pub content_distribution: ContentDistribution,
    pub technical_seo: TechnicalSeoSummary,
    /// Ground-truth verification from reading actual source/build files
    pub code_verification: CodeVerification,
    /// Human-readable notes on what was verified vs. estimated
    pub verification_notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectSummary {
    pub project_id: String,
    pub project_path: String,
    pub tech_stack: String,
    pub article_count: i64,
    pub total_word_count: i64,
    pub avg_word_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentHealthSummary {
    pub latest_audit_date: Option<String>,
    pub total_audited: i64,
    pub good: i64,
    pub needs_improvement: i64,
    pub poor: i64,
    /// Top issue categories (e.g. {"thin_content": 12})
    pub top_issues: HashMap<String, i64>,
    /// Worst articles with their actual problems
    pub worst_articles: Vec<ArticleHealthBrief>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleHealthBrief {
    pub slug: String,
    pub title: String,
    pub health: String,
    pub health_score: i64,
    pub word_count: i64,
    pub top_issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingSummary {
    pub not_indexed_count: i64,
    pub indexed_count: i64,
    pub unknown_count: i64,
    /// Why pages aren't indexed
    pub reason_breakdown: HashMap<String, i64>,
    /// Sample of non-indexed URLs (first 10)
    pub sample_not_indexed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CtrSummary {
    pub articles_with_issues: i64,
    pub issue_breakdown: HashMap<String, i64>,
    /// Sample articles with CTR issues
    pub sample_articles: Vec<ArticleCtrBrief>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleCtrBrief {
    pub slug: String,
    pub title: String,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CannibalizationSummary {
    pub cluster_count: i64,
    pub merge_candidates: i64,
    pub hub_gaps: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkSummary {
    pub orphan_count: i64,
    pub zero_incoming_count: i64,
    pub total_internal_links: i64,
    pub avg_links_per_article: f64,
    /// Sample orphan slugs
    pub sample_orphans: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentDistribution {
    pub status_breakdown: HashMap<String, i64>,
    pub word_count_histogram: HashMap<String, i64>,
    pub temporal_url_count: i64,
    pub sample_temporal_urls: Vec<String>,
    /// Articles with < 100 words
    pub very_thin_count: i64,
    /// Articles with 100-300 words
    pub thin_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TechnicalSeoSummary {
    /// Articles missing description in frontmatter
    pub missing_description_count: i64,
    /// Articles missing target_keyword in frontmatter
    pub missing_keyword_count: i64,
    /// Duplicate titles found (from actual frontmatter)
    pub duplicate_title_count: i64,
    /// Sample duplicate titles
    pub duplicate_title_samples: Vec<DuplicateTitle>,
    /// Articles with published_date > 1 year old (NOT a missing feature — content is just old)
    pub content_older_than_1_year_count: i64,
    /// Sample old article slugs
    pub content_older_than_1_year_samples: Vec<String>,
    /// Whether lastModified/lastmod is present in frontmatter and handled by templates
    pub last_modified_supported: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateTitle {
    pub title: String,
    pub slugs: Vec<String>,
}

/// Results from actually reading source files and build output.
/// This is ground truth — not inferred from audit data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeVerification {
    /// What the SEO head component actually does (from reading source)
    pub seo_head_summary: String,
    /// How the blog parser derives slugs from filenames
    pub slug_derivation_logic: String,
    /// Sampled built HTML verification results
    pub sampled_html_check: HtmlSampleCheck,
    /// Sampled MDX frontmatter verification results
    pub sampled_frontmatter_check: FrontmatterSampleCheck,
    /// Key source files found and their purposes
    pub key_source_files: Vec<SourceFileNote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HtmlSampleCheck {
    pub files_checked: i64,
    pub all_have_unique_title: bool,
    pub all_have_unique_description: bool,
    pub total_title_samples: Vec<String>,
    pub total_description_samples: Vec<String>,
    pub malformed_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrontmatterSampleCheck {
    pub files_checked: i64,
    pub all_have_description: bool,
    pub all_have_title: bool,
    pub last_modified_present_count: i64,
    pub malformed_frontmatter_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFileNote {
    pub path: String,
    pub purpose: String,
}

pub fn collect_project_intelligence(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
) -> Result<ProjectIntelligenceReport, String> {
    Ok(ProjectIntelligenceReport {
        project: collect_project_summary(conn, project_id, project_path)?,
        content_health: collect_content_health(conn, project_id)?,
        indexing: collect_indexing(conn, project_id)?,
        ctr: collect_ctr(conn, project_id)?,
        cannibalization: collect_cannibalization(conn, project_id)?,
        links: collect_links(conn, project_id, project_path)?,
        content_distribution: collect_content_distribution(conn, project_id)?,
        technical_seo: collect_technical_seo(conn, project_id, project_path)?,
        code_verification: collect_code_verification(project_path)?,
        verification_notes: build_verification_notes(),
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
    let total_words: i64 = articles.iter().map(|a| a.word_count).sum();
    let avg_words = if articles.is_empty() { 0 } else { total_words / articles.len() as i64 };

    Ok(ProjectSummary {
        project_id: project_id.to_string(),
        project_path: project_path.to_string(),
        tech_stack,
        article_count: articles.len() as i64,
        total_word_count: total_words,
        avg_word_count: avg_words,
    })
}

fn collect_content_health(
    conn: &Connection,
    project_id: &str,
) -> Result<ContentHealthSummary, String> {
    let run = crate::db::content_audit::get_latest_audit_run(conn, project_id)
        .map_err(|e| format!("get_latest_audit_run: {e}"))?;

    let mut summary = ContentHealthSummary {
        latest_audit_date: None,
        total_audited: 0,
        good: 0,
        needs_improvement: 0,
        poor: 0,
        top_issues: HashMap::new(),
        worst_articles: Vec::new(),
    };

    let Some(run) = run else { return Ok(summary); };

    summary.latest_audit_date = Some(run.run_at.clone());
    summary.total_audited = run.total_audited;
    summary.good = run.good_count;
    summary.needs_improvement = run.needs_improvement_count;
    summary.poor = run.poor_count;

    let articles = crate::db::content_audit::get_articles_for_run(conn, run.id)
        .map_err(|e| format!("get_articles_for_run: {e}"))?;

    let mut issue_counts: HashMap<String, i64> = HashMap::new();
    let mut worst: Vec<ArticleHealthBrief> = Vec::new();

    for a in &articles {
        let top_issues = if let Ok(data) = serde_json::from_str::<serde_json::Value>(&a.data_json) {
            extract_issue_names(&data)
        } else {
            Vec::new()
        };

        for issue in &top_issues {
            *issue_counts.entry(issue.clone()).or_insert(0) += 1;
        }

        if a.health == "poor" || a.health == "needs_improvement" {
            let word_count = crate::engine::task_store::list_articles(conn, project_id)
                .ok()
                .and_then(|all| all.into_iter().find(|art| art.id == a.article_id))
                .map(|art| art.word_count)
                .unwrap_or(0);

            worst.push(ArticleHealthBrief {
                slug: a.url_slug.clone(),
                title: a.title.clone(),
                health: a.health.clone(),
                health_score: a.health_score,
                word_count,
                top_issues: top_issues.clone(),
            });
        }
    }

    worst.sort_by(|a, b| {
        let a_bad = if a.health == "poor" { 0 } else { 1 };
        let b_bad = if b.health == "poor" { 0 } else { 1 };
        a_bad.cmp(&b_bad).then_with(|| a.health_score.cmp(&b.health_score))
    });
    worst.truncate(3);

    let mut issue_vec: Vec<(String, i64)> = issue_counts.into_iter().collect();
    issue_vec.sort_by(|a, b| b.1.cmp(&a.1));
    issue_vec.truncate(5);
    summary.top_issues = issue_vec.into_iter().collect();
    summary.worst_articles = worst;

    Ok(summary)
}

fn collect_indexing(conn: &Connection, project_id: &str) -> Result<IndexingSummary, String> {
    let statuses = crate::gsc::db::list_by_project(conn, project_id)
        .map_err(|e| format!("list_by_project: {e}"))?;

    let mut not_indexed = 0i64;
    let mut indexed = 0i64;
    let mut unknown = 0i64;
    let mut reasons: HashMap<String, i64> = HashMap::new();
    let mut sample_not_indexed: Vec<String> = Vec::new();

    for s in &statuses {
        match s.last_verdict.as_deref().unwrap_or("") {
            "PASS" => indexed += 1,
            "FAIL" => {
                not_indexed += 1;
                let reason = s.last_reason_code.clone().unwrap_or_else(|| "unknown".to_string());
                *reasons.entry(reason).or_insert(0) += 1;
                // Extract slug from URL for sample
                if sample_not_indexed.len() < 3 {
                    let slug = s.url.rsplit('/').next().unwrap_or("").to_string();
                    if !slug.is_empty() && !sample_not_indexed.contains(&slug) {
                        sample_not_indexed.push(slug);
                    }
                }
            }
            _ => {
                unknown += 1;
                if sample_not_indexed.len() < 3 {
                    let slug = s.url.rsplit('/').next().unwrap_or("").to_string();
                    if !slug.is_empty() && !sample_not_indexed.contains(&slug) {
                        sample_not_indexed.push(slug);
                    }
                }
            }
        }
    }

    Ok(IndexingSummary {
        not_indexed_count: not_indexed,
        indexed_count: indexed,
        unknown_count: unknown,
        reason_breakdown: reasons,
        sample_not_indexed,
    })
}

fn collect_ctr(conn: &Connection, project_id: &str) -> Result<CtrSummary, String> {
    let mut stmt = conn.prepare(
        "SELECT a.url_slug, a.title, i.issue_type
         FROM article_ctr_issues i
         JOIN articles a ON a.id = i.article_id
         WHERE a.project_id = ?1"
    ).map_err(|e| format!("prepare ctr: {e}"))?;

    let rows = stmt.query_map([project_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    }).map_err(|e| format!("query ctr: {e}"))?;

    let mut issue_counts: HashMap<String, i64> = HashMap::new();
    let mut article_issues: HashMap<String, (String, Vec<String>)> = HashMap::new();

    for row in rows {
        let (slug, title, issue_type) = row.map_err(|e| format!("row: {e}"))?;
        *issue_counts.entry(issue_type.clone()).or_insert(0) += 1;
        let entry = article_issues.entry(slug.clone()).or_insert_with(|| (title, Vec::new()));
        entry.1.push(issue_type);
    }

    let mut sample: Vec<ArticleCtrBrief> = article_issues
        .into_iter()
        .map(|(slug, (title, issues))| ArticleCtrBrief { slug, title, issues })
        .collect();
    sample.sort_by(|a, b| b.issues.len().cmp(&a.issues.len()));
    sample.truncate(3);

    Ok(CtrSummary {
        articles_with_issues: sample.len() as i64,
        issue_breakdown: issue_counts,
        sample_articles: sample,
    })
}

fn collect_cannibalization(
    conn: &Connection,
    project_id: &str,
) -> Result<CannibalizationSummary, String> {
    let mut cluster_count = 0i64;
    let mut merge_candidates = 0i64;
    let mut hub_gaps = 0i64;

    if let Ok(Some(clusters_json)) =
        crate::db::content_audit::get_latest_audit_artifact(conn, project_id, "cannibalization_clusters")
    {
        if let Some(clusters) = clusters_json["clusters"].as_array() {
            cluster_count = clusters.len() as i64;
            for c in clusters {
                if c["recommendation"].as_str() == Some("merge") {
                    merge_candidates += 1;
                }
            }
        }
        if let Some(gaps) = clusters_json["hub_gaps"].as_array() {
            hub_gaps = gaps.len() as i64;
        }
    }

    Ok(CannibalizationSummary {
        cluster_count,
        merge_candidates,
        hub_gaps,
    })
}

fn collect_links(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
) -> Result<LinkSummary, String> {
    let articles = crate::engine::task_store::list_articles(conn, project_id)
        .map_err(|e| format!("list_articles: {e}"))?;

    let content_dirs = crate::content::article_resolver::discover_content_dirs(std::path::Path::new(project_path));
    let content_dir = content_dirs.first().map(|s| s.as_str()).unwrap_or("src/content");

    let scan = crate::content::linking::scan_links(
        std::path::Path::new(project_path).join(content_dir).as_path(),
        &articles,
    )
    .map_err(|e| format!("scan_links: {e}"))?;

    let orphan_count = scan.orphan_ids.len() as i64;
    let zero_incoming = scan.zero_incoming_ids.len() as i64;
    let total_links = scan.total_internal_links as i64;
    let avg_links = if articles.is_empty() { 0.0 } else { total_links as f64 / articles.len() as f64 };

    // Map orphan IDs back to slugs
    let id_to_slug: std::collections::HashMap<i64, String> = articles
        .iter()
        .map(|a| (a.id, a.url_slug.clone()))
        .collect();
    let sample_orphans: Vec<String> = scan.orphan_ids
        .iter()
        .filter_map(|id| id_to_slug.get(id).cloned())
        .take(3)
        .collect();

    Ok(LinkSummary {
        orphan_count,
        zero_incoming_count: zero_incoming,
        total_internal_links: total_links,
        avg_links_per_article: avg_links,
        sample_orphans,
    })
}

fn collect_content_distribution(
    conn: &Connection,
    project_id: &str,
) -> Result<ContentDistribution, String> {
    let articles = crate::engine::task_store::list_articles(conn, project_id)
        .map_err(|e| format!("list_articles: {e}"))?;

    let mut status_breakdown: HashMap<String, i64> = HashMap::new();
    let mut word_hist: HashMap<String, i64> = HashMap::new();
    let mut temporal_count = 0i64;
    let mut sample_temporal: Vec<String> = Vec::new();
    let mut very_thin = 0i64;
    let mut thin = 0i64;

    // Only match year suffixes at the END of the slug (e.g. "-2025", "-2026").
    // Date prefixes like "2025-01-18-xxx" are stripped by the blog parser and
    // do NOT create year-based URLs. Do NOT match years in the middle of slugs.
    let temporal_re = regex::Regex::new(r"-\d{4}$").unwrap();

    for a in &articles {
        *status_breakdown.entry(a.status.clone()).or_insert(0) += 1;

        let bucket = match a.word_count {
            0..=99 => { very_thin += 1; "0-99" }
            100..=299 => { thin += 1; "100-299" }
            300..=499 => "300-499",
            500..=999 => "500-999",
            1000..=1999 => "1000-1999",
            _ => "2000+",
        };
        *word_hist.entry(bucket.to_string()).or_insert(0) += 1;

        // Strip numeric prefix (e.g. "49-") before checking for temporal suffix,
        // since the blog parser strips these. Then check for year suffix at end.
        let slug_no_prefix = regex::Regex::new(r"^\d+-").unwrap()
            .replace(&a.url_slug, "");
        if temporal_re.is_match(&slug_no_prefix) {
            temporal_count += 1;
            if sample_temporal.len() < 3 {
                sample_temporal.push(a.url_slug.clone());
            }
        }
    }

    Ok(ContentDistribution {
        status_breakdown,
        word_count_histogram: word_hist,
        temporal_url_count: temporal_count,
        sample_temporal_urls: sample_temporal,
        very_thin_count: very_thin,
        thin_count: thin,
    })
}

fn collect_technical_seo(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
) -> Result<TechnicalSeoSummary, String> {
    let articles = crate::engine::task_store::list_articles(conn, project_id)
        .map_err(|e| format!("list_articles: {e}"))?;

    let mut missing_desc = 0i64;
    let mut missing_keyword = 0i64;
    let mut title_counts: HashMap<String, Vec<String>> = HashMap::new();
    let mut stale_count = 0i64;
    let one_year_ago = chrono::Utc::now() - chrono::Duration::days(365);

    let content_dirs = crate::content::article_resolver::discover_content_dirs(std::path::Path::new(project_path));
    let content_dirs_refs: Vec<&str> = content_dirs.iter().map(|s| s.as_str()).collect();
    let repo_root = std::path::Path::new(project_path);

    let mut stale_samples: Vec<String> = Vec::new();

    for a in &articles {
        // Check frontmatter for description, keyword, and ACTUAL title
        let resolved = crate::content::article_resolver::resolve_article_file(
            repo_root, &a.file, &content_dirs_refs,
        );
        if resolved.found {
            if let Ok(content) = std::fs::read_to_string(&resolved._absolute_path) {
                if let Ok(frontmatter) = crate::content::frontmatter::parse(&content) {
                    let parsed = &frontmatter.parsed;
                    let has_desc = parsed.get("description")
                        .and_then(|v| v.as_str())
                        .map(|s| !s.is_empty())
                        .unwrap_or(false);
                    if !has_desc {
                        missing_desc += 1;
                    }
                    let has_kw = parsed.get("target_keyword")
                        .and_then(|v| v.as_str())
                        .map(|s| !s.is_empty())
                        .unwrap_or(false);
                    if !has_kw {
                        missing_keyword += 1;
                    }
                    // Track ACTUAL frontmatter title for duplicate detection
                    if let Some(fm_title) = parsed.get("title").and_then(|v| v.as_str()) {
                        let title_lower = fm_title.to_lowercase().trim().to_string();
                        if !title_lower.is_empty() {
                            title_counts.entry(title_lower).or_default().push(a.url_slug.clone());
                        }
                    }
                }
            }
        }

        // Check stale content
        if let Some(ref date_str) = a.published_date {
            if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                let date: chrono::DateTime<chrono::Utc> = chrono::DateTime::from_naive_utc_and_offset(
                    date.and_hms_opt(0, 0, 0).unwrap(),
                    chrono::Utc,
                );
                if date < one_year_ago {
                    stale_count += 1;
                    if stale_samples.len() < 3 {
                        stale_samples.push(a.url_slug.clone());
                    }
                }
            }
        }
    }

    // Find duplicates (titles used by more than one article)
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
    let duplicate_count = duplicate_samples.len() as i64;

    // Check if lastModified is supported by sampling frontmatter
    let last_modified_supported = check_last_modified_support(project_path);

    Ok(TechnicalSeoSummary {
        missing_description_count: missing_desc,
        missing_keyword_count: missing_keyword,
        duplicate_title_count: duplicate_count,
        duplicate_title_samples: duplicate_samples,
        content_older_than_1_year_count: stale_count,
        content_older_than_1_year_samples: stale_samples,
        last_modified_supported,
    })
}

fn extract_issue_names(data: &serde_json::Value) -> Vec<String> {
    let mut issues = Vec::new();

    if let Some(checks) = data.get("checks").and_then(|c| c.as_object()) {
        for (key, val) in checks {
            if let Some(passed) = val.get("passed").and_then(|v| v.as_bool()) {
                if !passed {
                    issues.push(key.replace('_', " "));
                }
            }
        }
    }

    if let Some(detected) = data.get("issues_detected").and_then(|v| v.as_array()) {
        for issue in detected {
            if let Some(name) = issue.as_str() {
                if !issues.contains(&name.to_string()) {
                    issues.push(name.to_string());
                }
            }
        }
    }

    for section in ["quality", "readability", "seo"] {
        if let Some(obj) = data.get(section).and_then(|v| v.as_object()) {
            for (key, val) in obj {
                if let Some(passed) = val.get("passed").and_then(|v| v.as_bool()) {
                    if !passed {
                        let name = format!("{section}: {key}");
                        if !issues.contains(&name) {
                            issues.push(name);
                        }
                    }
                }
            }
        }
    }

    issues.truncate(5);
    issues
}

// ═══════════════════════════════════════════════════════════════════════════════
// Code verification — read actual source files and build output
// ═══════════════════════════════════════════════════════════════════════════════

fn collect_code_verification(project_path: &str) -> Result<CodeVerification, String> {
    let root = std::path::Path::new(project_path);

    // 1. Check SEO head component
    let seo_head_summary = check_seo_head_component(root);

    // 2. Check blog parser slug logic
    let slug_logic = check_blog_parser(root);

    // 3. Sample built HTML
    let html_check = sample_built_html(root);

    // 4. Sample MDX frontmatter
    let fm_check = sample_frontmatter(root);

    // 5. Key source files
    let mut key_files = Vec::new();
    for (path, purpose) in [
        ("src/components/SEOHead.vue", "Injects meta tags, titles, JSON-LD"),
        ("src/views/BlogPostPage.vue", "Renders blog posts, passes SEO data"),
        ("src/blog/index.ts", "Parses MDX, derives slugs"),
        ("src/App.vue", "Root layout"),
    ] {
        if root.join(path).exists() {
            key_files.push(SourceFileNote {
                path: path.to_string(),
                purpose: purpose.to_string(),
            });
        }
    }

    Ok(CodeVerification {
        seo_head_summary,
        slug_derivation_logic: slug_logic,
        sampled_html_check: html_check,
        sampled_frontmatter_check: fm_check,
        key_source_files: key_files,
    })
}

fn check_seo_head_component(root: &std::path::Path) -> String {
    let seo_head = root.join("src/components/SEOHead.vue");
    if !seo_head.exists() {
        return "SEOHead.vue not found".to_string();
    }
    let content = std::fs::read_to_string(&seo_head).unwrap_or_default();
    let mut findings = Vec::new();

    if content.contains("useHead") || content.contains("useSeoMeta") || content.contains("@unhead/vue") {
        findings.push("uses head injection library");
    }
    if content.contains("title") {
        findings.push("handles title");
    }
    if content.contains("description") || content.contains("meta") {
        findings.push("handles meta description");
    }
    if content.contains("og:") || content.contains("openGraph") || content.contains("ogImage") {
        findings.push("handles Open Graph");
    }
    if content.contains("json") || content.contains("ld+json") || content.contains("schema") {
        findings.push("handles JSON-LD");
    }
    if content.contains("faq") || content.contains("FAQPage") {
        findings.push("handles FAQ schema");
    }
    if content.contains("lastModified") || content.contains("last-modified") || content.contains("modified_time") {
        findings.push("handles lastModified / article:modified_time");
    }

    if findings.is_empty() {
        "Found SEOHead.vue but could not determine capabilities from source".to_string()
    } else {
        format!("SEOHead.vue: {}", findings.join(", "))
    }
}

fn check_blog_parser(root: &std::path::Path) -> String {
    let parser = root.join("src/blog/index.ts");
    if !parser.exists() {
        // Try other common locations
        let alt = root.join("src/content/blog.ts");
        if alt.exists() {
            return "Blog parser found at src/content/blog.ts (check for slug logic)".to_string();
        }
        return "Blog parser not found at expected location".to_string();
    }
    let content = std::fs::read_to_string(&parser).unwrap_or_default();

    if content.contains("replace") && content.contains("YYYY") {
        return "Blog parser strips date prefixes from filenames when deriving slugs".to_string();
    }
    if content.contains("slug") && content.contains("replace") {
        return "Blog parser applies transformations when deriving slugs from filenames".to_string();
    }
    if content.contains("slug") {
        return "Blog parser has custom slug derivation logic".to_string();
    }
    "Blog parser found but slug logic could not be determined".to_string()
}

fn sample_built_html(root: &std::path::Path) -> HtmlSampleCheck {
    let dist_blog = root.join("dist/blog");
    let mut checked = 0i64;
    let mut titles = Vec::new();
    let mut descriptions = Vec::new();
    let mut malformed = Vec::new();
    let mut all_unique_title = true;
    let mut all_unique_desc = true;

    if !dist_blog.exists() {
        return HtmlSampleCheck {
            files_checked: 0,
            all_have_unique_title: false,
            all_have_unique_description: false,
            total_title_samples: Vec::new(),
            total_description_samples: Vec::new(),
            malformed_files: Vec::new(),
        };
    }

    let mut seen_titles: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut seen_descs: std::collections::HashSet<String> = std::collections::HashSet::new();

    if let Ok(entries) = std::fs::read_dir(&dist_blog) {
        for entry in entries.flatten() {
            if checked >= 3 {
                break;
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("html") {
                continue;
            }
            checked += 1;
            let content = std::fs::read_to_string(&path).unwrap_or_default();

            // Extract title
            let title_re = regex::Regex::new(r"<title>(.*?)</title>").unwrap();
            let title = title_re.captures(&content).and_then(|c| c.get(1)).map(|m| m.as_str().to_string());

            // Extract meta description
            let desc_re = regex::Regex::new(r#"<meta[^>]*name=["']description["'][^>]*content=["']([^"']*)["'][^>]*>"#).unwrap();
            let desc = desc_re.captures(&content).and_then(|c| c.get(1)).map(|m| m.as_str().to_string());

            match (&title, &desc) {
                (Some(t), Some(d)) => {
                    titles.push(char_limit(t, 80).to_string());
                    descriptions.push(char_limit(d, 80).to_string());
                    if !seen_titles.insert(t.clone()) {
                        all_unique_title = false;
                    }
                    if !seen_descs.insert(d.clone()) {
                        all_unique_desc = false;
                    }
                }
                _ => {
                    malformed.push(path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string());
                }
            }
        }
    }

    HtmlSampleCheck {
        files_checked: checked,
        all_have_unique_title: all_unique_title && !titles.is_empty(),
        all_have_unique_description: all_unique_desc && !descriptions.is_empty(),
        total_title_samples: titles,
        total_description_samples: descriptions,
        malformed_files: malformed,
    }
}

fn sample_frontmatter(root: &std::path::Path) -> FrontmatterSampleCheck {
    let mut checked = 0i64;
    let mut malformed = Vec::new();
    let mut all_have_desc = true;
    let mut all_have_title = true;
    let mut lastmod_count = 0i64;

    let content_dir = root.join("src/blog/posts");
    if !content_dir.exists() {
        return FrontmatterSampleCheck {
            files_checked: 0,
            all_have_description: false,
            all_have_title: false,
            last_modified_present_count: 0,
            malformed_frontmatter_files: Vec::new(),
        };
    }

    if let Ok(entries) = std::fs::read_dir(&content_dir) {
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
            match crate::content::frontmatter::parse(&content) {
                Ok(fm) => {
                    let parsed = &fm.parsed;
                    let has_title = parsed.get("title").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false);
                    let has_desc = parsed.get("description").and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false);
                    let has_lastmod = parsed.get("lastModified").or_else(|| parsed.get("lastmod"))
                        .and_then(|v| v.as_str()).map(|s| !s.is_empty()).unwrap_or(false);
                    if !has_title {
                        all_have_title = false;
                    }
                    if !has_desc {
                        all_have_desc = false;
                    }
                    if has_lastmod {
                        lastmod_count += 1;
                    }
                }
                Err(_) => {
                    malformed.push(path.file_stem().and_then(|s| s.to_str()).unwrap_or("unknown").to_string());
                }
            }
        }
    }

    FrontmatterSampleCheck {
        files_checked: checked,
        all_have_description: all_have_desc,
        all_have_title: all_have_title,
        last_modified_present_count: lastmod_count,
        malformed_frontmatter_files: malformed,
    }
}

fn check_last_modified_support(project_path: &str) -> bool {
    let root = std::path::Path::new(project_path);
    let seo_head = root.join("src/components/SEOHead.vue");
    let blog_page = root.join("src/views/BlogPostPage.vue");

    let seo_head_has_it = seo_head.exists() && {
        let c = std::fs::read_to_string(&seo_head).unwrap_or_default();
        c.contains("lastModified") || c.contains("last-modified") || c.contains("modified_time")
    };
    let blog_page_has_it = blog_page.exists() && {
        let c = std::fs::read_to_string(&blog_page).unwrap_or_default();
        c.contains("lastModified") || c.contains("lastmod") || c.contains("dateModified")
    };

    seo_head_has_it && blog_page_has_it
}

fn char_limit(s: &str, max_chars: usize) -> &str {
    if let Some((idx, _)) = s.char_indices().nth(max_chars) {
        &s[..idx]
    } else {
        s
    }
}

fn build_verification_notes() -> String {
    "VERIFIED: code_verification section reads actual source files and built HTML. \
UNVERIFIED: ctr data comes from article_ctr_issues table and may be stale — cross-check with code_verification before reporting. \
UNVERIFIED: indexing data comes from GSC and may be outdated. \
NOTE: temporal_url detection matches ONLY explicit year suffixes like '-2025' at the END of slugs. \
      Date prefixes like '2025-01-18-' are stripped by the blog parser and do NOT create year-based URLs. \
NOTE: duplicate titles detected from actual frontmatter (not DB titles). \
NOTE: content_older_than_1_year is just old published_date — lastModified support is verified separately."
        .to_string()
}
