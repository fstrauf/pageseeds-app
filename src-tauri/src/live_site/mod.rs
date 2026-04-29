use crate::error::{Error, Result};
use crate::models::gsc::PageMetrics;
use crate::models::live_site::{
    LiveSiteAuditPage, LiveSiteAuditReport, LiveSiteAuditSummary, LiveSiteGscSyncResult,
    LiveSiteImportResult, LiveSiteLinkProfile, LiveSiteLinkScanResult, LiveSitePage,
};
use crate::models::project::Project;
use reqwest::header::CONTENT_TYPE;
use rusqlite::Connection;
use scraper::{Html, Selector};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use url::Url;

const DEFAULT_IMPORT_LIMIT: usize = 50;
const THIN_CONTENT_WORDS_THRESHOLD: i64 = 500;
const MIN_HEADING_COUNT: i64 = 2;
const MIN_OUTGOING_LINKS: i64 = 2;
const STALE_CRAWL_DAYS: i64 = 30;

#[derive(Debug, Clone, Serialize)]
struct ImportedPage {
    url: String,
    path: String,
    title: String,
    meta_description: Option<String>,
    h1: Option<String>,
    content_excerpt: Option<String>,
    word_count: i64,
    heading_count: i64,
    internal_links_out: i64,
    status_code: Option<i64>,
    last_crawled_at: String,
}

#[derive(Debug, Clone, Serialize)]
struct ImportedLink {
    source_url: String,
    target_url: String,
    anchor_text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportedSiteInventory {
    pages: Vec<ImportedPage>,
    links: Vec<ImportedLink>,
    sitemap_url: String,
    discovered_urls: usize,
    pages_failed: usize,
}

pub async fn import_project_site(
    project: &Project,
    limit: Option<usize>,
) -> Result<ImportedSiteInventory> {
    let site_url = project
        .site_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Other("Project has no site_url configured".to_string()))?;

    let base_url = normalize_site_url(site_url)?;
    let sitemap_url = project
        .sitemap_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_string())
        .unwrap_or_else(|| derive_sitemap_url(&base_url));
    let max_urls = limit.unwrap_or(DEFAULT_IMPORT_LIMIT).max(1);
    let sitemap_urls = crate::gsc::sitemap::fetch_sitemap_urls(&sitemap_url, max_urls).await?;

    if sitemap_urls.is_empty() {
        return Err(Error::Other(format!(
            "Sitemap at '{}' returned no URLs",
            sitemap_url
        )));
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (compatible; PageSeeds/1.0)")
        .build()
        .map_err(Error::Http)?;

    let mut pages = Vec::new();
    let mut links = Vec::new();
    let mut pages_failed = 0usize;
    let mut imported_urls = HashSet::new();
    let mut canonical_urls = HashSet::new();

    for raw_url in sitemap_urls.iter() {
        let Some(page_url) = normalize_same_site_url(&base_url, raw_url) else {
            continue;
        };

        let normalized_url = normalize_url_string(&page_url);
        if !imported_urls.insert(normalized_url.clone()) {
            continue;
        }

        match fetch_page_snapshot(&client, &base_url, &page_url).await {
            Ok((page, page_links)) => {
                if !canonical_urls.insert(page.url.clone()) {
                    log::warn!(
                        "[live_site] Skipping duplicate canonical URL from sitemap: {}",
                        page.url
                    );
                    continue;
                }
                pages.push(page);
                links.extend(page_links);
            }
            Err(err) => {
                pages_failed += 1;
                log::warn!("[live_site] Failed to import {}: {}", normalized_url, err);
            }
        }
    }

    if pages.is_empty() {
        return Err(Error::Other(format!(
            "No pages could be imported from sitemap '{}'",
            sitemap_url
        )));
    }

    Ok(ImportedSiteInventory {
        pages,
        links,
        sitemap_url,
        discovered_urls: sitemap_urls.len(),
        pages_failed,
    })
}

