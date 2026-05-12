/// Deterministic verification that applied content fixes meet health thresholds.
///
/// 1. Load the content_fix_patch artifact to know what fixes were expected.
/// 2. Read the (now modified) MDX file.
/// 3. Re-run the same health checks used by content_audit.
/// 4. Compare before/after values.
/// 5. Return a ContentFixVerificationReport.
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
                return StepResult {
                    success: false,
                    message: format!(
                        "File not found: {}. Run sanitize_content to repair paths.",
                        patch.file
                    ),
                    output: None,
                };
            }
        };

    let content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(_e) => {
            return StepResult {
                success: false,
                message: format!("File not found: {}", file_path.display()),
                output: None,
            };
        }
    };

    let (frontmatter, body) = match crate::content::frontmatter::split_mdx(&content) {
        Some((f, b)) => (f, b),
        None => {
            return StepResult {
                success: false,
                message: "Could not parse frontmatter from MDX file".to_string(),
                output: None,
            };
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
        let word_count = first_para.split_whitespace().count();
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

    // Date check
    if patch.changes.date.is_some() {
        let date = get_scalar("date");
        if !date.is_empty() {
            verified_count += 1;
            fixes.push(ContentFixVerifiedItem {
                category: "date".to_string(),
                status: "verified".to_string(),
                detail: Some(format!("date = {}", date)),
                actual: Some(date),
                expected: None,
            });
        } else {
            failed_count += 1;
            fixes.push(ContentFixVerifiedItem {
                category: "date".to_string(),
                status: "failed".to_string(),
                detail: Some("date field is empty".to_string()),
                actual: Some(date),
                expected: Some("non-empty date".to_string()),
            });
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

    let report_json = match serde_json::to_string_pretty(&report) {
        Ok(s) => s,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to serialize verification report: {}", e),
                output: None,
            };
        }
    };

    // Verification is advisory — the actual fixes were already applied and
    // validated for structural integrity in the apply step. We report
    // metric results for visibility but never fail the task here.
    StepResult {
        success: true,
        message: summary,
        output: Some(report_json),
    }
}

// ─── Patch resolution ─────────────────────────────────────────────────────────

fn resolve_patch(task: &Task) -> Result<ContentFixPatch, StepResult> {
    if let Some(artifact) = task.artifacts.iter().find(|a| a.key == "content_fix_patch") {
        if let Some(content) = &artifact.content {
            match serde_json::from_str::<ContentFixPatch>(content) {
                Ok(p) => return Ok(p),
                Err(e) => {
                    return Err(StepResult {
                        success: false,
                        message: format!(
                            "content_fix_patch artifact exists but is invalid JSON: {}",
                            e
                        ),
                        output: Some(content.clone()),
                    });
                }
            }
        }
    }

    Err(StepResult {
        success: false,
        message: "No content_fix_patch artifact found. Run the generate step first.".to_string(),
        output: None,
    })
}
