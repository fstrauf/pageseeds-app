//! Live eval: CTR fix patch generation (`fix_ctr_article` generate step).
//!
//! Runs `exec_ctr_fix_generate_with_backend` over `fixtures/evals/ctr_fix/` and checks:
//! - deterministic: patch echoes article identity, addresses every requested fix type,
//!   respects TITLE_MAX_LEN / META range / FAQ count contracts
//! - judge: would the proposed title/meta plausibly improve CTR (no clickbait)?

use serde::Deserialize;

use super::{finish_suite, generation_backend, judge_score, list_cases, load_fixture, temp_project_with_mdx, task_with_artifact, CaseReport};
use crate::engine::exec::audit_health;
use crate::engine::exec::ctr_audit::exec_ctr_fix_generate_with_backend;
use crate::models::ctr::{CtrFixPatch, CtrFixType, CtrRecommendation};

const JUDGE_CRITERIA: &[&str] = &[
    "The proposed title and meta description would plausibly improve click-through rate in search results.",
    "Copy is specific and benefit-driven, not generic or clickbait.",
    "The target keyword appears naturally in the new title and meta description.",
];

#[derive(Debug, Deserialize)]
struct CtrFixCase {
    name: String,
    mdx_path: String,
    mdx: String,
    ctr_recommendation: CtrRecommendation,
}

fn check_patch(patch: &CtrFixPatch, rec: &CtrRecommendation) -> Vec<String> {
    let mut violations = Vec::new();

    if let Some(err) = &patch.error {
        violations.push(format!("patch carries agent-reported error: {}", err));
        return violations;
    }

    if patch.article_id != rec.article_id {
        violations.push(format!(
            "article_id mismatch: expected {}, got {}",
            rec.article_id, patch.article_id
        ));
    }
    if patch.file != rec.file {
        violations.push(format!(
            "file mismatch: expected {}, got {}",
            rec.file, patch.file
        ));
    }

    for fix in &rec.fixes {
        match fix.fix_type {
            CtrFixType::TitleRewrite => match &patch.changes.title {
                Some(title) => {
                    let len = title.chars().count();
                    if len > audit_health::TITLE_MAX_LEN {
                        violations.push(format!(
                            "new title is {} chars (max {})",
                            len,
                            audit_health::TITLE_MAX_LEN
                        ));
                    }
                }
                None => violations.push(
                    "title_rewrite requested but patch has no title change".to_string(),
                ),
            },
            CtrFixType::MetaDescription => match &patch.changes.description {
                Some(desc) => {
                    let len = desc.chars().count();
                    if len < audit_health::META_MIN_LEN || len > audit_health::META_MAX_LEN {
                        violations.push(format!(
                            "new description is {} chars (allowed {}-{})",
                            len,
                            audit_health::META_MIN_LEN,
                            audit_health::META_MAX_LEN
                        ));
                    }
                }
                None => violations.push(
                    "meta_description requested but patch has no description change".to_string(),
                ),
            },
            CtrFixType::FaqSchema => match &patch.changes.faq_questions {
                Some(faqs) => {
                    if faqs.len() < 3 || faqs.len() > 5 {
                        violations.push(format!("faq count {} outside 3-5", faqs.len()));
                    }
                    for (i, faq) in faqs.iter().enumerate() {
                        if faq.question.trim().is_empty() || faq.answer.trim().is_empty() {
                            violations.push(format!("faq #{} has empty question or answer", i + 1));
                        }
                    }
                }
                None => violations
                    .push("faq_schema requested but patch has no faq_questions".to_string()),
            },
            CtrFixType::SnippetBait => {
                if patch.changes.first_paragraph.is_none() && patch.changes.snippet_patch.is_none() {
                    violations.push(
                        "snippet_bait requested but patch has neither first_paragraph nor snippet_patch"
                            .to_string(),
                    );
                }
            }
        }
    }

    violations
}