pub fn store_imported_site_inventory(
    conn: &Connection,
    project_id: &str,
    inventory: ImportedSiteInventory,
) -> Result<LiveSiteImportResult> {
    let tx = conn.unchecked_transaction()?;

    tx.execute(
        "DELETE FROM live_site_links WHERE project_id = ?1",
        [project_id],
    )?;
    tx.execute(
        "DELETE FROM live_site_pages WHERE project_id = ?1",
        [project_id],
    )?;

    let mut seen_urls = HashSet::new();
    for page in &inventory.pages {
        if !seen_urls.insert(page.url.clone()) {
            log::warn!(
                "[live_site] Skipping duplicate page URL during store: {}",
                page.url
            );
            continue;
        }
        tx.execute(
            "INSERT INTO live_site_pages (
                project_id, url, path, title, meta_description, h1, content_excerpt,
                word_count, heading_count, internal_links_out, status_code, last_crawled_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                project_id,
                page.url,
                page.path,
                page.title,
                page.meta_description,
                page.h1,
                page.content_excerpt,
                page.word_count,
                page.heading_count,
                page.internal_links_out,
                page.status_code,
                page.last_crawled_at,
            ],
        )?;
    }

    let now = chrono::Utc::now().to_rfc3339();
    for link in &inventory.links {
        tx.execute(
            "INSERT OR IGNORE INTO live_site_links (project_id, source_url, target_url, anchor_text, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![project_id, link.source_url, link.target_url, link.anchor_text, now],
        )?;
    }

    tx.commit()?;

    Ok(LiveSiteImportResult {
        sitemap_url: inventory.sitemap_url,
        discovered_urls: inventory.discovered_urls,
        pages_imported: inventory.pages.len(),
        links_imported: inventory.links.len(),
        pages_failed: inventory.pages_failed,
    })
}

pub fn list_live_site_pages(conn: &Connection, project_id: &str) -> Result<Vec<LiveSitePage>> {
    let mut stmt = conn.prepare(
        "SELECT url, path, title, meta_description, h1, content_excerpt,
                word_count, heading_count, internal_links_out, status_code,
                gsc_clicks, gsc_impressions, gsc_ctr, gsc_position, gsc_synced_at,
                last_crawled_at
         FROM live_site_pages
         WHERE project_id = ?1
         ORDER BY path ASC, url ASC",
    )?;

    let pages = stmt
        .query_map([project_id], |row| {
            Ok(LiveSitePage {
                url: row.get(0)?,
                path: row.get(1)?,
                title: row.get(2)?,
                meta_description: row.get(3)?,
                h1: row.get(4)?,
                content_excerpt: row.get(5)?,
                word_count: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                heading_count: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                internal_links_out: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
                status_code: row.get(9)?,
                gsc_clicks: row.get(10)?,
                gsc_impressions: row.get(11)?,
                gsc_ctr: row.get(12)?,
                gsc_position: row.get(13)?,
                gsc_synced_at: row.get(14)?,
                last_crawled_at: row.get(15)?,
            })
        })?
        .filter_map(|row| row.ok())
        .collect();

    Ok(pages)
}

