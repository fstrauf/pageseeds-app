use super::rank::*;

// ═══════════════════════════════════════════════════════════════════════════════
// Honest cannibalization (issue #123)
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn soft_mega_cluster_does_not_mark_cannibalized() {
    // A 147-page-style soft TF-IDF cluster is exploratory only — membership
    // alone must never set cannibalized=true.
    let soft_clusters = serde_json::json!({
        "clusters": [{
            "cluster_id": "mega",
            "hub_exists": false,
            "pages": [
                { "id": 1, "url": "/blog/page-a" },
                { "id": 2, "url": "/blog/page-b" },
                { "id": 3, "url": "/blog/page-c" },
                { "id": 4, "url": "/blog/page-d" },
            ]
        }]
    });
    // Soft clusters are not passed into is_honestly_cannibalized at all.
    let _ = soft_clusters;
    assert!(
        !is_honestly_cannibalized(1, "page-a", None, None),
        "no evidence artifacts → not cannibalized"
    );
    // Even if a caller mistakenly passed soft clusters as candidates, the
    // candidates shape expects "candidates" not "clusters" — still false.
    let wrong_shape = serde_json::json!({
        "clusters": [{
            "pages": [
                { "id": 1, "url": "/blog/page-a" },
                { "id": 2, "url": "/blog/page-b" },
            ]
        }]
    });
    assert!(
        !is_honestly_cannibalized(1, "page-a", Some(&wrong_shape), None),
        "soft cluster JSON must not set cannibalized via candidates path"
    );
}

#[test]
fn candidates_membership_marks_cannibalized() {
    let candidates = serde_json::json!({
        "candidates": [{
            "candidate_type": "merge_candidate",
            "pages": [
                { "id": 10, "url": "/blog/alpha-guide" },
                { "id": 11, "url": "/blog/alpha-intro" },
            ]
        }]
    });
    assert!(
        is_honestly_cannibalized(10, "alpha-guide", Some(&candidates), None),
        "article in candidates pages list must be cannibalized"
    );
    assert!(
        is_honestly_cannibalized(11, "alpha-intro", Some(&candidates), None),
        "sibling in same candidate must be cannibalized"
    );
    assert!(
        !is_honestly_cannibalized(99, "unrelated", Some(&candidates), None),
        "article not in any candidate must not be cannibalized"
    );
}

#[test]
fn exact_keyword_duplicate_marks_cannibalized() {
    let dupes = serde_json::json!({
        "duplicates": [{
            "keyword": "cash secured puts",
            "pages": [
                { "id": 1, "url_slug": "best-stocks-csp" },
                { "id": 2, "url_slug": "csp-strategy" },
            ]
        }]
    });
    assert!(
        is_honestly_cannibalized(1, "best-stocks-csp", None, Some(&dupes)),
        "exact keyword dupe group membership must mark cannibalized"
    );
    assert!(
        is_honestly_cannibalized(2, "csp-strategy", None, Some(&dupes)),
        "other page in same exact-kw group must mark cannibalized"
    );
    assert!(
        !is_honestly_cannibalized(3, "other-page", None, Some(&dupes)),
        "page outside exact-kw groups must not be cannibalized"
    );
}

#[test]
fn empty_keyword_dupe_group_does_not_mark_cannibalized() {
    let dupes = serde_json::json!({
        "duplicates": [{
            "keyword": "",
            "pages": [
                { "id": 1, "url_slug": "a" },
                { "id": 2, "url_slug": "b" },
            ]
        }]
    });
    assert!(
        !is_honestly_cannibalized(1, "a", None, Some(&dupes)),
        "empty keyword groups are not evidence"
    );
}

#[test]
fn single_page_candidate_does_not_mark_cannibalized() {
    let candidates = serde_json::json!({
        "candidates": [{
            "candidate_type": "merge_candidate",
            "pages": [
                { "id": 1, "url": "/blog/solo" },
            ]
        }]
    });
    assert!(
        !is_honestly_cannibalized(1, "solo", Some(&candidates), None),
        "candidate with <2 pages is not cannibalization evidence"
    );
}

