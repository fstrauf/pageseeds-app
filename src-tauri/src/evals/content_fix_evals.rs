//! Live eval: content fix patch generation (`fix_content_article` generate step).
//!
//! Runs `exec_fix_content_article_generate_with_backend` over `fixtures/evals/content_fix/`
//! and checks:
//! - deterministic: patch echoes article identity, respects length/keyword contracts,
//!   and — most importantly — never hallucinates internal-link slugs
//! - judge: do the proposed changes genuinely improve the article for the target keyword?

use serde::Deserialize;

use super::{finish_suite, generation_backend, judge_score, list_cases, load_fixture, temp_project_with_mdx, task_with_artifact, CaseReport};
use crate::content::ops::count_words;
use crate::engine::exec::audit_health;
use crate::engine::exec::content::exec_fix_content_article_generate_with_backend;
use crate::engine::exec::content::keyword_words_present;
use crate::models::content_review::ContentFixPatch;

const JUDGE_CRITERIA: &[&str] = &[
    "The proposed changes genuinely improve the article's SEO for the target keyword.",
    "New copy reads naturally — the keyword is not stuffed or awkwardly forced.",
    "Internal link anchor text is descriptive and relevant to the target slug.",
];

#[derive(Debug, Deserialize)]
struct ContentFixCase {
    name: String,
    mdx_path: String,
    mdx: String,
    /// Raw `content_fix_context` artifact payload (see `extract_context`).
    content_fix_context: serde_json::Value,
    /// Slugs the model is allowed to link to (the ones present in its context).
    valid_slugs: Vec<String>,
}

fn check_patch(
    patch: &ContentFixPatch,
    context: &serde_json::Value,
    valid_slugs: &[String],
) -> Vec<String> {
    let mut violations = Vec::new();

    let requested: Vec<&str> = context["suggestions"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s["category"].as_str())
                .collect()
        })
        .unwrap_or_default();

    if let Some(err) = &patch.error {
        // A refusal is a CORRECT outcome only when the only requested category
        // is internal_links AND the context provided no valid link targets
        // (empty `available_link_slugs`) — then the prompt's "if you are
        // unsure whether a target exists, do NOT include it" rule applies.
        // When the context supplies a deterministic target list, the model
        // has verified slugs and must link from it, so a refusal fails.
        // The refusal wording varies, so match structurally: links-only
        // request + error mentioning links. Any other agent-reported error
        // fails the case.
        let only_links_requested = requested.len() == 1 && requested.contains(&"internal_links");
        let is_link_refusal = err.to_lowercase().contains("link");
        let no_targets_provided = context["available_link_slugs"]
            .as_array()
            .map(|arr| arr.is_empty())
            .unwrap_or(true);
        if !(only_links_requested && is_link_refusal && no_targets_provided) {
            violations.push(format!("patch carries agent-reported error: {}", err));
        }
        return violations;
    }

    let expected_id = context["article_id"].as_i64().unwrap_or(0);
    let expected_file = context["article_file"].as_str().unwrap_or("");
    let keyword = context["target_keyword"].as_str().unwrap_or("");

    if patch.article_id != expected_id {
        violations.push(format!(
            "article_id mismatch: expected {}, got {}",
            expected_id, patch.article_id
        ));
    }
    if patch.file != expected_file {
        violations.push(format!(
            "file mismatch: expected {}, got {}",
            expected_file, patch.file
        ));
    }

    let changes = &patch.changes;

    // Word-level keyword presence, identical to the production verifier
    // (`fix_verify.rs`): every significant keyword token must appear, so
    // "companion planting in containers" satisfies "companion planting containers".
    let kw_lower = keyword.to_lowercase();

    if requested.contains(&"title") {
        match &changes.title {
            Some(title) => {
                let len = title.chars().count();
                if len > audit_health::TITLE_MAX_LEN {
                    violations.push(format!(
                        "new title is {} chars (max {})",
                        len,
                        audit_health::TITLE_MAX_LEN
                    ));
                }
                if !keyword.is_empty()
                    && !keyword_words_present(&kw_lower, &title.to_lowercase())
                {
                    violations.push(format!("new title missing target keyword \"{}\"", keyword));
                }
            }
            None => violations.push("title suggested but patch has no title change".to_string()),
        }
    }

    if requested.contains(&"meta_description") {
        match &changes.description {
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
                if !keyword.is_empty()
                    && !keyword_words_present(&kw_lower, &desc.to_lowercase())
                {
                    violations
                        .push(format!("new description missing target keyword \"{}\"", keyword));
                }
            }
            None => violations
                .push("meta_description suggested but patch has no description change".to_string()),
        }
    }

    if requested.contains(&"h1") {
        match &changes.h1 {
            Some(h1) => {
                if !keyword.is_empty()
                    && !keyword_words_present(&kw_lower, &h1.to_lowercase())
                {
                    violations.push(format!("new h1 missing target keyword \"{}\"", keyword));
                }
            }
            None => violations.push("h1 suggested but patch has no h1 change".to_string()),
        }
    }

    if requested.contains(&"intro") {
        match &changes.intro {
            Some(intro) => {
                let words = count_words(intro);
                if !(40..=60).contains(&words) {
                    violations.push(format!("new intro is {} words (allowed 40-60)", words));
                }
                if !keyword.is_empty()
                    && !keyword_words_present(&kw_lower, &intro.to_lowercase())
                {
                    violations.push(format!("new intro missing target keyword \"{}\"", keyword));
                }
            }
            None => violations.push("intro suggested but patch has no intro change".to_string()),
        }
    }

    if requested.contains(&"internal_links") {
        match &changes.internal_links {
            Some(links) => {
                for link in links {
                    if link.anchor_text.trim().is_empty() {
                        violations.push("internal link has empty anchor_text".to_string());
                    }
                    if link.target_slug.starts_with('/') || link.target_slug.contains('/') {
                        violations.push(format!(
                            "target_slug \"{}\" must be a bare slug, not a path",
                            link.target_slug
                        ));
                    }
                    if !valid_slugs.iter().any(|s| s == &link.target_slug) {
                        violations.push(format!(
                            "hallucinated link target \"{}\" (not in provided valid slugs)",
                            link.target_slug
                        ));
                    }
                }
            }
            None => {
                // Omission is accepted only when the context provided no valid
                // link targets — then the prompt's "if you are unsure whether a
                // target exists, do NOT include it" rule applies. When the
                // context supplies a deterministic `available_link_slugs` list,
                // the model must link from it; silent omission is a failure.
                let targets_provided = context["available_link_slugs"]
                    .as_array()
                    .map(|arr| !arr.is_empty())
                    .unwrap_or(false);
                if targets_provided {
                    violations.push(
                        "internal_links suggested but patch has no internal_links change \
                         even though available_link_slugs were provided in the context"
                            .to_string(),
                    );
                }
            }
        }
    }

    if requested.contains(&"faq") {
        match &changes.faq_questions {
            Some(faqs) => {
                if faqs.len() < 3 || faqs.len() > 5 {
                    violations.push(format!("faq count {} outside 3-5", faqs.len()));
                }
            }
            None => violations.push("faq suggested but patch has no faq_questions".to_string()),
        }
    }

    violations
}

