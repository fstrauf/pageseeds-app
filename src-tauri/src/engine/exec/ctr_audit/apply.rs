use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::models::ctr::{
    CtrFixCheckResult, CtrFixPatch, CtrFixPatchChanges, CtrFixType, CtrFixVerificationReport,
};
use crate::models::task::Task;

/// Deterministic application of agent-generated CTR fix patch.
///
/// 1. Parse CtrFixPatch from latest_raw (agent output)
/// 2. Resolve absolute file path from project_path + patch.file
/// 3. Read original file content
/// 4. Snapshot to {file}.backup
/// 5. parse_frontmatter → (fm, body)
/// 6. Apply changes deterministically
/// 7. rebuild_mdx → write file
/// 8. validate_mdx_structure → if fail, restore snapshot, return failed
/// 9. Return success with summary
#[allow(deprecated)]
pub(crate) fn exec_ctr_fix_apply(
    _task: &Task,
    project_path: &str,
    latest_raw: Option<&str>,
) -> StepResult {
    let raw = match latest_raw {
        Some(r) => r,
        None => {
            return StepResult {
                success: false,
                message: "Agent did not return valid CtrFixPatch JSON".to_string(),
                output: None,
            };
        }
    };

    // Extract JSON from agent output
    let json_str = match crate::engine::text::extract_json(raw) {
        Some(v) => match serde_json::to_string(&v) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[ctr_fix_apply] Failed to serialize extracted JSON: {}", e);
                return StepResult {
                    success: false,
                    message: format!("Agent did not return valid CtrFixPatch JSON: {}", e),
                    output: None,
                };
            }
        },
        None => {
            log::warn!("[ctr_fix_apply] No JSON found in agent output");
            return StepResult {
                success: false,
                message: "Agent did not return valid CtrFixPatch JSON — no JSON found".to_string(),
                output: None,
            };
        }
    };

    let patch: CtrFixPatch = match serde_json::from_str(&json_str) {
        Ok(p) => p,
        Err(e) => {
            log::warn!("[ctr_fix_apply] Failed to parse agent output as CtrFixPatch: {}", e);
            return StepResult {
                success: false,
                message: format!("Agent did not return valid CtrFixPatch JSON: {}", e),
                output: None,
            };
        }
    };

    if let Some(error) = patch.error {
        return StepResult {
            success: false,
            message: format!("Agent reported error: {}", error),
            output: None,
        };
    }

    let repo_root = Path::new(project_path);
    let file_path = match crate::engine::exec::audit_health::resolve_content_file(repo_root, &patch.file) {
        Some(p) => p,
        None => {
            return StepResult {
                success: false,
                message: format!("File not found: {}", patch.file),
                output: None,
            };
        }
    };

    let original_content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(_e) => {
            return StepResult {
                success: false,
                message: format!("File not found: {}", file_path.display()),
                output: None,
            };
        }
    };

    // Snapshot original
    let backup_path = file_path.with_extension("mdx.backup");
    if let Err(e) = std::fs::write(&backup_path, &original_content) {
        return StepResult {
            success: false,
            message: format!("Failed to write snapshot: {}", e),
            output: None,
        };
    }

    let (fm, body) = match crate::content::frontmatter::split_mdx(&original_content) {
        Some((f, b)) => (f.to_string(), b.to_string()),
        None => {
            return StepResult {
                success: false,
                message: "Could not parse frontmatter from MDX file".to_string(),
                output: None,
            };
        }
    };

    let CtrFixPatchChanges {
        title,
        description,
        first_paragraph,
        faq_questions,
    } = patch.changes;

    let mut new_fm = fm;
    let mut new_body = body;
    let mut applied = Vec::new();

    if let Some(new_title) = title {
        new_fm = crate::content::frontmatter::replace_scalar(&new_fm, "title", &new_title);
        applied.push("title".to_string());
    }

    if let Some(new_desc) = description {
        new_fm = crate::content::frontmatter::replace_scalar(&new_fm, "description", &new_desc);
        applied.push("description".to_string());
    }

    if let Some(new_para) = first_paragraph {
        new_body = crate::content::cleaner::replace_first_paragraph(&new_body, &new_para);
        applied.push("first_paragraph".to_string());
    }

    if let Some(questions) = faq_questions {
        let qa: Vec<(String, String)> = questions
            .into_iter()
            .map(|q| (q.question, q.answer))
            .collect();
        new_body = crate::content::cleaner::insert_faq_schema(&new_body, &qa);
        applied.push(format!("faq_schema ({} questions)", qa.len()));
    }

    let new_content = crate::content::cleaner::rebuild_mdx(&new_fm, &new_body);

    // Validate before writing
    if let Err(e) = crate::content::cleaner::validate_mdx_structure(&new_content) {
        // Restore from snapshot
        let _ = std::fs::write(&file_path, &original_content);
        let _ = std::fs::remove_file(&backup_path);
        return StepResult {
            success: false,
            message: format!("File integrity failed after edit: {}. Original restored.", e),
            output: None,
        };
    }

    if let Err(e) = std::fs::write(&file_path, &new_content) {
        let _ = std::fs::write(&file_path, &original_content);
        let _ = std::fs::remove_file(&backup_path);
        return StepResult {
            success: false,
            message: format!("Failed to write file: {}. Original restored.", e),
            output: None,
        };
    }

    StepResult {
        success: true,
        message: format!(
            "Applied CTR fixes to {}: {}",
            patch.file,
            applied.join(", ")
        ),
        output: Some(new_content),
    }
}

