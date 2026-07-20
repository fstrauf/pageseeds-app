/// Internal link scanning and gap detection across MDX content files.
///
/// Mirrors `packages/seo-content-cli/src/seo_content_mcp/clustering_linking.py`.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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

/// A single `/blog/` link that does not resolve to any article in the project.
#[derive(Debug, Clone, Serialize)]
pub struct UnresolvedLink {
    pub article_id: i64,
    pub file: String,
    /// The unresolvable target as written (slug, filename, or malformed href).
    pub target: String,
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
    /// Every `/blog/` link whose target does not resolve to a project article.
    pub unresolved_links: Vec<UnresolvedLink>,
    pub profiles: Vec<ArticleLinkProfile>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Shared link regexes (compiled once)
//
// These are the single source of truth for `/blog/` link detection — used by
// `scan_links`, `extract_blog_link_hrefs`, and `repair_blog_link_hrefs`.
// Do not copy these patterns into other modules.
// ═══════════════════════════════════════════════════════════════════════════════

/// Canonical `[anchor](/blog/slug)` links.
/// Groups: 1 = anchor text, 2 = full href as written, 3 = slug segment.
pub(crate) fn canonical_blog_link_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[([^\]]+)\]\((/blog/([^/)]+)/?[^)]*)\)").unwrap())
}

/// Malformed `[anchor] /blog/slug` links (missing parentheses around the URL).
/// Groups: 1 = anchor text, 2 = whitespace gap, 3 = full href as written.
pub(crate) fn malformed_blog_link_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\[([^\]]*)\]([ \t]*)(/blog/[^)\s]*)").unwrap())
}

/// Extract every internal `/blog/` link from markdown/MDX content.
///
/// Returns one `(anchor_text, raw_href, slug_as_written)` entry per distinct
/// href. Covers canonical `[text](/blog/slug)` links and malformed
/// `[text]/blog/slug` links (missing parentheses). `slug_as_written` is the
/// first path segment after `/blog/` exactly as written — NOT normalized —
/// so callers can decide between exact-match and normalized resolution.
pub fn extract_blog_link_hrefs(content: &str) -> Vec<(String, String, String)> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    for cap in canonical_blog_link_re().captures_iter(content) {
        let raw_href = cap[2].to_string();
        if seen.insert(raw_href.clone()) {
            out.push((cap[1].to_string(), raw_href, cap[3].to_string()));
        }
    }
    for cap in malformed_blog_link_re().captures_iter(content) {
        let raw_href = cap[3].to_string();
        if seen.insert(raw_href.clone()) {
            let slug = raw_href
                .trim_start_matches("/blog/")
                .split('/')
                .next()
                .unwrap_or("")
                .to_string();
            out.push((cap[1].to_string(), raw_href, slug));
        }
    }

    out
}

/// Rewrite `/blog/` link hrefs in `content` according to `repairs`
/// (raw href as written → replacement href).
///
/// Only the link target changes; anchor text and surrounding markup are
/// preserved. Covers canonical and malformed links, and rewrites every
/// occurrence of each repaired href.
pub fn repair_blog_link_hrefs(
    content: &str,
    repairs: &std::collections::HashMap<String, String>,
) -> String {
    if repairs.is_empty() {
        return content.to_string();
    }

    let repaired = canonical_blog_link_re().replace_all(content, |caps: &regex::Captures| {
        match repairs.get(&caps[2]) {
            Some(new_href) => format!("[{}]({})", &caps[1], new_href),
            None => caps[0].to_string(),
        }
    });

    malformed_blog_link_re()
        .replace_all(&repaired, |caps: &regex::Captures| {
            match repairs.get(&caps[3]) {
                Some(new_href) => format!("[{}]{}{}", &caps[1], &caps[2], new_href),
                None => caps[0].to_string(),
            }
        })
        .into_owned()
}

/// A `/blog/` link found in a content file whose normalized slug belongs to a
/// caller-supplied slug set.
#[derive(Debug, Clone)]
pub struct SlugLinkMatch {
    /// Full path to the file containing the link.
    pub file: PathBuf,
    /// Raw href exactly as written (e.g. `/blog/248_old_post`).
    pub raw_href: String,
    /// Normalized slug that matched the target set.
    pub normalized_slug: String,
}

