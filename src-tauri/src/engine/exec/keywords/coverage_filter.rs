use crate::models::live_site::LiveSitePage;
use std::collections::HashSet;

/// Coverage cluster data loaded from keyword_coverage.json
#[derive(Debug, Clone)]
pub(crate) struct CoverageCluster {
    id: String,
    name: String,
    primary_keywords: Vec<String>,
    article_count: i64,
}

/// Load coverage clusters from keyword_coverage.json if available
pub(crate) fn load_coverage_clusters(project_path: &str) -> Vec<CoverageCluster> {
    let coverage = match crate::engine::exec::coverage::read_keyword_coverage(project_path) {
        Some(c) => c,
        None => return Vec::new(),
    };

    coverage
        .get("clusters")
        .and_then(|c| c.as_array())
        .map(|clusters| {
            clusters
                .iter()
                .filter_map(|c| {
                    let id = c.get("cluster_id")?.as_str()?.to_string();
                    let name = c.get("cluster_name")?.as_str()?.to_string();
                    let article_count = c.get("article_count")?.as_i64()?;
                    let primary_keywords = c
                        .get("primary_keywords")
                        .and_then(|k| k.as_array())
                        .map(|arr| {
                            arr.iter()
                                .filter_map(|k| k.as_str().map(|s| s.to_lowercase()))
                                .collect()
                        })
                        .unwrap_or_default();

                    Some(CoverageCluster {
                        id,
                        name,
                        primary_keywords,
                        article_count,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn normalize_keyword_candidate(value: &str) -> Option<String> {
    let lowered = value.trim().to_lowercase();
    if lowered.is_empty() {
        return None;
    }

    let normalized = lowered
        .chars()
        .map(|ch| if ch.is_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if normalized.len() < 3 {
        None
    } else {
        Some(normalized)
    }
}

pub(crate) fn collect_existing_keywords_from_live_site(pages: &[LiveSitePage]) -> HashSet<String> {
    let mut existing = HashSet::new();

    for page in pages {
        if let Some(title) = normalize_keyword_candidate(&page.title) {
            existing.insert(title);
        }
        if let Some(h1) = page.h1.as_deref().and_then(normalize_keyword_candidate) {
            existing.insert(h1);
        }
        if let Some(last_segment) = page
            .path
            .trim_matches('/')
            .split('/')
            .filter(|segment| !segment.is_empty())
            .next_back()
            .and_then(normalize_keyword_candidate)
        {
            existing.insert(last_segment);
        }
    }

    existing
}

const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "for", "to", "of", "in", "on", "and", "or", "is", "are", "how", "what",
    "best", "top",
];

fn word_set(s: &str) -> HashSet<&str> {
    s.split_whitespace()
        .filter(|w| !STOP_WORDS.contains(w))
        .collect()
}

fn fuzzy_word_match(a: &str, b: &str) -> bool {
    a == b || a.starts_with(b) || b.starts_with(a)
}

/// Score how well a keyword fills a coverage gap.
///
/// Returns (score, match_type, cluster_name):
/// - score: 0-100, higher = better gap fill
/// - match_type: "exact", "semantic", "new_topic"
/// - cluster_name: which cluster it relates to (if any)
///
/// Scoring logic:
/// - Keywords not matching any cluster: 100 (new topic, highest priority)
/// - Keywords matching a cluster with < 3 articles: 80 (thin cluster, needs content)
/// - Keywords matching a cluster with 3-5 articles: 50 (moderate coverage)
/// - Keywords matching a cluster with > 5 articles: 20 (well covered, low priority)
fn score_coverage_gap(
    keyword: &str,
    clusters: &[CoverageCluster],
    existing_keywords: &HashSet<String>,
) -> (u8, &'static str, Option<String>) {
    let kw_lower = keyword.to_lowercase();

    // Exact duplicate check
    if existing_keywords.contains(&kw_lower) {
        return (0, "exact_duplicate", None);
    }

    let kw_words: HashSet<&str> = word_set(&kw_lower);

    // Check for semantic match against cluster keywords using Jaccard word-overlap
    for cluster in clusters {
        let is_related = cluster.primary_keywords.iter().any(|pk| {
            let pk_words = word_set(pk);
            if pk_words.is_empty() || kw_words.is_empty() {
                return false;
            }
            // Count intersection using fuzzy word match (covers call/calls, trade/trading)
            let intersection = kw_words
                .iter()
                .filter(|kw_w| pk_words.iter().any(|pk_w| fuzzy_word_match(kw_w, pk_w)))
                .count();
            let union = kw_words.union(&pk_words).count();
            let jaccard = intersection as f64 / union as f64;
            jaccard >= 0.3
        });

        if is_related {
            let score = match cluster.article_count {
                0..=2 => 80,  // Thin cluster - high priority
                3..=5 => 50,  // Moderate coverage
                6..=10 => 30, // Good coverage
                _ => 20,      // Well covered - low priority
            };
            return (score, "semantic", Some(cluster.name.clone()));
        }
    }

    // No cluster match = new topic, highest priority
    (100, "new_topic", None)
}

/// Filter and sort candidates by coverage gap score.
///
/// Removes exact duplicates and low-value keywords, prioritizes gap-filling keywords.
pub(crate) fn filter_by_coverage_gap(
    candidates: Vec<super::Candidate>,
    clusters: &[CoverageCluster],
    existing_keywords: &HashSet<String>,
) -> Vec<super::Candidate> {
    let mut scored: Vec<(super::Candidate, u8, &'static str)> = candidates
        .into_iter()
        .filter_map(|c| {
            let (score, match_type, _) =
                score_coverage_gap(&c.keyword, clusters, existing_keywords);

            // Filter out exact duplicates entirely
            if score == 0 {
                return None;
            }

            Some((c, score, match_type))
        })
        .collect();

    // Sort by gap score desc, then by volume desc
    scored.sort_by(|a, b| {
        let score_cmp = b.1.cmp(&a.1); // Higher gap score first
        if score_cmp != std::cmp::Ordering::Equal {
            return score_cmp;
        }
        let vol_a = a.0.volume.unwrap_or(0);
        let vol_b = b.0.volume.unwrap_or(0);
        vol_b.cmp(&vol_a) // Higher volume first
    });

    // Log the distribution
    let new_topic_count = scored.iter().filter(|(_, _, t)| *t == "new_topic").count();
    let semantic_count = scored.iter().filter(|(_, _, t)| *t == "semantic").count();
    log::info!(
        "[coverage_filter] {} new topics, {} semantic matches after gap filtering",
        new_topic_count,
        semantic_count
    );

    scored.into_iter().map(|(c, _, _)| c).collect()
}