#[tokio::test]
#[ignore = "live LLM eval; run with `cargo test evals -- --ignored --nocapture`"]
async fn eval_ctr_fix_generate() {
    let _guard = super::EVAL_LOCK.lock().await;
    let backend = generation_backend().await;
    let mut reports = Vec::new();

    for case_path in list_cases("ctr_fix") {
        let case: CtrFixCase = load_fixture(&case_path);
        let project = temp_project_with_mdx(&case.mdx_path, &case.mdx);
        let task = task_with_artifact(
            "fix_ctr_article",
            "ctr_recommendations",
            serde_json::to_string(&case.ctr_recommendation).expect("serialize rec"),
        );

        let result = exec_ctr_fix_generate_with_backend(
            &task,
            project.to_str().expect("utf-8 temp path"),
            &backend,
        )
        .await;

        let mut violations = Vec::new();
        let mut judge = None;

        if !result.success {
            violations.push(format!("generate step failed: {}", result.message));
        } else {
            match serde_json::from_str::<CtrFixPatch>(
                result.output.as_deref().unwrap_or_default(),
            ) {
                Ok(patch) => {
                    violations.extend(check_patch(&patch, &case.ctr_recommendation));
                    let judge_input = format!(
                        "Target keyword: {}\nCurrent article slug: {}\nProposed CTR changes:\n{}",
                        case.ctr_recommendation.target_keyword,
                        case.ctr_recommendation.url_slug,
                        serde_json::to_string_pretty(&patch.changes).unwrap_or_default()
                    );
                    judge = judge_score(JUDGE_CRITERIA, judge_input).await;
                }
                Err(e) => violations.push(format!("step output is not a CtrFixPatch: {}", e)),
            }
        }

        reports.push(CaseReport {
            name: case.name,
            violations,
            judge,
        });
    }

    finish_suite("ctr_fix", reports);
}

// ─── Contract-check unit tests (no LLM — guard the eval harness itself) ──────

#[cfg(test)]
mod contract_tests {
    use super::*;
    use crate::models::ctr::{CtrFix, CtrFixPatchChanges};

    fn rec_with(fixes: Vec<CtrFix>) -> CtrRecommendation {
        CtrRecommendation {
            article_id: 1,
            url_slug: "slug".to_string(),
            file: "content/blog/slug.mdx".to_string(),
            priority: None,
            expected_ctr_improvement: None,
            target_keyword: "kw".to_string(),
            fixes,
        }
    }

    fn fix(fix_type: CtrFixType) -> CtrFix {
        CtrFix {
            fix_type,
            current: None,
            recommended: serde_json::Value::Null,
            reason: None,
        }
    }

    fn patch(changes: CtrFixPatchChanges) -> CtrFixPatch {
        CtrFixPatch {
            article_id: 1,
            file: "content/blog/slug.mdx".to_string(),
            error: None,
            changes,
        }
    }

    #[test]
    fn catches_missing_requested_fix() {
        let rec = rec_with(vec![fix(CtrFixType::TitleRewrite)]);
        let violations = check_patch(&patch(CtrFixPatchChanges::default()), &rec);
        assert!(
            violations.iter().any(|v| v.contains("no title change")),
            "expected missing-title violation, got: {:?}",
            violations
        );
    }

    #[test]
    fn catches_oversized_title_and_short_meta() {
        let rec = rec_with(vec![
            fix(CtrFixType::TitleRewrite),
            fix(CtrFixType::MetaDescription),
        ]);
        let changes = CtrFixPatchChanges {
            title: Some("x".repeat(80)),
            description: Some("too short".to_string()),
            ..Default::default()
        };
        let violations = check_patch(&patch(changes), &rec);
        assert!(violations.iter().any(|v| v.contains("80 chars")));
        assert!(violations.iter().any(|v| v.contains("allowed 120-155")));
    }

    #[test]
    fn clean_patch_has_no_violations() {
        let rec = rec_with(vec![
            fix(CtrFixType::TitleRewrite),
            fix(CtrFixType::MetaDescription),
            fix(CtrFixType::FaqSchema),
        ]);
        let changes = CtrFixPatchChanges {
            title: Some("kw: a tight keyword-led title".to_string()),
            description: Some(
                "a valid meta description that sits comfortably within the allowed one-twenty to one-fifty-five char range and mentions kw naturally"
                    .to_string(),
            ),
            faq_questions: Some(vec![
                crate::models::ctr::CtrFixPatchFaqQuestion {
                    question: "q1?".to_string(),
                    answer: "a1".to_string(),
                },
                crate::models::ctr::CtrFixPatchFaqQuestion {
                    question: "q2?".to_string(),
                    answer: "a2".to_string(),
                },
                crate::models::ctr::CtrFixPatchFaqQuestion {
                    question: "q3?".to_string(),
                    answer: "a3".to_string(),
                },
            ]),
            ..Default::default()
        };
        let violations = check_patch(&patch(changes), &rec);
        assert!(violations.is_empty(), "unexpected violations: {:?}", violations);
    }
}
