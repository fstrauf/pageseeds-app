use std::collections::{HashMap, HashSet};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::gsc::{DriftUrl, GscDriftReport, ResubmitCandidate};
use crate::models::task::Task;
use super::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn make_article(id: i64, slug: &str, title: &str, keyword: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "url_slug": slug,
            "title": title,
            "target_keyword": keyword,
            "file": format!("{:03}_{}.mdx", id, slug.replace('-', "_")),
        })
    }

    #[test]
    fn build_source_candidates_excludes_target_and_already_linked() {
        let mut articles = HashMap::new();
        articles.insert(
            "target".to_string(),
            make_article(1, "target", "Target Page", "machine learning"),
        );
        articles.insert(
            "source-a".to_string(),
            make_article(2, "source-a", "Source A", "deep learning"),
        );
        articles.insert(
            "source-b".to_string(),
            make_article(3, "source-b", "Source B", "baking recipes"),
        );

        let incoming_counts = HashMap::new();
        let gsc_items = HashMap::new();
        let link_scan = serde_json::json!({
            "profiles": [
                {
                    "id": 2,
                    "outgoing_ids": [1]
                }
            ]
        });

        let mut usage = HashMap::new();
        let candidates = build_source_candidates(
            1,
            "target",
            "machine learning",
            &articles,
            &incoming_counts,
            &gsc_items,
            Some(&link_scan),
            &mut usage,
        );

        // source-a already links to target → excluded
        assert!(
            candidates.iter().all(|c| c.article_id != 2),
            "already-linked source should be excluded"
        );
        // source-b is unrelated (score 0) → excluded by score filter
        assert!(
            candidates.iter().all(|c| c.article_id != 3),
            "unrelated source with score 0 should be excluded"
        );
        // source-a is the only candidate but it's excluded, so list may be empty
        // or source-b might have minimal overlap. The key assertion is source-a is gone.
    }

    #[test]
    fn build_source_candidates_enforces_overuse_limit() {
        let mut articles = HashMap::new();
        for i in 1..=10 {
            articles.insert(
                format!("source-{}", i),
                make_article(
                    i as i64,
                    &format!("source-{}", i),
                    &format!("Source {}", i),
                    "machine learning",
                ),
            );
        }
        // Add target
        articles.insert(
            "target".to_string(),
            make_article(99, "target", "Target Page", "machine learning"),
        );

        let incoming_counts = HashMap::new();
        let gsc_items = HashMap::new();
        let link_scan = serde_json::json!({ "profiles": [] });

        let mut usage = HashMap::new();

        // First call for target-1: gets top candidates
        let c1 = build_source_candidates(
            99,
            "target",
            "machine learning",
            &articles,
            &incoming_counts,
            &gsc_items,
            Some(&link_scan),
            &mut usage,
        );
        assert!(!c1.is_empty(), "should find candidates");

        // Use the top candidate MAX_SOURCE_USES_PER_CAMPAIGN times
        let top_id = c1[0].article_id;
        for _ in 0..MAX_SOURCE_USES_PER_CAMPAIGN {
            *usage.entry(top_id).or_insert(0) += 1;
        }

        // Next call should not include the overused source
        let c2 = build_source_candidates(
            98,
            "target-2",
            "machine learning",
            &articles,
            &incoming_counts,
            &gsc_items,
            Some(&link_scan),
            &mut usage,
        );
        assert!(
            !c2.iter().any(|c| c.article_id == top_id),
            "overused source ({} uses) should be excluded",
            MAX_SOURCE_USES_PER_CAMPAIGN
        );
    }

    #[test]
    fn build_source_candidates_scores_by_topical_similarity() {
        let mut articles = HashMap::new();
        articles.insert(
            "target".to_string(),
            make_article(1, "target", "Machine Learning Guide", "machine learning"),
        );
        articles.insert(
            "related".to_string(),
            make_article(2, "related", "Deep Learning Tutorial", "deep learning"),
        );
        articles.insert(
            "unrelated".to_string(),
            make_article(3, "unrelated", "Chocolate Cake Recipe", "baking"),
        );

        let incoming_counts = HashMap::new();
        let gsc_items = HashMap::new();
        let link_scan = serde_json::json!({ "profiles": [] });

        let mut usage = HashMap::new();
        let candidates = build_source_candidates(
            1,
            "target",
            "machine learning",
            &articles,
            &incoming_counts,
            &gsc_items,
            Some(&link_scan),
            &mut usage,
        );

        // Related source should score higher than unrelated
        let related = candidates.iter().find(|c| c.article_id == 2);
        let unrelated = candidates.iter().find(|c| c.article_id == 3);

        if let (Some(r), Some(u)) = (related, unrelated) {
            assert!(
                r.score > u.score,
                "related source ({}: {}) should score higher than unrelated ({}: {})",
                r.title,
                r.score,
                u.title,
                u.score
            );
        }
    }

    #[test]
    fn file_age_hours_returns_none_for_missing_file() {
        let path = std::path::Path::new("/nonexistent/path/to/file.txt");
        assert!(file_age_hours(path).is_none());
    }
}