pub fn get_live_site_audit(conn: &Connection, project_id: &str) -> Result<LiveSiteAuditReport> {
    let pages = list_live_site_pages(conn, project_id)?;
    let links = list_live_site_links(conn, project_id)?;
    let graph = build_link_graph(&pages, &links);

    let mut healthy_pages = 0usize;
    let mut thin_content_pages = 0usize;
    let mut missing_metadata_pages = 0usize;
    let mut weak_heading_pages = 0usize;
    let mut stale_crawl_pages = 0usize;
    let mut weak_interlinking_pages = 0usize;
    let mut orphan_pages = 0usize;

    let mut audit_pages: Vec<LiveSiteAuditPage> = pages
        .iter()
        .map(|page| {
            let incoming_links = graph
                .incoming
                .get(&page.url)
                .map(|entries| entries.len() as i64)
                .unwrap_or(0);
            let crawl_age_days = calculate_crawl_age_days(&page.last_crawled_at);

            let mut issue_flags = Vec::new();

            if page.word_count < THIN_CONTENT_WORDS_THRESHOLD {
                thin_content_pages += 1;
                issue_flags.push("thin_content".to_string());
            }

            let missing_meta_description = page
                .meta_description
                .as_deref()
                .map(str::trim)
                .map(|value| value.is_empty())
                .unwrap_or(true);
            let missing_h1 = page
                .h1
                .as_deref()
                .map(str::trim)
                .map(|value| value.is_empty())
                .unwrap_or(true);
            if missing_meta_description || missing_h1 {
                missing_metadata_pages += 1;
                if missing_meta_description {
                    issue_flags.push("missing_meta_description".to_string());
                }
                if missing_h1 {
                    issue_flags.push("missing_h1".to_string());
                }
            }

            if page.heading_count < MIN_HEADING_COUNT {
                weak_heading_pages += 1;
                issue_flags.push("weak_headings".to_string());
            }

            if crawl_age_days >= STALE_CRAWL_DAYS {
                stale_crawl_pages += 1;
                issue_flags.push("stale_crawl".to_string());
            }

            if page.internal_links_out < MIN_OUTGOING_LINKS || incoming_links == 0 {
                weak_interlinking_pages += 1;
                issue_flags.push("weak_interlinking".to_string());
            }

            if incoming_links == 0 {
                orphan_pages += 1;
                issue_flags.push("orphan_page".to_string());
            }

            if issue_flags.is_empty() {
                healthy_pages += 1;
            }

            LiveSiteAuditPage {
                url: page.url.clone(),
                path: page.path.clone(),
                title: page.title.clone(),
                word_count: page.word_count,
                heading_count: page.heading_count,
                internal_links_out: page.internal_links_out,
                internal_links_in: incoming_links,
                has_meta_description: !missing_meta_description,
                has_h1: !missing_h1,
                last_crawled_at: page.last_crawled_at.clone(),
                crawl_age_days,
                issue_count: issue_flags.len() as i64,
                issue_flags,
            }
        })
        .collect();

    audit_pages.sort_by(|left, right| {
        right
            .issue_count
            .cmp(&left.issue_count)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.url.cmp(&right.url))
    });

    let total_pages = audit_pages.len();
    Ok(LiveSiteAuditReport {
        summary: LiveSiteAuditSummary {
            total_pages,
            healthy_pages,
            pages_with_issues: total_pages.saturating_sub(healthy_pages),
            thin_content_pages,
            missing_metadata_pages,
            weak_heading_pages,
            stale_crawl_pages,
            weak_interlinking_pages,
            orphan_pages,
        },
        pages: audit_pages,
    })
}

pub fn apply_live_site_gsc_metrics(
    conn: &Connection,
    project: &Project,
    start_date: &str,
    end_date: &str,
    page_rows: Vec<PageMetrics>,
) -> Result<LiveSiteGscSyncResult> {
    let site_url = project
        .site_url
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| Error::Other("Project has no site_url configured".to_string()))?;
    let base_url = normalize_site_url(site_url)?;
    let pages = list_live_site_pages(conn, &project.id)?;

    if pages.is_empty() {
        return Err(Error::Other(
            "No live-site pages imported yet. Import the site before syncing GSC metrics."
                .to_string(),
        ));
    }

    let mut metrics_by_url: HashMap<String, PageMetrics> = HashMap::new();
    let mut metrics_by_path: HashMap<String, PageMetrics> = HashMap::new();
    let rows_fetched = page_rows.len();

    for row in page_rows {
        let Some(url) = normalize_same_site_url(&base_url, &row.page) else {
            continue;
        };
        let normalized_url = normalize_url_string(&url);
        metrics_by_path
            .entry(url.path().to_string())
            .or_insert_with(|| row.clone());
        metrics_by_url.entry(normalized_url).or_insert(row);
    }

    let synced_at = chrono::Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE live_site_pages
         SET gsc_clicks = NULL,
             gsc_impressions = NULL,
             gsc_ctr = NULL,
             gsc_position = NULL,
             gsc_synced_at = ?2
         WHERE project_id = ?1",
        rusqlite::params![project.id, synced_at],
    )?;

    let mut pages_synced = 0usize;
    let mut pages_unmatched = 0usize;

    for page in &pages {
        let metrics = metrics_by_url
            .get(&page.url)
            .or_else(|| metrics_by_path.get(&page.path));

        if let Some(metrics) = metrics {
            tx.execute(
                "UPDATE live_site_pages
                 SET gsc_clicks = ?1,
                     gsc_impressions = ?2,
                     gsc_ctr = ?3,
                     gsc_position = ?4,
                     gsc_synced_at = ?5
                 WHERE project_id = ?6 AND url = ?7",
                rusqlite::params![
                    metrics.clicks,
                    metrics.impressions,
                    metrics.ctr,
                    metrics.position,
                    synced_at,
                    project.id,
                    page.url,
                ],
            )?;
            pages_synced += 1;
        } else {
            pages_unmatched += 1;
        }
    }

    tx.commit()?;

    Ok(LiveSiteGscSyncResult {
        site_url: site_url.to_string(),
        start_date: start_date.to_string(),
        end_date: end_date.to_string(),
        rows_fetched,
        pages_synced,
        pages_unmatched,
        synced_at,
    })
}