/// Deterministic verification that applied CTR fixes meet health thresholds.
///
/// 1. Extract article info from task artifacts
/// 2. Read the CURRENT file from disk (post-apply)
/// 3. Run read_article_excerpt
/// 4. Run check_article_health with the SAME thresholds as the audit
/// 5. Compare against the fixes that were requested
/// 6. Build CtrFixVerificationReport
/// 7. If ALL requested fixes pass → success, status done
///    If SOME fail → success=false (soft), message includes per-fix detail, status review
///    If file missing → failed
pub(crate) fn exec_ctr_verify_fix(
    task: &Task,
    project_path: &str,
) -> StepResult {
    let rec = match extract_recommendation(task) {
        Some(r) => r,
        None => {
            return StepResult {
                success: false,
                message: "No ctr_recommendations artifact found on task".to_string(),
                output: None,
            };
        }
    };

    let file_ref = rec.file.unwrap_or_default();
    if file_ref.is_empty() {
        return StepResult {
            success: false,
            message: "Recommendation has no file reference".to_string(),
            output: None,
        };
    }

    let repo_root = Path::new(project_path);
    let _full_path = match crate::engine::exec::audit_health::resolve_content_file(repo_root, &file_ref) {
        Some(p) => p,
        None => {
            return StepResult {
                success: false,
                message: format!("File not found: {}", file_ref),
                output: None,
            };
        }
    };

    let target_keyword = rec.target_keyword.unwrap_or_default();

    let (title, meta, first_paragraph, _h1, has_faq, file_found) =
        crate::engine::exec::audit_health::read_article_excerpt(project_path, &file_ref);

    let _health = crate::engine::exec::audit_health::check_article_health(
        &title,
        &meta,
        &first_paragraph,
        &target_keyword,
        has_faq,
        file_found,
    );

    let mut checks = Vec::new();
    let mut all_pass = true;

    // Determine which fixes were requested and verify each
    let requested_fix_types: Vec<CtrFixType> = rec.fixes.iter().map(|f| f.fix_type.clone()).collect();

    for fix_type in &requested_fix_types {
        let (check_type, status, expected, actual, detail) = match fix_type {
            CtrFixType::TitleRewrite => {
                let expected = format!("≤ {} chars", crate::engine::exec::audit_health::TITLE_MAX_LEN);
                let actual = format!("{} chars", title.len());
                if title.len() <= crate::engine::exec::audit_health::TITLE_MAX_LEN {
                    ("title", "pass", expected, actual, None)
                } else {
                    (
                        "title",
                        "fail",
                        expected,
                        actual,
                        Some(format!(
                            "title is {} chars, expected ≤ {}",
                            title.len(),
                            crate::engine::exec::audit_health::TITLE_MAX_LEN
                        )),
                    )
                }
            }
            CtrFixType::MetaDescription => {
                let expected = format!(
                    "{}–{} chars",
                    crate::engine::exec::audit_health::META_MIN_LEN,
                    crate::engine::exec::audit_health::META_MAX_LEN
                );
                let actual = format!("{} chars", meta.len());
                if meta.len() >= crate::engine::exec::audit_health::META_MIN_LEN
                    && meta.len() <= crate::engine::exec::audit_health::META_MAX_LEN
                {
                    ("description", "pass", expected, actual, None)
                } else {
                    (
                        "description",
                        "fail",
                        expected,
                        actual,
                        Some(format!(
                            "meta is {} chars, expected {}–{}",
                            meta.len(),
                            crate::engine::exec::audit_health::META_MIN_LEN,
                            crate::engine::exec::audit_health::META_MAX_LEN
                        )),
                    )
                }
            }
            CtrFixType::SnippetBait => {
                let word_count = first_paragraph.split_whitespace().count();
                let expected = format!(
                    "{}–{} words",
                    crate::engine::exec::audit_health::SNIPPET_MIN_WORDS,
                    crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
                );
                let actual = format!("{} words", word_count);
                let keyword_lower = target_keyword.to_lowercase();
                let has_kw_or_q = keyword_lower.is_empty()
                    || first_paragraph.to_lowercase().contains(&keyword_lower)
                    || first_paragraph.contains('?');
                if word_count >= crate::engine::exec::audit_health::SNIPPET_MIN_WORDS
                    && word_count <= crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
                    && has_kw_or_q
                {
                    ("snippet", "pass", expected, actual, None)
                } else {
                    let detail = if !has_kw_or_q {
                        Some(format!(
                            "first paragraph is {} words, expected {}–{}, and missing keyword or question mark",
                            word_count,
                            crate::engine::exec::audit_health::SNIPPET_MIN_WORDS,
                            crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
                        ))
                    } else {
                        Some(format!(
                            "first paragraph is {} words, expected {}–{}",
                            word_count,
                            crate::engine::exec::audit_health::SNIPPET_MIN_WORDS,
                            crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
                        ))
                    };
                    ("snippet", "fail", expected, actual, detail)
                }
            }
            CtrFixType::FaqSchema => {
                let expected = "has FAQ schema".to_string();
                let actual = if has_faq { "has FAQ schema" } else { "no FAQ schema" }.to_string();
                if has_faq {
                    ("faq", "pass", expected, actual, None)
                } else {
                    (
                        "faq",
                        "fail",
                        expected,
                        actual,
                        Some("FAQ schema is missing".to_string()),
                    )
                }
            }
        };

        if status == "fail" {
            all_pass = false;
        }

        checks.push(CtrFixCheckResult {
            check_type: check_type.to_string(),
            status: status.to_string(),
            expected,
            actual,
            detail,
        });
    }

    let overall_status = if all_pass { "verified" } else { "partial" };

    let report = CtrFixVerificationReport {
        article_id: rec.article_id,
        file: file_ref.clone(),
        overall_status: overall_status.to_string(),
        checks,
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

    if all_pass {
        StepResult {
            success: true,
            message: format!("CTR verification passed for {}", file_ref),
            output: Some(report_json),
        }
    } else {
        let fail_details: Vec<String> = report
            .checks
            .iter()
            .filter(|c| c.status == "fail")
            .filter_map(|c| c.detail.clone())
            .collect();
        StepResult {
            success: false,
            message: format!(
                "CTR verification found issues for {}: {}",
                file_ref,
                fail_details.join("; ")
            ),
            output: Some(report_json),
        }
    }
}

/// Extract the single CtrRecommendation from the task's ctr_recommendations artifact.
fn extract_recommendation(task: &Task) -> Option<crate::models::ctr::CtrRecommendation> {
    let json = task
        .artifacts
        .iter()
        .find(|a| a.key == "ctr_recommendations")
        .and_then(|a| a.content.as_ref())?;

    serde_json::from_str(json).ok()
}
