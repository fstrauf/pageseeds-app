/// Internal link scanning and gap detection across MDX content files.
///
/// Mirrors `packages/seo-content-cli/src/seo_content_mcp/clustering_linking.py`.
use std::collections::{HashMap, HashSet};
use std::path::Path;

use regex::Regex;
use serde::Serialize;

use crate::error::Result;
use crate::models::article::Article;

#[derive(Debug, Clone, Serialize)]
pub struct InternalLink {
    pub source_id: i64,
    pub source_file: String,
    /// Resolved target article ID, or -1 if unresolvable
    pub target_id: i64,
    pub target_file: String,
    pub anchor_text: String,
    pub line_number: usize,
    pub in_related_section: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArticleLinkProfile {
    pub id: i64,
    pub title: String,
    pub file: String,
    pub outgoing_ids: Vec<i64>,
    pub incoming_ids: Vec<i64>,
    pub unresolved_links: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct LinkScanResult {
    pub total_articles: usize,
    pub total_internal_links: usize,
    pub articles_with_outgoing: usize,
    pub articles_with_incoming: usize,
    /// Articles with zero incoming AND zero outgoing links (completely disconnected)
    pub orphan_ids: Vec<i64>,
    /// Articles with zero incoming links (may have outgoing links — Google cannot discover them)
    pub zero_incoming_ids: Vec<i64>,
    pub profiles: Vec<ArticleLinkProfile>,
}

/// Scan all MDX files in `content_dir` and build a link profile for each article.
pub fn scan_links(content_dir: &Path, articles: &[Article]) -> Result<LinkScanResult> {
    // Build lookup maps
    let file_to_id: HashMap<String, i64> = articles
        .iter()
        .map(|a| {
            let basename = Path::new(&a.file)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&a.file)
                .to_string();
            (basename, a.id)
        })
        .collect();

    let slug_to_id: HashMap<String, i64> = articles
        .iter()
        .map(|a| (a.url_slug.clone(), a.id))
        .collect();

    let id_to_article: HashMap<i64, &Article> = articles.iter().map(|a| (a.id, a)).collect();

    // Regex patterns for internal links
    let re_canonical = Regex::new(r"\[([^\]]+)\]\(/blog/([^/)]+)/?[^)]*\)").unwrap();
    let re_relative = Regex::new(r"\[([^\]]+)\]\(\./([^)]+\.mdx?)\)").unwrap();
    let re_related_heading = Regex::new(r"(?im)^#{1,4}\s+Related\s+Articles").unwrap();

    let mut all_links: Vec<InternalLink> = Vec::new();
    // Map from article id → set of (outgoing target ids)
    let mut outgoing: HashMap<i64, HashSet<i64>> = HashMap::new();
    let mut incoming: HashMap<i64, HashSet<i64>> = HashMap::new();
    let mut unresolved_map: HashMap<i64, Vec<String>> = HashMap::new();

    let files = crate::content::locator::collect_markdown_files(content_dir);

    for file_path in &files {
        let basename = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let Some(&source_id) = file_to_id.get(&basename) else {
            continue;
        };

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let mut in_related = false;

        for (lineno, line) in content.lines().enumerate() {
            let lineno = lineno + 1;

            // Track "Related Articles" section
            if re_related_heading.is_match(line) {
                in_related = true;
                continue;
            }
            // Reset on any other top-level heading
            if in_related && line.starts_with("## ") || (in_related && line.starts_with("# ")) {
                in_related = false;
            }

            // Match canonical /blog/slug links
            for cap in re_canonical.captures_iter(line) {
                let anchor = cap[1].to_string();
                let slug = cap[2].to_string();
                let (target_id, target_file) = if let Some(&tid) = slug_to_id.get(&slug) {
                    let tfile = id_to_article
                        .get(&tid)
                        .map(|a| {
                            Path::new(&a.file)
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(&a.file)
                                .to_string()
                        })
                        .unwrap_or_default();
                    (tid, tfile)
                } else {
                    unresolved_map
                        .entry(source_id)
                        .or_default()
                        .push(slug.clone());
                    (-1, slug)
                };

                if target_id > 0 {
                    outgoing.entry(source_id).or_default().insert(target_id);
                    incoming.entry(target_id).or_default().insert(source_id);
                }

                all_links.push(InternalLink {
                    source_id,
                    source_file: basename.clone(),
                    target_id,
                    target_file,
                    anchor_text: anchor,
                    line_number: lineno,
                    in_related_section: in_related,
                });
            }

            // Match relative ./filename.mdx links
            for cap in re_relative.captures_iter(line) {
                let anchor = cap[1].to_string();
                let target_file = cap[2].to_string();
                let (target_id, resolved_file) = if let Some(&tid) = file_to_id.get(&target_file) {
                    (tid, target_file.clone())
                } else {
                    unresolved_map
                        .entry(source_id)
                        .or_default()
                        .push(target_file.clone());
                    (-1, target_file)
                };

                if target_id > 0 {
                    outgoing.entry(source_id).or_default().insert(target_id);
                    incoming.entry(target_id).or_default().insert(source_id);
                }

                all_links.push(InternalLink {
                    source_id,
                    source_file: basename.clone(),
                    target_id,
                    target_file: resolved_file,
                    anchor_text: anchor,
                    line_number: lineno,
                    in_related_section: in_related,
                });
            }
        }
    }

    // Build profiles
    let profiles: Vec<ArticleLinkProfile> = articles
        .iter()
        .map(|a| {
            let out: Vec<i64> = outgoing
                .get(&a.id)
                .map(|s| {
                    let mut v: Vec<i64> = s.iter().copied().collect();
                    v.sort();
                    v
                })
                .unwrap_or_default();
            let inc: Vec<i64> = incoming
                .get(&a.id)
                .map(|s| {
                    let mut v: Vec<i64> = s.iter().copied().collect();
                    v.sort();
                    v
                })
                .unwrap_or_default();
            let unresolved = unresolved_map.get(&a.id).cloned().unwrap_or_default();

            ArticleLinkProfile {
                id: a.id,
                title: a.title.clone(),
                file: Path::new(&a.file)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&a.file)
                    .to_string(),
                outgoing_ids: out,
                incoming_ids: inc,
                unresolved_links: unresolved,
            }
        })
        .collect();

    let articles_with_outgoing = profiles
        .iter()
        .filter(|p| !p.outgoing_ids.is_empty())
        .count();
    let articles_with_incoming = profiles
        .iter()
        .filter(|p| !p.incoming_ids.is_empty())
        .count();
    let orphan_ids: Vec<i64> = profiles
        .iter()
        .filter(|p| p.incoming_ids.is_empty() && p.outgoing_ids.is_empty())
        .map(|p| p.id)
        .collect();
    let zero_incoming_ids: Vec<i64> = profiles
        .iter()
        .filter(|p| p.incoming_ids.is_empty())
        .map(|p| p.id)
        .collect();

    Ok(LinkScanResult {
        total_articles: articles.len(),
        total_internal_links: all_links.len(),
        articles_with_outgoing,
        articles_with_incoming,
        orphan_ids,
        zero_incoming_ids,
        profiles,
    })
}