pub fn scan_live_site_links(conn: &Connection, project_id: &str) -> Result<LiveSiteLinkScanResult> {
    let pages = list_live_site_pages(conn, project_id)?;
    let links = list_live_site_links(conn, project_id)?;
    let graph = build_link_graph(&pages, &links);

    let mut profiles: Vec<LiveSiteLinkProfile> = pages
        .iter()
        .map(|page| {
            let mut outgoing_urls: Vec<String> = graph
                .outgoing
                .get(&page.url)
                .map(|entries| entries.iter().cloned().collect())
                .unwrap_or_default();
            outgoing_urls.sort();

            let mut incoming_urls: Vec<String> = graph
                .incoming
                .get(&page.url)
                .map(|entries| entries.iter().cloned().collect())
                .unwrap_or_default();
            incoming_urls.sort();

            let mut unresolved_links = graph.unresolved.get(&page.url).cloned().unwrap_or_default();
            unresolved_links.sort();
            unresolved_links.dedup();

            LiveSiteLinkProfile {
                url: page.url.clone(),
                path: page.path.clone(),
                title: page.title.clone(),
                outgoing_urls,
                incoming_urls,
                unresolved_links,
            }
        })
        .collect();

    profiles.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then_with(|| left.url.cmp(&right.url))
    });

    let mut orphan_urls: Vec<String> = pages
        .iter()
        .filter(|page| !graph.incoming.contains_key(&page.url))
        .map(|page| page.url.clone())
        .collect();
    orphan_urls.sort();

    Ok(LiveSiteLinkScanResult {
        total_pages: pages.len(),
        total_internal_links: links.len(),
        pages_with_outgoing: profiles
            .iter()
            .filter(|profile| !profile.outgoing_urls.is_empty())
            .count(),
        pages_with_incoming: profiles
            .iter()
            .filter(|profile| !profile.incoming_urls.is_empty())
            .count(),
        orphan_urls,
        profiles,
    })
}

#[derive(Default)]
struct LiveSiteLinkGraph {
    outgoing: HashMap<String, HashSet<String>>,
    incoming: HashMap<String, HashSet<String>>,
    unresolved: HashMap<String, Vec<String>>,
}

fn list_live_site_links(conn: &Connection, project_id: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT source_url, target_url
         FROM live_site_links
         WHERE project_id = ?1",
    )?;

    let links = stmt
        .query_map([project_id], |row| Ok((row.get(0)?, row.get(1)?)))?
        .filter_map(|row| row.ok())
        .collect();

    Ok(links)
}