/// Walk every markdown file in `content_dir` and return all `/blog/` links
/// whose normalized slug is in `slugs`.
///
/// Shared traversal for the consolidation steps (`merge_rewrite_inbound_links`
/// builds repairs from it, `merge_validate_output` builds issues from it).
/// Files are visited in [`crate::content::locator::collect_markdown_files`]
/// order and links in [`extract_blog_link_hrefs`] order; unreadable files are
/// skipped.
pub fn find_links_to_slugs(content_dir: &Path, slugs: &HashSet<String>) -> Vec<SlugLinkMatch> {
    let mut matches = Vec::new();
    for file in crate::content::locator::collect_markdown_files(content_dir) {
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (_anchor, raw_href, slug_written) in extract_blog_link_hrefs(&content) {
            let normalized_slug = crate::content::slug::normalize_url_slug(&slug_written);
            if slugs.contains(&normalized_slug) {
                matches.push(SlugLinkMatch {
                    file: file.clone(),
                    raw_href,
                    normalized_slug,
                });
            }
        }
    }
    matches
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
        .map(|a| (crate::content::slug::normalize_url_slug(&a.url_slug), a.id))
        .collect();

    let id_to_article: HashMap<i64, &Article> = articles.iter().map(|a| (a.id, a)).collect();

    // Regex patterns for internal links (canonical + malformed are shared —
    // see the module-level fns; do not re-define them here).
    let re_canonical = canonical_blog_link_re();
    let re_malformed = malformed_blog_link_re();
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
                let slug = crate::content::slug::normalize_url_slug(&cap[3]);
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

            // Detect malformed links like `[text]/blog/slug` (missing parentheses)
            for cap in re_malformed.captures_iter(line) {
                let href = cap[3].to_string();
                unresolved_map
                    .entry(source_id)
                    .or_default()
                    .push(format!("{} (malformed link — missing parentheses)", href));
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

    // Flatten the per-article unresolved map into a project-level list so
    // consumers (cluster_link_scan message, link_scan.json) can surface every
    // dead /blog/ link without walking all profiles.
    let mut unresolved_links: Vec<UnresolvedLink> = unresolved_map
        .iter()
        .flat_map(|(article_id, targets)| {
            let file = profiles
                .iter()
                .find(|p| p.id == *article_id)
                .map(|p| p.file.clone())
                .unwrap_or_default();
            targets.iter().map(move |target| UnresolvedLink {
                article_id: *article_id,
                file: file.clone(),
                target: target.clone(),
            })
        })
        .collect();
    unresolved_links.sort_by(|a, b| a.file.cmp(&b.file).then_with(|| a.target.cmp(&b.target)));

    Ok(LinkScanResult {
        total_articles: articles.len(),
        total_internal_links: all_links.len(),
        articles_with_outgoing,
        articles_with_incoming,
        orphan_ids,
        zero_incoming_ids,
        unresolved_links,
        profiles,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_blog_link_hrefs_finds_canonical_and_malformed() {
        let content = r#"# Post

See [the roast guide](/blog/248_roast_profile_management) and
[hub](/blog/hub-coffee/) plus an external [site](https://example.com/blog/x).
Broken: [stale link] /blog/old_legacy_post here.
"#;
        let links = extract_blog_link_hrefs(content);
        assert_eq!(links.len(), 3);

        let roast = links
            .iter()
            .find(|(_, href, _)| href == "/blog/248_roast_profile_management")
            .expect("filename-form link extracted");
        assert_eq!(roast.0, "the roast guide");
        assert_eq!(roast.2, "248_roast_profile_management");

        let hub = links
            .iter()
            .find(|(_, href, _)| href == "/blog/hub-coffee/")
            .expect("trailing-slash link extracted with raw href preserved");
        assert_eq!(hub.2, "hub-coffee");

        let malformed = links
            .iter()
            .find(|(_, href, _)| href == "/blog/old_legacy_post")
            .expect("malformed link extracted");
        assert_eq!(malformed.0, "stale link");
        assert_eq!(malformed.2, "old_legacy_post");
    }

    #[test]
    fn extract_blog_link_hrefs_dedupes_repeated_hrefs() {
        let content = "[a](/blog/one) and [b](/blog/one) and [c](/blog/two)";
        let links = extract_blog_link_hrefs(content);
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn repair_blog_link_hrefs_rewrites_target_and_keeps_anchor() {
        let content = "Intro [the roast guide](/blog/248_roast_profile_management) outro.\n\
                       Again [second mention](/blog/248_roast_profile_management).\n\
                       Malformed [stale] /blog/old_legacy_post end.\n\
                       Untouched [other](/blog/other-post).\n";
        let mut repairs = std::collections::HashMap::new();
        repairs.insert(
            "/blog/248_roast_profile_management".to_string(),
            "/blog/roast-profile-management".to_string(),
        );
        repairs.insert(
            "/blog/old_legacy_post".to_string(),
            "/blog/old-legacy-post".to_string(),
        );

        let repaired = repair_blog_link_hrefs(content, &repairs);
        assert!(repaired.contains("[the roast guide](/blog/roast-profile-management)"));
        assert!(repaired.contains("[second mention](/blog/roast-profile-management)"));
        assert!(repaired.contains("[stale] /blog/old-legacy-post"));
        assert!(repaired.contains("[other](/blog/other-post)"));
        assert!(!repaired.contains("248_roast_profile_management"));
        assert!(!repaired.contains("old_legacy_post"));
    }

    #[test]
    fn repair_blog_link_hrefs_empty_repairs_is_identity() {
        let content = "[a](/blog/one)";
        assert_eq!(
            repair_blog_link_hrefs(content, &std::collections::HashMap::new()),
            content
        );
    }

    #[test]
    fn find_links_to_slugs_matches_normalized_slugs_across_files() {
        let dir = std::env::temp_dir().join(format!("pageseeds-find-links-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("1_post.mdx"),
            "[underscore](/blog/248_old_post) and [plain](/blog/old-post) and \
             [slash](/blog/old-post/) and [other](/blog/unrelated-post)\n",
        )
        .unwrap();
        std::fs::write(dir.join("2_keeper.mdx"), "[hub](/blog/hub-coffee)\n").unwrap();

        let slugs: HashSet<String> = ["old-post".to_string()].into_iter().collect();
        let matches = find_links_to_slugs(&dir, &slugs);

        // underscore, plain, and trailing-slash forms all normalize to old-post.
        assert_eq!(matches.len(), 3);
        assert!(matches
            .iter()
            .all(|m| m.file.ends_with("1_post.mdx") && m.normalized_slug == "old-post"));
        assert!(matches
            .iter()
            .any(|m| m.raw_href == "/blog/248_old_post"));
        assert!(matches.iter().any(|m| m.raw_href == "/blog/old-post"));
        assert!(matches.iter().any(|m| m.raw_href == "/blog/old-post/"));

        // No overlap → no matches.
        let none = find_links_to_slugs(&dir, &["ghost".to_string()].into_iter().collect());
        assert!(none.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_links_reports_unresolved_links() {
        let dir = std::env::temp_dir().join(format!("pageseeds-linking-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("1_post.mdx"),
            "---\ntitle: Post\n---\n\n[ok](/blog/keeper) and [bad](/blog/ghost)\n",
        )
        .unwrap();

        let articles = vec![
            crate::models::article::Article {
                id: 1,
                title: "Post".to_string(),
                url_slug: "post".to_string(),
                file: "1_post.mdx".to_string(),
                target_keyword: None,
                keyword_difficulty: None,
                target_volume: 0,
                published_date: None,
                word_count: 0,
                status: "published".to_string(),
                review_status: None,
                review_started_at: None,
                last_reviewed_at: None,
                review_count: 0,
                content_gaps_addressed: vec![],
                estimated_traffic_monthly: None,
                project_id: "p1".to_string(),
                quality_score: None,
                quality_grade: None,
                quality_rated_at: None,
                publishing_ready: None,
                quality_breakdown: None,
                content_hash: None,
                last_edited_at: None,
                page_type: None,
            },
            crate::models::article::Article {
                id: 2,
                title: "Keeper".to_string(),
                url_slug: "keeper".to_string(),
                file: "2_keeper.mdx".to_string(),
                target_keyword: None,
                keyword_difficulty: None,
                target_volume: 0,
                published_date: None,
                word_count: 0,
                status: "published".to_string(),
                review_status: None,
                review_started_at: None,
                last_reviewed_at: None,
                review_count: 0,
                content_gaps_addressed: vec![],
                estimated_traffic_monthly: None,
                project_id: "p1".to_string(),
                quality_score: None,
                quality_grade: None,
                quality_rated_at: None,
                publishing_ready: None,
                quality_breakdown: None,
                content_hash: None,
                last_edited_at: None,
                page_type: None,
            },
        ];

        let result = scan_links(&dir, &articles).unwrap();
        assert_eq!(result.unresolved_links.len(), 1);
        assert_eq!(result.unresolved_links[0].file, "1_post.mdx");
        assert_eq!(result.unresolved_links[0].target, "ghost");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
