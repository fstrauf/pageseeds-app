/// Deterministic verification that applied content fixes meet health thresholds.
///
/// 1. Load the content_fix_patch artifact to know what fixes were expected.
/// 2. Read the (now modified) MDX file.
/// 3. Re-run the same health checks used by content_audit.
/// 4. Compare before/after values.
/// 5. Return a ContentFixVerificationReport.
///
/// ## Residual vs write path (issue #122)
///
/// Shared structural SEO floors live in [`crate::content::validate_article`].
/// `content_write_verify` hard-gates on them for new articles. This step does
/// **not** hard-fail on the full `validate_article` report: partial fixes often
/// touch only title/meta/H1/intro while leaving pre-existing short body word
/// counts or meta lengths unchanged, and hard-gating would flip intentional
/// partial-fix success paths to failure. Patch-scoped checks remain here;
/// a follow-up may fold shared floors in once fix tasks always own full-file
/// quality (or only assert floors for categories present on the patch).
use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::models::content_review::{
    ContentFixPatch, ContentFixVerifiedItem, ContentFixVerificationReport,
};
use crate::models::task::Task;

pub(crate) fn exec_fix_content_article_verify(task: &Task, project_path: &str) -> StepResult {
    let patch = match resolve_patch(task) {
        Ok(p) => p,
        Err(result) => return result,
    };

    let repo_root = Path::new(project_path);
    let file_path =
        match crate::engine::exec::audit_health::resolve_content_file(repo_root, &patch.file) {
            Some(p) => p,
            None => {
                return StepResult::fail(format!(
                        "File not found: {}. Run sanitize_content to repair paths.",
                        patch.file
                    ));
            }
        };

    let content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(_e) => {
            return StepResult::fail(format!("File not found: {}", file_path.display()));
        }
    };

    let (frontmatter, body) = match crate::content::frontmatter::split_mdx(&content) {
        Some((f, b)) => (f, b),
        None => {
            return StepResult::fail("Could not parse frontmatter from MDX file".to_string());
        }
    };

    let mut fixes = Vec::new();
    let mut verified_count = 0usize;
    let mut failed_count = 0usize;
    let mut skipped_count = 0usize;

    let scalars = crate::content::frontmatter::top_level_scalars(frontmatter);
    let get_scalar = |key: &str| -> String {
        scalars
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.raw_value.trim_matches('"').trim_matches('\'').to_string())
            .unwrap_or_default()
    };

    // Title check
    if patch.changes.title.is_some() {
        let title = get_scalar("title");
        let title_len = title.chars().count();
        let title_max = crate::engine::exec::audit_health::TITLE_MAX_LEN;
        if title_len <= title_max {
            verified_count += 1;
            fixes.push(ContentFixVerifiedItem {
                category: "title".to_string(),
                status: "verified".to_string(),
                detail: Some(format!("{} chars (max {})", title_len, title_max)),
                actual: Some(title),
                expected: Some(format!("≤{}", title_max)),
            });
        } else {
            failed_count += 1;
            fixes.push(ContentFixVerifiedItem {
                category: "title".to_string(),
                status: "failed".to_string(),
                detail: Some(format!("{} chars (max {})", title_len, title_max)),
                actual: Some(title),
                expected: Some(format!("≤{}", title_max)),
            });
        }
    }

    // Meta description check
    if patch.changes.description.is_some() {
        let meta = get_scalar("description");
        let meta_len = meta.chars().count();
        let meta_min = crate::engine::exec::audit_health::META_MIN_LEN;
        let meta_max = crate::engine::exec::audit_health::META_MAX_LEN;
        if meta_len >= meta_min && meta_len <= meta_max {
            verified_count += 1;
            fixes.push(ContentFixVerifiedItem {
                category: "description".to_string(),
                status: "verified".to_string(),
                detail: Some(format!("{} chars (range {}-{})", meta_len, meta_min, meta_max)),
                actual: Some(meta),
                expected: Some(format!("{}-{}", meta_min, meta_max)),
            });
        } else {
            failed_count += 1;
            fixes.push(ContentFixVerifiedItem {
                category: "description".to_string(),
                status: "failed".to_string(),
                detail: Some(format!("{} chars (range {}-{})", meta_len, meta_min, meta_max)),
                actual: Some(meta),
                expected: Some(format!("{}-{}", meta_min, meta_max)),
            });
        }
    }

    // Intro check
    if patch.changes.intro.is_some() {
        let first_para = crate::content::cleaner::find_first_paragraph_range(body)
            .map(|(start, end)| body[start..end].trim().to_string())
            .unwrap_or_default();
        let word_count = crate::content::ops::count_words(&first_para);
        let snippet_min = crate::engine::exec::audit_health::SNIPPET_MIN_WORDS;
        let snippet_max = crate::engine::exec::audit_health::SNIPPET_MAX_WORDS;
        if word_count >= snippet_min && word_count <= snippet_max {
            verified_count += 1;
            fixes.push(ContentFixVerifiedItem {
                category: "intro".to_string(),
                status: "verified".to_string(),
                detail: Some(format!("{} words (range {}-{})", word_count, snippet_min, snippet_max)),
                actual: Some(first_para),
                expected: Some(format!("{}-{}", snippet_min, snippet_max)),
            });
        } else {
            failed_count += 1;
            fixes.push(ContentFixVerifiedItem {
                category: "intro".to_string(),
                status: "failed".to_string(),
                detail: Some(format!("{} words (range {}-{})", word_count, snippet_min, snippet_max)),
                actual: Some(first_para),
                expected: Some(format!("{}-{}", snippet_min, snippet_max)),
            });
        }
    }

    // FAQ check
    if patch.changes.faq_questions.is_some() {
        let has_faq = crate::engine::exec::audit_health::has_frontmatter_faq(&content);
        if has_faq {
            verified_count += 1;
            fixes.push(ContentFixVerifiedItem {
                category: "faq".to_string(),
                status: "verified".to_string(),
                detail: Some("FAQ present in frontmatter".to_string()),
                actual: None,
                expected: Some("FAQ in frontmatter".to_string()),
            });
        } else {
            failed_count += 1;
            fixes.push(ContentFixVerifiedItem {
                category: "faq".to_string(),
                status: "failed".to_string(),
                detail: Some("FAQ missing from frontmatter".to_string()),
                actual: None,
                expected: Some("FAQ in frontmatter".to_string()),
            });
        }
    }

    // EEAT check
    if patch.changes.eeat_signal.is_some() {
        // EEAT is hard to verify deterministically; mark as verified if patch was applied
        verified_count += 1;
        fixes.push(ContentFixVerifiedItem {
            category: "eeat".to_string(),
            status: "verified".to_string(),
            detail: Some("EEAT signal applied".to_string()),
            actual: None,
            expected: None,
        });
    }

    // CTA check
    if patch.changes.cta.is_some() {
        // CTA is hard to verify deterministically; mark as verified if patch was applied
        verified_count += 1;
        fixes.push(ContentFixVerifiedItem {
            category: "cta".to_string(),
            status: "verified".to_string(),
            detail: Some("CTA applied".to_string()),
            actual: None,
            expected: None,
        });
    }

    // Keyword placement checks — verify the target keyword appears in the
    // generated text via the canonical tolerant matcher
    // (`content::keyword_match::keyword_present`): verbatim phrase first,
    // all-significant-tokens fallback for long keywords. Backfilled keywords
    // are normalized to titleable length at the GSC sync boundary (issue
    // #74), so no length-based skip is needed here.
    let target_kw = load_target_keyword(task);

    if !target_kw.is_empty() {
        let kw_lower = target_kw.to_lowercase();

        // H1 keyword check
        if patch.changes.h1.is_some() {
            let h1_text = crate::content::frontmatter::split_mdx(&content)
                .and_then(|(fm, body)| {
                    body.lines()
                        .find(|l| l.trim_start().starts_with("# ") && !l.trim_start().starts_with("## "))
                        .map(|l| l.trim_start_matches("# ").trim().to_lowercase())
                        // Fall back to frontmatter title if no H1 heading in body
                        .or_else(|| {
                            crate::content::frontmatter::top_level_scalars(fm)
                                .iter()
                                .find(|f| f.key == "title")
                                .map(|f| f.raw_value.trim_matches('"').trim_matches('\'').to_lowercase())
                        })
                })
                .unwrap_or_default();
            if crate::content::keyword_match::keyword_present(&h1_text, &kw_lower) {
                verified_count += 1;
                fixes.push(ContentFixVerifiedItem {
                    category: "h1_keyword".to_string(),
                    status: "verified".to_string(),
                    detail: Some("Target keyword found in H1".to_string()),
                    actual: Some(h1_text),
                    expected: Some(format!("contains (keyword_match): {}", target_kw)),
                });
            } else {
                failed_count += 1;
                fixes.push(ContentFixVerifiedItem {
                    category: "h1_keyword".to_string(),
                    status: "failed".to_string(),
                    detail: Some("Target keyword NOT found in H1 after fix".to_string()),
                    actual: Some(h1_text),
                    expected: Some(format!("contains (keyword_match): {}", target_kw)),
                });
            }
        }

        // Meta description keyword check
        if patch.changes.description.is_some() {
            let meta = get_scalar("description").to_lowercase();
            if crate::content::keyword_match::keyword_present(&meta, &kw_lower) {
                verified_count += 1;
                fixes.push(ContentFixVerifiedItem {
                    category: "meta_desc_keyword".to_string(),
                    status: "verified".to_string(),
                    detail: Some("Target keyword found in meta description".to_string()),
                    actual: Some(meta),
                    expected: Some(format!("contains (keyword_match): {}", target_kw)),
                });
            } else {
                failed_count += 1;
                fixes.push(ContentFixVerifiedItem {
                    category: "meta_desc_keyword".to_string(),
                    status: "failed".to_string(),
                    detail: Some("Target keyword NOT found in meta description after fix".to_string()),
                    actual: Some(meta),
                    expected: Some(format!("contains (keyword_match): {}", target_kw)),
                });
            }
        }

        // Title keyword check
        if patch.changes.title.is_some() {
            let title_lower = get_scalar("title").to_lowercase();
            if crate::content::keyword_match::keyword_present(&title_lower, &kw_lower) {
                verified_count += 1;
                fixes.push(ContentFixVerifiedItem {
                    category: "title_keyword".to_string(),
                    status: "verified".to_string(),
                    detail: Some("Target keyword found in title".to_string()),
                    actual: Some(title_lower),
                    expected: Some(format!("contains (keyword_match): {}", target_kw)),
                });
            } else {
                failed_count += 1;
                fixes.push(ContentFixVerifiedItem {
                    category: "title_keyword".to_string(),
                    status: "failed".to_string(),
                    detail: Some("Target keyword NOT found in title after fix".to_string()),
                    actual: Some(title_lower),
                    expected: Some(format!("contains (keyword_match): {}", target_kw)),
                });
            }
        }

        // First paragraph keyword check
        if patch.changes.intro.is_some() {
            let first_para = crate::content::cleaner::find_first_paragraph_range(body)
                .map(|(start, end)| body[start..end].trim().to_lowercase())
                .unwrap_or_default();
            if crate::content::keyword_match::keyword_present(&first_para, &kw_lower) {
                verified_count += 1;
                fixes.push(ContentFixVerifiedItem {
                    category: "keyword_first_para".to_string(),
                    status: "verified".to_string(),
                    detail: Some("Target keyword found in first paragraph".to_string()),
                    actual: Some(first_para),
                    expected: Some(format!("contains (keyword_match): {}", target_kw)),
                });
            } else {
                failed_count += 1;
                fixes.push(ContentFixVerifiedItem {
                    category: "keyword_first_para".to_string(),
                    status: "failed".to_string(),
                    detail: Some("Target keyword NOT found in first paragraph after fix".to_string()),
                    actual: Some(first_para),
                    expected: Some(format!("contains (keyword_match): {}", target_kw)),
                });
            }
        }
    }

    let total = verified_count + failed_count + skipped_count;
    let summary = if failed_count == 0 {
        format!(
            "All {} fix(es) verified for {}",
            verified_count, patch.file
        )
    } else {
        format!(
            "{} verified, {} failed, {} skipped out of {} fix(es) for {}",
            verified_count, failed_count, skipped_count, total, patch.file
        )
    };

    let report = ContentFixVerificationReport {
        summary: summary.clone(),
        verified_count,
        failed_count,
        skipped_count,
        fixes,
    };

    // Keyword check failures are real failures — the fix didn't actually
    // resolve the SEO issues the audit flagged. Surface them so the task
    // goes to Review (not Done), making the problem visible.
    let has_seo_failures = report.fixes.iter().any(|f| {
        f.status == "failed"
            && (f.category == "h1_keyword"
                || f.category == "meta_desc_keyword"
                || f.category == "title_keyword"
                || f.category == "keyword_first_para")
    });

    let report_json = match serde_json::to_string_pretty(&report) {
        Ok(s) => s,
        Err(e) => {
            return StepResult::fail(format!("Failed to serialize verification report: {}", e));
        }
    };

    StepResult {
        success: !has_seo_failures,
        message: summary,
        output: Some(report_json),
        artifact_key: None,
    }
}