fn build_link_graph(pages: &[LiveSitePage], links: &[(String, String)]) -> LiveSiteLinkGraph {
    let known_urls: HashSet<&str> = pages.iter().map(|page| page.url.as_str()).collect();
    let mut graph = LiveSiteLinkGraph::default();

    for (source_url, target_url) in links {
        if known_urls.contains(target_url.as_str()) {
            graph
                .outgoing
                .entry(source_url.clone())
                .or_default()
                .insert(target_url.clone());
            graph
                .incoming
                .entry(target_url.clone())
                .or_default()
                .insert(source_url.clone());
        } else {
            graph
                .unresolved
                .entry(source_url.clone())
                .or_default()
                .push(target_url.clone());
        }
    }

    graph
}

fn calculate_crawl_age_days(last_crawled_at: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(last_crawled_at)
        .map(|timestamp| {
            let timestamp = timestamp.with_timezone(&chrono::Utc);
            (chrono::Utc::now() - timestamp).num_days().max(0)
        })
        .unwrap_or(0)
}

async fn fetch_page_snapshot(
    client: &reqwest::Client,
    base_url: &Url,
    page_url: &Url,
) -> Result<(ImportedPage, Vec<ImportedLink>)> {
    let response = client
        .get(page_url.clone())
        .send()
        .await
        .map_err(Error::Http)?;
    let status_code = i64::from(response.status().as_u16());
    if !response.status().is_success() {
        return Err(Error::Other(format!(
            "Request returned HTTP {}",
            response.status()
        )));
    }

    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    if !content_type.contains("text/html") {
        return Err(Error::Other(format!(
            "Unsupported content type '{}'",
            content_type
        )));
    }

    let html = response.text().await.map_err(Error::Http)?;
    let document = Html::parse_document(&html);

    let canonical_url = extract_canonical_url(&document, page_url)
        .and_then(|candidate| normalize_same_site_url(base_url, candidate.as_str()))
        .unwrap_or_else(|| page_url.clone());

    let normalized_url = normalize_url_string(&canonical_url);
    let path = canonical_url.path().to_string();
    let title = extract_title(&document).unwrap_or_else(|| path.clone());
    let h1 = extract_h1(&document);
    let meta_description = extract_meta_description(&document);
    let text_content = extract_main_content(&document);
    let word_count = crate::content::ops::count_words(&text_content) as i64;
    let heading_count = count_headings(&document) as i64;
    let excerpt = build_excerpt(&text_content);
    let outgoing_links = extract_internal_links(&document, &canonical_url, base_url);
    let internal_links_out = outgoing_links.len() as i64;
    let last_crawled_at = chrono::Utc::now().to_rfc3339();

    let page = ImportedPage {
        url: normalized_url.clone(),
        path,
        title,
        meta_description,
        h1,
        content_excerpt: excerpt,
        word_count,
        heading_count,
        internal_links_out,
        status_code: Some(status_code),
        last_crawled_at,
    };

    let links = outgoing_links
        .into_iter()
        .map(|(target_url, anchor_text)| ImportedLink {
            source_url: normalized_url.clone(),
            target_url,
            anchor_text,
        })
        .collect();

    Ok((page, links))
}

fn normalize_site_url(site_url: &str) -> Result<Url> {
    let mut url = Url::parse(site_url)
        .map_err(|e| Error::Other(format!("Invalid site URL '{}': {}", site_url, e)))?;
    url.set_fragment(None);
    url.set_query(None);
    if url.path().is_empty() {
        url.set_path("/");
    }
    Ok(url)
}

fn derive_sitemap_url(base_url: &Url) -> String {
    let mut sitemap_url = base_url.clone();
    sitemap_url.set_path("/sitemap.xml");
    sitemap_url.set_query(None);
    sitemap_url.set_fragment(None);
    sitemap_url.to_string()
}

fn normalize_same_site_url(base_url: &Url, candidate: &str) -> Option<Url> {
    let parsed = base_url.join(candidate).ok()?;
    if !same_host(base_url, &parsed) {
        return None;
    }
    Some(normalize_url(parsed))
}

