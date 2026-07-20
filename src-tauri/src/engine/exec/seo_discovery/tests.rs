use super::rank::*;

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