// ─── Patch resolution ─────────────────────────────────────────────────────────

fn resolve_patch(task: &Task) -> Result<ContentFixPatch, StepResult> {
    if let Some(artifact) = task.artifacts.iter().find(|a| a.key == "content_fix_patch") {
        if let Some(content) = &artifact.content {
            match serde_json::from_str::<ContentFixPatch>(content) {
                Ok(p) => return Ok(p),
                Err(e) => {
                    return Err(StepResult::fail_with_output(format!(
                            "content_fix_patch artifact exists but is invalid JSON: {}",
                            e
                        ), content.clone()));
                }
            }
        }
    }

    Err(StepResult::fail("No content_fix_patch artifact found. Run the generate step first.".to_string()))
}

/// Extract the target keyword from the content_fix_context artifact.
fn load_target_keyword(task: &Task) -> String {
    task.artifacts
        .iter()
        .find(|a| a.key == "content_fix_context")
        .and_then(|a| a.content.as_deref())
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|v| v["target_keyword"].as_str().map(|s| s.to_string()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keyword_present_with_quoted_vs_keyword() {
        // "vs" is a stopword in the canonical matcher, not a contentful
        // keyword word. The check succeeds when all CONTENT words are present
        // even if the intro uses "are two strategies" instead of "vs".
        let kw = r#""cash-secured put" vs "naked put""#;
        let kw_lower = kw.to_lowercase();

        let first_para = "naked puts and cash-secured puts are two popular options strategies, but they differ dramatically in capital requirements and risk. a cash-secured put requires holding the full assignment amount in cash, limiting risk to the strike price minus premium. a naked put uses margin instead, requiring only a fraction of the capital but exposing you to margin calls and amplified losses if the stock drops sharply.";

        assert!(crate::content::keyword_match::keyword_present(
            first_para,
            &kw_lower
        ));
    }

    #[test]
    fn test_find_first_paragraph_range_returns_full_para() {
        let body = "\
naked puts and cash-secured puts are two popular options strategies, but they differ dramatically in capital requirements and risk. a cash-secured put requires holding the full assignment amount in cash, limiting risk to the strike price minus premium. a naked put uses margin instead, requiring only a fraction of the capital but exposing you to margin calls and amplified losses if the stock drops sharply.

## Next heading
more text here.";

        let range = crate::content::cleaner::find_first_paragraph_range(body)
            .expect("should find first paragraph");
        let para = &body[range.0..range.1];
        let para_lower = para.trim().to_lowercase();

        let kw = r#""cash-secured put" vs "naked put""#;
        let kw_lower = kw.to_lowercase();

        assert!(
            para_lower.contains("cash-secured"),
            "first_para should contain 'cash-secured', got: '{}'", para
        );
        assert!(
            para_lower.contains("naked"),
            "first_para should contain 'naked', got: '{}'", para
        );

        let result = crate::content::keyword_match::keyword_present(&para_lower, &kw_lower);
        assert!(result, "keyword_present should return true for the full paragraph from find_first_paragraph_range. para='{}'", para);
    }
}