#[tokio::test]
#[ignore = "live LLM eval; run with `cargo test evals -- --ignored --nocapture`"]
async fn eval_content_fix_generate() {
    let _guard = super::EVAL_LOCK.lock().await;
    let backend = generation_backend().await;
    let mut reports = Vec::new();

    for case_path in list_cases("content_fix") {
        let case: ContentFixCase = load_fixture(&case_path);
        let project = temp_project_with_mdx(&case.mdx_path, &case.mdx);
        // Materialize the linkable articles as stubs so a project-aware (acp)
        // backend sees the suggested targets as existing — matching production,
        // where suggestions only contain slugs from the real project.
        for slug in &case.valid_slugs {
            let stub = project.join(format!("content/blog/{}.mdx", slug));
            std::fs::write(
                &stub,
                format!(
                    "---\ntitle: \"{}\"\ndate: \"2026-01-01\"\n---\n\n# {}\n\nStub article for eval fixture.\n",
                    slug, slug
                ),
            )
            .expect("write link-target stub");
        }
        let task = task_with_artifact(
            "fix_content_article",
            "content_fix_context",
            serde_json::to_string(&case.content_fix_context).expect("serialize context"),
        );

        let result = exec_fix_content_article_generate_with_backend(
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
            match serde_json::from_str::<ContentFixPatch>(
                result.output.as_deref().unwrap_or_default(),
            ) {
                Ok(patch) => {
                    violations.extend(check_patch(
                        &patch,
                        &case.content_fix_context,
                        &case.valid_slugs,
                    ));
                    let judge_input = format!(
                        "Target keyword: {}\nArticle: {}\nProposed content changes:\n{}",
                        case.content_fix_context["target_keyword"].as_str().unwrap_or(""),
                        case.content_fix_context["article_title"].as_str().unwrap_or(""),
                        serde_json::to_string_pretty(&patch.changes).unwrap_or_default()
                    );
                    judge = judge_score(JUDGE_CRITERIA, judge_input).await;
                }
                Err(e) => violations.push(format!("step output is not a ContentFixPatch: {}", e)),
            }
        }

        reports.push(CaseReport {
            name: case.name,
            violations,
            judge,
        });
    }

    finish_suite("content_fix", reports);
}

// ─── Contract-check unit tests (no LLM — guard the eval harness itself) ──────

#[cfg(test)]
mod contract_tests {
    use super::*;
    use crate::models::content_review::{ContentFixChanges, ContentFixLink};

