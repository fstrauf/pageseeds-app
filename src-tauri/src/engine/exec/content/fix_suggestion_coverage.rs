//! Suggestion-coverage validation for `fix_content_article`.
//!
//! Ensures generated/applied patches actually address open recovery suggestions
//! (especially SERP-facing fields). Shared by generate and apply so empty/no-op
//! patches cannot soft-succeed when title/meta/h1/intro still fail health.

use crate::models::content_review::{ContentFixChanges, ContentFixPatch, ReviewSuggestion};

/// Map a suggestion category (case-insensitive) to a ContentFixChanges field name.
pub(crate) fn suggestion_field(category: &str) -> Option<&'static str> {
    match category.trim().to_lowercase().as_str() {
        "title" => Some("title"),
        "h1" => Some("h1"),
        "description" | "meta" | "meta_description" => Some("description"),
        "intro" | "snippet" | "first_paragraph" => Some("intro"),
        "faq" | "faq_schema" => Some("faq_questions"),
        "internal_links" | "links" => Some("internal_links"),
        "eeat" => Some("eeat_signal"),
        "cta" => Some("cta"),
        _ => None,
    }
}

/// Whether any change field is set on the patch.
pub(crate) fn patch_has_any_changes(changes: &ContentFixChanges) -> bool {
    changes.title.is_some()
        || changes.h1.is_some()
        || changes.description.is_some()
        || changes.intro.is_some()
        || changes.internal_links.as_ref().is_some_and(|v| !v.is_empty())
        || changes.faq_questions.as_ref().is_some_and(|v| !v.is_empty())
        || changes.eeat_signal.is_some()
        || changes.cta.is_some()
}

fn patch_field_present(patch: &ContentFixPatch, field: &str) -> bool {
    match field {
        "title" => patch.changes.title.is_some(),
        "h1" => patch.changes.h1.is_some(),
        "description" => patch.changes.description.is_some(),
        "intro" => patch.changes.intro.is_some(),
        "faq_questions" => patch
            .changes
            .faq_questions
            .as_ref()
            .is_some_and(|v| !v.is_empty()),
        "internal_links" => patch
            .changes
            .internal_links
            .as_ref()
            .is_some_and(|v| !v.is_empty()),
        "eeat_signal" => patch.changes.eeat_signal.is_some(),
        "cta" => patch.changes.cta.is_some(),
        _ => false,
    }
}

/// Body has a non-empty first H1 (`# ...`, not `##`).
pub(crate) fn body_has_h1(original_content: &str) -> bool {
    let body = crate::content::frontmatter::split_mdx(original_content)
        .map(|(_, b)| b)
        .unwrap_or(original_content);
    body.lines().any(|line| {
        let t = line.trim_start();
        t.starts_with("# ") && !t.starts_with("## ") && t.len() > 2
    })
}

/// Deterministic health for a logical patch field against current file content.
/// Non-measurable fields (links/eeat/cta) are never treated as already healthy.
pub(crate) fn field_already_healthy(field: &str, original_content: &str) -> bool {
    let (title, meta, first) =
        crate::engine::exec::ctr_audit::parse_content_excerpt(original_content);
    match field {
        "title" => {
            let len = title.chars().count();
            !title.trim().is_empty() && len <= crate::engine::exec::audit_health::TITLE_MAX_LEN
        }
        "description" => {
            let len = meta.chars().count();
            len >= crate::engine::exec::audit_health::META_MIN_LEN
                && len <= crate::engine::exec::audit_health::META_MAX_LEN
        }
        "intro" => {
            let wc = crate::content::ops::count_words(&first);
            wc >= crate::engine::exec::audit_health::SNIPPET_MIN_WORDS
                && wc <= crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
        }
        "h1" => body_has_h1(original_content),
        "faq_questions" => {
            crate::engine::exec::audit_health::has_frontmatter_faq(original_content)
        }
        // No reliable "already satisfied" signal for these.
        "internal_links" | "eeat_signal" | "cta" => false,
        _ => false,
    }
}

/// Logical field names still open (suggestion present, patch missing field, health fails).
pub(crate) fn unsatisfied_suggestion_fields(
    suggestions: &[ReviewSuggestion],
    patch: Option<&ContentFixPatch>,
    original_content: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    for sug in suggestions {
        let Some(field) = suggestion_field(&sug.category) else {
            continue;
        };
        if let Some(p) = patch {
            if patch_field_present(p, field) {
                continue;
            }
        }
        if field_already_healthy(field, original_content) {
            continue;
        }
        if !out.iter().any(|f| f == field) {
            out.push(field.to_string());
        }
    }
    out
}

