//! Live eval: content review single-article recommendations (`content_review` recommend step).
//!
//! Runs `build_single_article_prompt` + typed extraction over
//! `fixtures/evals/content_review/` and checks:
//! - deterministic: 4-8 suggestions, valid categories, specific (non-empty, current != proposed)
//! - judge: are the suggestions relevant to the article's actual failed checks?

use serde::Deserialize;

use super::{finish_suite, generation_backend, judge_score, list_cases, load_fixture, CaseReport};
use crate::engine::exec::content::build_single_article_prompt;
use crate::models::content_review::SingleArticleRecommendations;

/// Same preamble as production (`exec_content_review_recommend`).
const PREAMBLE: &str = "You are an expert SEO content reviewer. \
    Analyze the single article below and generate structured recommendations using the submit tool.";

const VALID_CATEGORIES: &[&str] = &[
    "title",
    "meta_description",
    "intro",
    "h1",
    "internal_links",
    "faq",
    "eeat",
    "cta",
];

const JUDGE_CRITERIA: &[&str] = &[
    "The suggestions address the article's actual failed checks and weak spots.",
    "Proposed replacements are concrete, correct, and would improve search performance.",
    "No suggestion contradicts the input context (e.g. changing published_date, inventing facts).",
];

#[derive(Debug, Deserialize)]
struct ContentReviewCase {
    name: String,
    /// Per-article context JSON as built by `exec_content_review_recommend`.
    article_context: serde_json::Value,
}

fn check_recommendations(recs: &SingleArticleRecommendations) -> Vec<String> {
    let mut violations = Vec::new();

    if recs.suggestions.len() < 4 || recs.suggestions.len() > 8 {
        violations.push(format!(
            "suggestion count {} outside required 4-8",
            recs.suggestions.len()
        ));
    }

    for (i, s) in recs.suggestions.iter().enumerate() {
        let label = format!("suggestion #{} ({})", i + 1, s.category);
        if !VALID_CATEGORIES.contains(&s.category.as_str()) {
            violations.push(format!(
                "{} has invalid category (expected one of {})",
                label,
                VALID_CATEGORIES.join(", ")
            ));
        }
        // `current` may legitimately be empty for absent elements (no FAQ, no
        // internal links, no CTA) — there is no existing text to quote then.
        if s.proposed.trim().is_empty() {
            violations.push(format!("{} has empty proposed text", label));
        }
        if !s.current.trim().is_empty() && s.current.trim() == s.proposed.trim() {
            violations.push(format!("{} proposes no change (current == proposed)", label));
        }
        if s.reason.trim().is_empty() {
            violations.push(format!("{} has empty reason", label));
        }
    }

    violations
}

#[tokio::test]
#[ignore = "live LLM eval; run with `cargo test evals -- --ignored --nocapture`"]
async fn eval_content_review_recommend() {
    let _guard = super::EVAL_LOCK.lock().await;
    let backend = generation_backend().await;
    let mut reports = Vec::new();

    for case_path in list_cases("content_review") {
        let case: ContentReviewCase = load_fixture(&case_path);
        let prompt = build_single_article_prompt(&case.article_context);

        let result = crate::rig::extraction::extract_with_backend::<
            SingleArticleRecommendations,
        >(&backend, &prompt, Some(PREAMBLE), Some("direct"), None)
        .await;

        let mut violations = Vec::new();
        let mut judge = None;

        match result {
            Err(e) => violations.push(format!("extraction failed: {}", e)),
            Ok(recs) => {
                violations.extend(check_recommendations(&recs));
                let judge_input = format!(
                    "Article context (failed checks, keyword, excerpt):\n{}\n\nGenerated suggestions:\n{}",
                    serde_json::to_string_pretty(&case.article_context).unwrap_or_default(),
                    serde_json::to_string_pretty(&recs.suggestions).unwrap_or_default()
                );
                judge = judge_score(JUDGE_CRITERIA, judge_input).await;
            }
        }

        reports.push(CaseReport {
            name: case.name,
            violations,
            judge,
        });
    }

    finish_suite("content_review", reports);
}

// ─── Contract-check unit tests (no LLM — guard the eval harness itself) ──────

#[cfg(test)]
mod contract_tests {
    use super::*;
    use crate::models::content_review::ReviewSuggestion;

    fn suggestion(category: &str, current: &str, proposed: &str) -> ReviewSuggestion {
        ReviewSuggestion {
            category: category.to_string(),
            current: current.to_string(),
            proposed: proposed.to_string(),
            reason: "because".to_string(),
            priority: None,
        }
    }

    fn recs(suggestions: Vec<ReviewSuggestion>) -> SingleArticleRecommendations {
        SingleArticleRecommendations { suggestions }
    }

    #[test]
    fn catches_too_few_suggestions() {
        let violations = check_recommendations(&recs(vec![
            suggestion("title", "a", "b"),
            suggestion("intro", "c", "d"),
        ]));
        assert!(violations.iter().any(|v| v.contains("outside required 4-8")));
    }

    #[test]
    fn catches_invalid_category_and_noop_proposal() {
        let violations = check_recommendations(&recs(vec![
            suggestion("not_a_category", "a", "b"),
            suggestion("title", "same", "same"),
            suggestion("intro", "c", "d"),
            suggestion("faq", "e", "f"),
        ]));
        assert!(violations.iter().any(|v| v.contains("invalid category")));
        assert!(violations.iter().any(|v| v.contains("proposes no change")));
    }

    #[test]
    fn clean_recommendations_have_no_violations() {
        let violations = check_recommendations(&recs(vec![
            suggestion("title", "old title", "new title with keyword"),
            suggestion("meta_description", "short", "a fuller meta description"),
            suggestion("intro", "thin intro", "a fuller direct-answer intro"),
            suggestion("internal_links", "none", "add links to related articles"),
        ]));
        assert!(violations.is_empty(), "unexpected violations: {:?}", violations);
    }
}
