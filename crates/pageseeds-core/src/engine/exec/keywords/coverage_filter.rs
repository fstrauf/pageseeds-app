use crate::models::live_site::LiveSitePage;
use crate::strategy::{match_cluster, ClusterStatus, ProjectStrategy};
use std::collections::HashSet;

/// Gap-score ceiling for candidates whose strategy classification is LEGACY
/// or MAINTAIN (issue #255). LEGACY / do_not_expand candidates are already
/// hard-dropped in final selection; this cap is defense-in-depth for the
/// scoring signal and for MAINTAIN clusters (which are NOT hard-dropped), so
/// off-strategy topics never outrank genuine ACTIVE-cluster gaps on the gap
/// score. Classification goes through the canonical [`match_cluster`] so the
/// cap, the shortlist annotation, and the rank hints never disagree.
pub(crate) const LEGACY_MAINTAIN_GAP_CAP: u8 = 50;

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
///
/// Strategy cap (issue #255): when the canonical strategy classification
/// ([`match_cluster`]) is LEGACY or MAINTAIN, the score is capped at
/// [`LEGACY_MAINTAIN_GAP_CAP`] regardless of the coverage-derived score —
/// otherwise the 100-point "new topic" bonus would reward drift off-cluster.
fn score_coverage_gap(
    keyword: &str,
    clusters: &[CoverageCluster],
    existing_keywords: &HashSet<String>,
    strategy: Option<&ProjectStrategy>,
) -> (u8, &'static str, Option<String>) {
    let (score, match_type, cluster_name) =
        score_coverage_gap_uncapped(keyword, clusters, existing_keywords);

    if let Some(s) = strategy {
        if matches!(
            match_cluster(s, keyword).map(|(_, status)| status),
            Some(ClusterStatus::Legacy | ClusterStatus::Maintain)
        ) {
            return (score.min(LEGACY_MAINTAIN_GAP_CAP), match_type, cluster_name);
        }
    }

    (score, match_type, cluster_name)
}

/// Raw coverage-gap score with no strategy input. Kept free of strategy
/// concerns so the cap above stays a visibly separate policy layer.
fn score_coverage_gap_uncapped(
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
/// The score is persisted on each candidate (`gap_score`) so downstream final
/// selection can use it as a sort tiebreak instead of the ordering being lost.
/// `strategy` caps LEGACY/MAINTAIN-cluster candidates at
/// [`LEGACY_MAINTAIN_GAP_CAP`]; `None` (or an empty strategy) is a no-op.
pub(crate) fn filter_by_coverage_gap(
    candidates: Vec<super::Candidate>,
    clusters: &[CoverageCluster],
    existing_keywords: &HashSet<String>,
    strategy: Option<&ProjectStrategy>,
) -> Vec<super::Candidate> {
    let mut scored: Vec<(super::Candidate, u8, &'static str)> = candidates
        .into_iter()
        .filter_map(|c| {
            let (score, match_type, _) =
                score_coverage_gap(&c.keyword, clusters, existing_keywords, strategy);

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

    scored
        .into_iter()
        .map(|(mut c, score, _)| {
            c.gap_score = Some(score as f64);
            c
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::{ClusterStatus, StrategyCluster};

    fn cluster(id: &str, name: &str, article_count: i64, keywords: &[&str]) -> CoverageCluster {
        CoverageCluster {
            id: id.to_string(),
            name: name.to_string(),
            primary_keywords: keywords.iter().map(|k| k.to_lowercase()).collect(),
            article_count,
        }
    }

    fn strategy() -> ProjectStrategy {
        ProjectStrategy {
            clusters: vec![
                StrategyCluster {
                    name: "Growth Topics".to_string(),
                    status: ClusterStatus::Active,
                    keywords: vec!["content ops".to_string()],
                },
                StrategyCluster {
                    name: "Alternatives".to_string(),
                    status: ClusterStatus::Maintain,
                    keywords: vec!["competitor alternatives".to_string()],
                },
                StrategyCluster {
                    name: "Old Services".to_string(),
                    status: ClusterStatus::Legacy,
                    keywords: vec!["web design packages".to_string()],
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn legacy_cluster_candidate_capped_even_as_new_topic() {
        // No coverage cluster match → would score 100 (new_topic); LEGACY caps at 50.
        let s = strategy();
        let (score, match_type, _) =
            score_coverage_gap("web design packages pricing", &[], &HashSet::new(), Some(&s));
        assert_eq!(match_type, "new_topic");
        assert_eq!(score, LEGACY_MAINTAIN_GAP_CAP);
    }

    #[test]
    fn maintain_cluster_candidate_capped_even_as_new_topic() {
        let s = strategy();
        let (score, _, _) = score_coverage_gap(
            "competitor alternatives guide",
            &[],
            &HashSet::new(),
            Some(&s),
        );
        assert_eq!(score, LEGACY_MAINTAIN_GAP_CAP);
    }

    #[test]
    fn cap_applies_to_semantic_scores_too() {
        // Thin coverage cluster (1 article) would score 80; LEGACY match caps at 50.
        let s = strategy();
        let clusters = vec![cluster("c1", "Web Design", 1, &["web design"])];
        let (score, match_type, name) = score_coverage_gap(
            "web design packages pricing",
            &clusters,
            &HashSet::new(),
            Some(&s),
        );
        assert_eq!(match_type, "semantic");
        assert_eq!(name.as_deref(), Some("Web Design"));
        assert_eq!(score, LEGACY_MAINTAIN_GAP_CAP);
    }

    #[test]
    fn active_cluster_and_unmatched_new_topic_unaffected() {
        let s = strategy();
        // ACTIVE-cluster candidate on a thin coverage cluster keeps 80.
        let clusters = vec![cluster("c2", "Content Ops", 1, &["content ops"])];
        let (score, _, _) =
            score_coverage_gap("content ops playbook", &clusters, &HashSet::new(), Some(&s));
        assert_eq!(score, 80);

        // Genuine off-strategy new topic keeps the full 100 bonus.
        let (score, match_type, _) =
            score_coverage_gap("brand new direction", &[], &HashSet::new(), Some(&s));
        assert_eq!(match_type, "new_topic");
        assert_eq!(score, 100);
    }

    #[test]
    fn none_strategy_preserves_legacy_behavior() {
        let (score, match_type, _) =
            score_coverage_gap("web design packages pricing", &[], &HashSet::new(), None);
        assert_eq!(match_type, "new_topic");
        assert_eq!(score, 100);
    }

    #[test]
    fn filter_by_coverage_gap_threads_strategy_cap() {
        let s = strategy();
        let candidates = vec![
            super::super::Candidate {
                keyword: "web design packages pricing".to_string(),
                source_theme: "t".to_string(),
                is_question: false,
                volume: Some(1000),
                kd: Some(10.0),
                intent: None,
                cpc: None,
                gap_score: None,
            },
            super::super::Candidate {
                keyword: "brand new direction".to_string(),
                source_theme: "t".to_string(),
                is_question: false,
                volume: Some(100),
                kd: Some(10.0),
                intent: None,
                cpc: None,
                gap_score: None,
            },
        ];
        let filtered = filter_by_coverage_gap(candidates, &[], &HashSet::new(), Some(&s));
        assert_eq!(filtered.len(), 2);
        // New topic (100) sorts ahead of the capped legacy candidate (50)
        // despite its lower volume.
        assert_eq!(filtered[0].keyword, "brand new direction");
        assert_eq!(filtered[0].gap_score, Some(100.0));
        assert_eq!(filtered[1].keyword, "web design packages pricing");
        assert_eq!(filtered[1].gap_score, Some(50.0));
    }
}