#[test]
fn page_matches_article_by_id_url_slug_and_blog_url() {
    assert!(page_matches_article(
        &serde_json::json!({ "id": 5, "url": "/blog/other" }),
        5,
        "ignored"
    ));
    assert!(page_matches_article(
        &serde_json::json!({ "id": 0, "url_slug": "my-slug" }),
        0,
        "my-slug"
    ));
    assert!(page_matches_article(
        &serde_json::json!({ "id": 0, "url": "/blog/my-slug" }),
        0,
        "my-slug"
    ));
    assert!(!page_matches_article(
        &serde_json::json!({ "id": 9, "url": "/blog/other" }),
        1,
        "my-slug"
    ));
}

#[test]
fn opportunity_score_favours_ctr_opportunity_in_top_positions() {
    let mut s = ArticleSignals::default();
    s.clicks_lost = 25.0;
    s.avg_position = 7.0;
    s.ctr_opportunity = true;
    s.impressions = 1000.0;

    let score = opportunity_score(&s);
    assert!(score > 700, "expected high score for top-10 CTR opportunity, got {}", score);
}

#[test]
fn opportunity_score_boosts_poor_content() {
    let mut s = ArticleSignals::default();
    s.content_health = "poor".to_string();
    s.checks_failed = 4;
    s.health_score = 30;

    let score = opportunity_score(&s);
    assert!(score >= 800, "expected poor content to score at least 800, got {}", score);
}

#[test]
fn opportunity_score_boosts_indexing_issues() {
    let mut s = ArticleSignals::default();
    s.indexing_status = "not_indexed_crawled".to_string();

    let score = opportunity_score(&s);
    assert!(score >= 600, "expected not_indexed_crawled to score at least 600, got {}", score);
}

#[test]
fn recommended_action_prefers_indexing_when_not_indexed() {
    let mut s = ArticleSignals::default();
    s.indexing_status = "not_indexed_crawled".to_string();
    s.internal_links = 0;

    let effort = classify_effort(&s);
    let action = recommended_action(&s, &effort);
    assert_eq!(action, "fix_indexing_internal_links");
}

#[test]
fn recommended_action_prefers_ctr_when_healthy_but_low_ctr() {
    let mut s = ArticleSignals::default();
    s.content_health = "good".to_string();
    s.ctr_opportunity = true;
    s.clicks_lost = 15.0;

    let effort = classify_effort(&s);
    let action = recommended_action(&s, &effort);
    assert_eq!(action, "fix_ctr_article");
}

#[test]
fn recommended_action_prefers_content_when_poor_health() {
    let mut s = ArticleSignals::default();
    s.content_health = "poor".to_string();
    s.ctr_opportunity = true;

    let effort = classify_effort(&s);
    let action = recommended_action(&s, &effort);
    assert_eq!(action, "fix_content_article");
}

#[test]
fn primary_signal_indexing_wins_over_ctr() {
    let mut s = ArticleSignals::default();
    s.indexing_status = "not_indexed_crawled".to_string();
    s.ctr_opportunity = true;

    assert_eq!(primary_signal(&s), "indexing");
}

#[test]
fn primary_signal_ctr_when_no_indexing_issue() {
    let mut s = ArticleSignals::default();
    s.ctr_opportunity = true;
    s.content_health = "needs_improvement".to_string();

    assert_eq!(primary_signal(&s), "ctr");
}

#[test]
fn should_skip_recently_edited_articles() {
    let mut s = ArticleSignals::default();
    s.last_edited_at = chrono::Utc::now().to_rfc3339();

    assert!(should_skip(&s));
}

#[test]
fn should_not_skip_stale_articles() {
    let mut s = ArticleSignals::default();
    s.last_edited_at = (chrono::Utc::now() - chrono::Duration::days(60)).to_rfc3339();

    assert!(!should_skip(&s));
}