/// Validate that the patch covers open suggestions (or current content already passes health).
///
/// Rules (mirrors CTR `validate_patch_against_recommendation` with stricter SERP recovery):
/// - Missing SERP fields (title/description/h1/intro/faq) while current content fails health → error
/// - Completely empty changes with any unsatisfied suggestion field → error
/// - Links/CTA-only patches while title/meta/h1 remain unsatisfied → error via SERP field rules
pub(crate) fn validate_patch_against_suggestions(
    patch: &ContentFixPatch,
    suggestions: &[ReviewSuggestion],
    original_content: &str,
) -> Vec<String> {
    if suggestions.is_empty() {
        return Vec::new();
    }

    let mut errors = Vec::new();
    let unsatisfied = unsatisfied_suggestion_fields(suggestions, Some(patch), original_content);
    let has_any_change = patch_has_any_changes(&patch.changes);

    for field in &unsatisfied {
        match field.as_str() {
            // Hard SERP fields — missing while unhealthy always fails.
            "title" | "description" | "h1" => {
                errors.push(format!(
                    "suggestion requested {field} but patch has no {field} and current content fails health for that field"
                ));
            }
            // Intro/FAQ often fail field validation (word count, keyword mess).
            // When the patch still lands other changes, allow partial recovery
            // instead of discarding title/meta too.
            "intro" | "faq_questions" => {
                if !has_any_change {
                    errors.push(format!(
                        "suggestion requested {field} but patch has no {field} and current content fails health for that field"
                    ));
                }
            }
            other => {
                // Soft fields: only fail when the patch is empty.
                if !has_any_change && !errors.iter().any(|e| e.contains(other)) {
                    errors.push(format!(
                        "suggestion requested {other} but patch has no {other}"
                    ));
                }
            }
        }
    }

    if !has_any_change && !unsatisfied.is_empty() {
        let summary = format!(
            "Empty/no-op patch is not success: {} suggestion field(s) remain unsatisfied ({})",
            unsatisfied.len(),
            unsatisfied.join(", ")
        );
        if !errors.iter().any(|e| e.starts_with("Empty/no-op")) {
            errors.insert(0, summary);
        }
    }

    errors
}