fn normalize_url(mut url: Url) -> Url {
    url.set_fragment(None);
    url.set_query(None);

    let path = url.path().trim_end_matches('/').to_string();
    if path.is_empty() {
        url.set_path("/");
    } else {
        url.set_path(&path);
    }
    url
}

fn normalize_url_string(url: &Url) -> String {
    normalize_url(url.clone()).to_string()
}

fn same_host(left: &Url, right: &Url) -> bool {
    normalize_host(left.host_str()) == normalize_host(right.host_str())
}

fn normalize_host(host: Option<&str>) -> Option<String> {
    host.map(|value| value.trim_start_matches("www.").to_ascii_lowercase())
}

fn extract_title(document: &Html) -> Option<String> {
    let selector = Selector::parse("title").ok()?;
    let value = document
        .select(&selector)
        .next()?
        .text()
        .collect::<String>();
    clean_text(&value).trim().to_owned().into()
}

fn extract_h1(document: &Html) -> Option<String> {
    let selector = Selector::parse("h1").ok()?;
    let value = document
        .select(&selector)
        .next()?
        .text()
        .collect::<String>();
    let cleaned = clean_text(&value);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn extract_meta_description(document: &Html) -> Option<String> {
    let selector = Selector::parse("meta[name='description']").ok()?;
    let element = document.select(&selector).next()?;
    let value = element.value().attr("content")?.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn extract_canonical_url(document: &Html, fallback: &Url) -> Option<Url> {
    let selector = Selector::parse("link[rel='canonical']").ok()?;
    let href = document.select(&selector).next()?.value().attr("href")?;
    fallback.join(href).ok()
}

fn extract_main_content(document: &Html) -> String {
    let selectors = [
        "article",
        "main",
        "[role='main']",
        ".content",
        ".post-content",
        ".entry-content",
        "#content",
        "body",
    ];

    for selector_str in selectors {
        if let Ok(selector) = Selector::parse(selector_str) {
            if let Some(element) = document.select(&selector).next() {
                let text = element.text().collect::<Vec<_>>().join(" ");
                let cleaned = clean_text(&text);
                if !cleaned.is_empty() {
                    return cleaned;
                }
            }
        }
    }

    String::new()
}

fn extract_internal_links(
    document: &Html,
    page_url: &Url,
    base_url: &Url,
) -> Vec<(String, String)> {
    let Ok(selector) = Selector::parse("a[href]") else {
        return Vec::new();
    };

    let mut seen = HashSet::new();
    let mut links = Vec::new();

    for anchor in document.select(&selector) {
        let Some(href) = anchor.value().attr("href") else {
            continue;
        };
        let Some(target) = normalize_same_site_url(base_url, href) else {
            continue;
        };

        let normalized_target = normalize_url_string(&target);
        if normalized_target == normalize_url_string(page_url) {
            continue;
        }

        let anchor_text = clean_text(&anchor.text().collect::<Vec<_>>().join(" "));
        let dedupe_key = format!("{}|{}", normalized_target, anchor_text);
        if seen.insert(dedupe_key) {
            links.push((normalized_target, anchor_text));
        }
    }

    links
}

fn count_headings(document: &Html) -> usize {
    let Ok(selector) = Selector::parse("h1, h2, h3") else {
        return 0;
    };
    document.select(&selector).count()
}

fn build_excerpt(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return None;
    }

    let excerpt: String = trimmed.chars().take(240).collect();
    if trimmed.chars().count() > 240 {
        Some(format!("{}…", excerpt.trim_end()))
    } else {
        Some(excerpt)
    }
}

fn clean_text(content: &str) -> String {
    content
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_same_host_urls() {
        let base = normalize_site_url("https://www.example.com/blog/").unwrap();
        let url = normalize_same_site_url(&base, "https://example.com/post/?utm_source=test#top")
            .unwrap();
        assert_eq!(url.as_str(), "https://example.com/post");
    }

    #[test]
    fn derives_default_sitemap_url() {
        let base = normalize_site_url("https://example.com/blog").unwrap();
        assert_eq!(derive_sitemap_url(&base), "https://example.com/sitemap.xml");
    }
}