    fn context(categories: &[&str]) -> serde_json::Value {
        let suggestions: Vec<serde_json::Value> = categories
            .iter()
            .map(|c| serde_json::json!({"category": c, "current": "x", "proposed": "y", "reason": "z"}))
            .collect();
        serde_json::json!({
            "article_id": 7,
            "article_file": "content/blog/slug.mdx",
            "article_title": "Some Article",
            "target_keyword": "container gardening",
            "suggestions": suggestions,
        })
    }

    fn valid_slugs() -> Vec<String> {
        vec!["real-article".to_string(), "another-real-one".to_string()]
    }

    fn context_with_link_targets(categories: &[&str], slugs: &[&str]) -> serde_json::Value {
        let mut ctx = context(categories);
        ctx["available_link_slugs"] = serde_json::json!(slugs);
        ctx
    }

    fn patch(changes: ContentFixChanges) -> ContentFixPatch {
        ContentFixPatch {
            article_id: 7,
            file: "content/blog/slug.mdx".to_string(),
            error: None,
            changes,
        }
    }

    #[test]
    fn catches_hallucinated_link_slug() {
        let changes = ContentFixChanges {
            internal_links: Some(vec![
                ContentFixLink {
                    anchor_text: "good link".to_string(),
                    target_slug: "real-article".to_string(),
                },
                ContentFixLink {
                    anchor_text: "bad link".to_string(),
                    target_slug: "invented-article".to_string(),
                },
            ]),
            ..Default::default()
        };
        let violations = check_patch(&patch(changes), &context(&["internal_links"]), &valid_slugs());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("invented-article"));
    }

    #[test]
    fn catches_path_style_link_slug() {
        let changes = ContentFixChanges {
            internal_links: Some(vec![ContentFixLink {
                anchor_text: "link".to_string(),
                target_slug: "/blog/real-article".to_string(),
            }]),
            ..Default::default()
        };
        let violations = check_patch(&patch(changes), &context(&["internal_links"]), &valid_slugs());
        assert!(violations.iter().any(|v| v.contains("bare slug")));
    }

    #[test]
    fn catches_missing_keyword_in_title() {
        let changes = ContentFixChanges {
            title: Some("A perfectly sized title without the phrase".to_string()),
            ..Default::default()
        };
        let violations = check_patch(&patch(changes), &context(&["title"]), &valid_slugs());
        assert!(violations.iter().any(|v| v.contains("missing target keyword")));
    }

    #[test]
    fn clean_patch_has_no_violations() {
        let changes = ContentFixChanges {
            title: Some("Container Gardening Basics for Small Spaces".to_string()),
            internal_links: Some(vec![ContentFixLink {
                anchor_text: "container gardening basics".to_string(),
                target_slug: "real-article".to_string(),
            }]),
            ..Default::default()
        };
        let violations = check_patch(
            &patch(changes),
            &context(&["title", "internal_links"]),
            &valid_slugs(),
        );
        assert!(violations.is_empty(), "unexpected violations: {:?}", violations);
    }

    #[test]
    fn accepts_documented_link_refusal() {
        let p = ContentFixPatch {
            article_id: 7,
            file: "content/blog/slug.mdx".to_string(),
            error: Some(
                "Suggested link targets do not match any existing article in this project; \
                 unverifiable links must not be added, so no changes are applied."
                    .to_string(),
            ),
            changes: ContentFixChanges::default(),
        };
        let violations = check_patch(&p, &context(&["internal_links"]), &valid_slugs());
        assert!(violations.is_empty(), "unexpected violations: {:?}", violations);
    }

    #[test]
    fn rejects_unrelated_agent_error() {
        let p = ContentFixPatch {
            article_id: 7,
            file: "content/blog/slug.mdx".to_string(),
            error: Some("could not parse the article frontmatter".to_string()),
            changes: ContentFixChanges::default(),
        };
        let violations = check_patch(&p, &context(&["title"]), &valid_slugs());
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("agent-reported error"));
    }

    #[test]
    fn requires_links_when_targets_provided() {
        let violations = check_patch(
            &patch(ContentFixChanges::default()),
            &context_with_link_targets(&["internal_links"], &["real-article"]),
            &valid_slugs(),
        );
        assert!(
            violations.iter().any(|v| v.contains("available_link_slugs")),
            "expected an omission violation, got: {:?}",
            violations
        );
    }

    #[test]
    fn accepts_omission_when_no_targets_provided() {
        let violations = check_patch(
            &patch(ContentFixChanges::default()),
            &context(&["internal_links"]),
            &valid_slugs(),
        );
        assert!(violations.is_empty(), "unexpected violations: {:?}", violations);
    }

    #[test]
    fn rejects_link_refusal_when_targets_provided() {
        let p = ContentFixPatch {
            article_id: 7,
            file: "content/blog/slug.mdx".to_string(),
            error: Some("could not verify the suggested link targets".to_string()),
            changes: ContentFixChanges::default(),
        };
        let violations = check_patch(
            &p,
            &context_with_link_targets(&["internal_links"], &["real-article"]),
            &valid_slugs(),
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("agent-reported error"));
    }
}