/// Operator-facing message when apply wrote nothing but open suggestions remain.
pub(crate) fn empty_apply_unsatisfied_message(unsatisfied: &[String]) -> String {
    format!(
        "Patch applied no changes but {} suggestion(s) remain unsatisfied (e.g. {}). \
Empty/no-op patches are not success for open recovery suggestions.",
        unsatisfied.len(),
        unsatisfied
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::content_review::{ContentFixChanges, ContentFixLink, ContentFixPatch};

    // Unhealthy meta (short) + short intro + healthy-length title + H1 present.
    const UNHEALTHY_MDX: &str = "---\ntitle: \"Container Gardening Basics\"\ndescription: \"Too short.\"\ndate: \"2026-01-01\"\n---\n\n# Container Gardening Basics\n\nShort intro only.\n";

    // Title over limit, no H1, short meta/intro.
    const BAD_TITLE_NO_H1: &str = "---\ntitle: \"This Title Is Definitely Way Too Long For SEO And Will Fail Health Checks Completely\"\ndescription: \"Short.\"\ndate: \"2026-01-01\"\n---\n\nNo heading here, just a paragraph that is still too short for intro health.\n";

    fn sug(category: &str) -> ReviewSuggestion {
        ReviewSuggestion {
            category: category.to_string(),
            current: "old".to_string(),
            proposed: "new".to_string(),
            reason: "test".to_string(),
            priority: None,
        }
    }

    fn empty_patch() -> ContentFixPatch {
        ContentFixPatch {
            article_id: 99,
            file: "content/wrong.mdx".to_string(),
            error: None,
            changes: ContentFixChanges::default(),
        }
    }

    #[test]
    fn empty_patch_with_open_title_and_description_fails() {
        let patch = empty_patch();
        let suggestions = vec![sug("title"), sug("description")];
        // Title in UNHEALTHY_MDX is healthy; description is not — only description should fail health.
        // Use BAD_TITLE so title is also unsatisfied.
        let errors =
            validate_patch_against_suggestions(&patch, &suggestions, BAD_TITLE_NO_H1);
        assert!(
            errors.iter().any(|e| e.contains("Empty/no-op")),
            "expected empty-patch error, got {:?}",
            errors
        );
        assert!(
            errors.iter().any(|e| e.contains("title")),
            "expected title unsatisfied, got {:?}",
            errors
        );
        assert!(
            errors.iter().any(|e| e.contains("description")),
            "expected description unsatisfied, got {:?}",
            errors
        );
    }

    #[test]
    fn links_only_fails_when_title_still_unhealthy() {
        let patch = ContentFixPatch {
            article_id: 1,
            file: "content/a.mdx".to_string(),
            error: None,
            changes: ContentFixChanges {
                internal_links: Some(vec![ContentFixLink {
                    anchor_text: "related".to_string(),
                    target_slug: "other-post".to_string(),
                }]),
                ..Default::default()
            },
        };
        let suggestions = vec![sug("title"), sug("internal_links")];
        let errors = validate_patch_against_suggestions(&patch, &suggestions, BAD_TITLE_NO_H1);
        assert!(
            errors.iter().any(|e| e.contains("title")),
            "links-only must fail when title suggestion remains unsatisfied: {:?}",
            errors
        );
        // Not empty — so Empty/no-op summary may be absent; SERP field error is enough.
        assert!(!errors.is_empty());
    }

    #[test]
    fn missing_field_ok_when_already_healthy() {
        // Title is healthy in UNHEALTHY_MDX; only description suggestion should complain if empty.
        let patch = empty_patch();
        let suggestions = vec![sug("title")];
        let errors = validate_patch_against_suggestions(&patch, &suggestions, UNHEALTHY_MDX);
        assert!(
            errors.is_empty(),
            "healthy title should allow omitting title change: {:?}",
            errors
        );
    }

    #[test]
    fn empty_patch_ok_when_no_suggestions() {
        let patch = empty_patch();
        let errors = validate_patch_against_suggestions(&patch, &[], UNHEALTHY_MDX);
        assert!(errors.is_empty());
    }

    #[test]
    fn category_aliases_map_to_fields() {
        assert_eq!(suggestion_field("META"), Some("description"));
        assert_eq!(suggestion_field("meta_description"), Some("description"));
        assert_eq!(suggestion_field("first_paragraph"), Some("intro"));
        assert_eq!(suggestion_field("links"), Some("internal_links"));
        assert_eq!(suggestion_field("faq_schema"), Some("faq_questions"));
    }

    #[test]
    fn h1_missing_fails_when_body_has_no_h1() {
        let patch = empty_patch();
        let suggestions = vec![sug("h1")];
        let errors = validate_patch_against_suggestions(&patch, &suggestions, BAD_TITLE_NO_H1);
        assert!(
            errors.iter().any(|e| e.contains("h1")),
            "expected h1 error, got {:?}",
            errors
        );
    }

    #[test]
    fn h1_missing_ok_when_body_has_h1() {
        let patch = empty_patch();
        let suggestions = vec![sug("h1")];
        let errors = validate_patch_against_suggestions(&patch, &suggestions, UNHEALTHY_MDX);
        assert!(
            errors.is_empty(),
            "existing H1 should satisfy h1 suggestion: {:?}",
            errors
        );
    }

    #[test]
    fn partial_title_ok_when_intro_still_unsatisfied() {
        // Title lands; intro suggestion remains open — partial SERP recovery must not
        // hard-fail the whole patch (intro often fails word-count after LLM).
        let patch = ContentFixPatch {
            article_id: 1,
            file: "content/a.mdx".to_string(),
            error: None,
            changes: ContentFixChanges {
                title: Some("Short Fixed Title Under Limit".to_string()),
                ..Default::default()
            },
        };
        let suggestions = vec![sug("title"), sug("intro")];
        let errors = validate_patch_against_suggestions(&patch, &suggestions, BAD_TITLE_NO_H1);
        assert!(
            errors.is_empty(),
            "title-only partial patch should pass when intro still open: {:?}",
            errors
        );
    }

    #[test]
    fn unsatisfied_fields_without_patch_for_apply() {
        let fields =
            unsatisfied_suggestion_fields(&[sug("description"), sug("title")], None, BAD_TITLE_NO_H1);
        assert!(fields.contains(&"description".to_string()));
        assert!(fields.contains(&"title".to_string()));
    }
}
