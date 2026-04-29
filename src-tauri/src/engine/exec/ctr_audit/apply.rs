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
/// 4. Validate original can be read
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
            log::warn!(
                "[ctr_fix_apply] Failed to parse agent output as CtrFixPatch: {}",
                e
            );
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

    let validation_errors = validate_patch_before_write(&patch, _task, &original_content);
    if !validation_errors.is_empty() {
        return StepResult {
            success: false,
            message: format!(
                "Agent returned invalid CtrFixPatch values: {}. No changes written.",
                validation_errors.join("; ")
            ),
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
        snippet_patch,
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
        // Phase 1 safety net: do not overwrite existing frontmatter FAQ.
        // If the file already has valid frontmatter `faq:`, skip the FAQ patch
        // even if the agent generated one. This prevents quality regression
        // where rich existing FAQ gets replaced with generic generated answers.
        let file_has_frontmatter_faq =
            crate::engine::exec::audit_health::has_frontmatter_faq(&original_content);
        if file_has_frontmatter_faq {
            log::info!(
                "[ctr_fix_apply] Skipping FAQ patch for {} — file already has frontmatter FAQ. \
                 If the existing FAQ needs improvement, handle it manually.",
                patch.file
            );
            applied
                .push("faq_frontmatter (skipped — existing frontmatter FAQ preserved)".to_string());
        } else {
            let qa: Vec<(String, String)> = questions
                .into_iter()
                .map(|q| (q.question, q.answer))
                .collect();
            new_fm = crate::content::frontmatter::replace_faq_block(&new_fm, &qa);
            applied.push(format!("faq_frontmatter ({} questions)", qa.len()));
        }
    }

    if let Some(snippet) = snippet_patch {
        let list = snippet.ordered_list.as_deref();
        let table = snippet.comparison_table.as_deref();
        new_body = crate::content::cleaner::insert_snippet_section(
            &new_body,
            &snippet.heading,
            &snippet.answer_paragraph,
            list,
            table,
        );
        applied.push(format!(
            "snippet_patch ({})",
            format!("{:?}", snippet.format).to_lowercase()
        ));
    }

    // Short-circuit: agent returned a valid patch struct but with no actual changes.
    if applied.is_empty() {
        return StepResult {
            success: true,
            message: format!(
                "No CTR fixes to apply for {} — agent returned empty patch (no changes recommended)",
                patch.file
            ),
            output: Some(original_content),
        };
    }

    let new_content = crate::content::cleaner::rebuild_mdx(&new_fm, &new_body);

    // Validate before writing
    if let Err(e) = crate::content::cleaner::validate_mdx_structure(&new_content) {
        // Original is still on disk untouched — nothing to restore.
        return StepResult {
            success: false,
            message: format!(
                "File integrity failed after edit: {}. No changes written.",
                e
            ),
            output: None,
        };
    }

    if let Err(e) = std::fs::write(&file_path, &new_content) {
        return StepResult {
            success: false,
            message: format!("Failed to write file: {}. No changes written.", e),
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

fn validate_patch_before_write(
    patch: &CtrFixPatch,
    task: &Task,
    original_content: &str,
) -> Vec<String> {
    let mut errors = Vec::new();

    if let Some(title) = patch.changes.title.as_deref() {
        let title_len = title.chars().count();
        if title.trim().is_empty() {
            errors.push("title is empty".to_string());
        } else if title_len > crate::engine::exec::audit_health::TITLE_MAX_LEN {
            errors.push(format!(
                "title is {} chars, expected <= {}",
                title_len,
                crate::engine::exec::audit_health::TITLE_MAX_LEN
            ));
        }
    }

    if let Some(description) = patch.changes.description.as_deref() {
        let description_len = description.chars().count();
        if description_len < crate::engine::exec::audit_health::META_MIN_LEN
            || description_len > crate::engine::exec::audit_health::META_MAX_LEN
        {
            errors.push(format!(
                "description is {} chars, expected {}-{}",
                description_len,
                crate::engine::exec::audit_health::META_MIN_LEN,
                crate::engine::exec::audit_health::META_MAX_LEN
            ));
        }
    }

    if let Some(first_paragraph) = patch.changes.first_paragraph.as_deref() {
        let word_count = first_paragraph.split_whitespace().count();
        if word_count < crate::engine::exec::audit_health::SNIPPET_MIN_WORDS
            || word_count > crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
        {
            errors.push(format!(
                "first_paragraph is {} words, expected {}-{}",
                word_count,
                crate::engine::exec::audit_health::SNIPPET_MIN_WORDS,
                crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
            ));
        }

        if first_paragraph.contains("\n\n") {
            errors.push("first_paragraph contains blank lines".to_string());
        }

        if let Ok(Some(rec)) = extract_recommendation(task) {
            let keyword_lower = rec.target_keyword.to_lowercase();
            let has_kw_or_question = keyword_lower.is_empty()
                || first_paragraph.to_lowercase().contains(&keyword_lower)
                || first_paragraph.contains('?');
            if !has_kw_or_question {
                errors.push(format!(
                    "first_paragraph must contain target keyword '{}' or a question mark",
                    rec.target_keyword
                ));
            }
        }
    }

    if let Some(questions) = patch.changes.faq_questions.as_ref() {
        if !crate::engine::exec::audit_health::has_frontmatter_faq(original_content) {
            if questions.len() < 3 || questions.len() > 5 {
                errors.push(format!(
                    "faq_questions has {} questions, expected 3-5",
                    questions.len()
                ));
            }
            for (index, question) in questions.iter().enumerate() {
                if question.question.trim().is_empty() {
                    errors.push(format!("faq_questions[{}].question is empty", index));
                } else if !question.question.trim().ends_with('?') {
                    errors.push(format!(
                        "faq_questions[{}].question must end with '?'",
                        index
                    ));
                }
                if question.answer.trim().is_empty() {
                    errors.push(format!("faq_questions[{}].answer is empty", index));
                }
            }
        }
    }

    if let Some(snippet) = patch.changes.snippet_patch.as_ref() {
        let answer_word_count = snippet.answer_paragraph.split_whitespace().count();
        if answer_word_count < crate::engine::exec::audit_health::SNIPPET_MIN_WORDS
            || answer_word_count > crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
        {
            errors.push(format!(
                "snippet_patch.answer_paragraph is {} words, expected {}-{}",
                answer_word_count,
                crate::engine::exec::audit_health::SNIPPET_MIN_WORDS,
                crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
            ));
        }
        if snippet.heading.trim().is_empty() {
            errors.push("snippet_patch.heading is empty".to_string());
        }
        if snippet.answer_paragraph.contains("\n\n") {
            errors.push("snippet_patch.answer_paragraph contains blank lines".to_string());
        }
    }

    errors
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
pub(crate) fn exec_ctr_verify_fix(task: &Task, project_path: &str) -> StepResult {
    let rec = match extract_recommendation(task) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return StepResult {
                success: false,
                message: "No ctr_recommendations artifact found on task".to_string(),
                output: None,
            };
        }
        Err(e) => {
            return StepResult {
                success: false,
                message: format!(
                    "ctr_recommendations artifact exists but is invalid: {}. \
                     This usually means the agent returned an unexpected JSON shape (e.g. empty recommendations array wrapped in CtrAgentOutput instead of a single CtrRecommendation).",
                    e
                ),
                output: None,
            };
        }
    };

    let file_ref = &rec.file;
    if file_ref.is_empty() {
        return StepResult {
            success: false,
            message: "Recommendation has no file reference. Run sanitize_content to repair paths."
                .to_string(),
            output: None,
        };
    }

    let repo_root = Path::new(project_path);
    let full_path =
        match crate::engine::exec::audit_health::resolve_content_file(repo_root, &file_ref) {
            Some(p) => p,
            None => {
                return StepResult {
                    success: false,
                    message: format!(
                        "File not found: {}. Run sanitize_content to repair paths.",
                        file_ref
                    ),
                    output: None,
                };
            }
        };

    let target_keyword = &rec.target_keyword;

    let (title, meta, first_paragraph, _h1, has_faq, file_found) =
        crate::engine::exec::audit_health::read_article_excerpt(project_path, &file_ref);

    let file_content = std::fs::read_to_string(&full_path).unwrap_or_default();
    let body_content = crate::content::frontmatter::split_mdx(&file_content)
        .map(|(_, b)| b.to_string())
        .unwrap_or_default();

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
    let requested_fix_types: Vec<CtrFixType> =
        rec.fixes.iter().map(|f| f.fix_type.clone()).collect();

    for fix_type in &requested_fix_types {
        let (check_type, status, expected, actual, detail) = match fix_type {
            CtrFixType::TitleRewrite => {
                let expected = format!(
                    "≤ {} chars",
                    crate::engine::exec::audit_health::TITLE_MAX_LEN
                );
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
                let keyword_lower = target_keyword.to_lowercase();
                // Backward-compatible check: first paragraph
                let word_count = first_paragraph.split_whitespace().count();
                let has_kw_or_q = keyword_lower.is_empty()
                    || first_paragraph.to_lowercase().contains(&keyword_lower)
                    || first_paragraph.contains('?');
                let first_para_ok = word_count
                    >= crate::engine::exec::audit_health::SNIPPET_MIN_WORDS
                    && word_count <= crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
                    && has_kw_or_q;

                // New structured snippet check: H2 + answer + list/table
                let snippet_check = check_snippet_structure(&body_content, &keyword_lower);
                let has_good_answer = snippet_check.answer_word_count
                    >= crate::engine::exec::audit_health::SNIPPET_MIN_WORDS
                    && snippet_check.answer_word_count
                        <= crate::engine::exec::audit_health::SNIPPET_MAX_WORDS;
                let has_structure = snippet_check.has_h2
                    && (snippet_check.has_ordered_list
                        || snippet_check.has_table
                        || has_good_answer);

                let expected = format!(
                    "{}–{} words",
                    crate::engine::exec::audit_health::SNIPPET_MIN_WORDS,
                    crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
                );
                let actual = format!("{} words", word_count);

                if first_para_ok || has_structure {
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
                let actual = if has_faq {
                    "has FAQ schema"
                } else {
                    "no FAQ schema"
                }
                .to_string();
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
/// Returns Ok(Some(rec)) on success, Ok(None) if the artifact is missing,
/// Err(parse_error) if the artifact exists but cannot be parsed.
fn extract_recommendation(
    task: &Task,
) -> Result<Option<crate::models::ctr::CtrRecommendation>, String> {
    let artifact = task
        .artifacts
        .iter()
        .find(|a| a.key == "ctr_recommendations");

    let json = match artifact {
        Some(a) => match a.content.as_ref() {
            Some(c) => c,
            None => return Err("ctr_recommendations artifact has no content".to_string()),
        },
        None => return Ok(None),
    };

    match serde_json::from_str(json) {
        Ok(r) => Ok(Some(r)),
        Err(e) => Err(format!("JSON parse error: {}", e)),
    }
}

#[derive(Debug, Default)]
struct SnippetCheck {
    has_h2: bool,
    answer_word_count: usize,
    has_ordered_list: bool,
    has_table: bool,
}

/// Check if the body contains a snippet-friendly structure:
/// - H2 heading containing the target keyword or a question word
/// - A paragraph after the H2 with a word count
/// - Presence of ordered list or table near the top
fn check_snippet_structure(body: &str, keyword_lower: &str) -> SnippetCheck {
    let mut result = SnippetCheck::default();
    let lines: Vec<&str> = body.lines().collect();

    // Find first H2 that contains the keyword or a question word
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("## ") {
            let h2_text = trimmed.trim_start_matches("## ").trim().to_lowercase();
            let has_question = h2_text.ends_with('?')
                || h2_text.starts_with("what ")
                || h2_text.starts_with("how ")
                || h2_text.starts_with("why ")
                || h2_text.starts_with("when ")
                || h2_text.starts_with("where ");
            let has_keyword = !keyword_lower.is_empty() && h2_text.contains(keyword_lower);
            if has_question || has_keyword {
                result.has_h2 = true;
                // Count words in the next non-empty paragraph
                let mut word_count = 0usize;
                for j in (i + 1)..lines.len() {
                    let next = lines[j].trim();
                    if next.is_empty() || next.starts_with('#') {
                        break;
                    }
                    if !next.starts_with('|') && !next.starts_with("1.") && !next.starts_with("- ")
                    {
                        word_count += next.split_whitespace().count();
                    }
                }
                result.answer_word_count = word_count;
                break;
            }
        }
    }

    // Check for ordered list or table in the first 50 lines
    for line in lines.iter().take(50) {
        let trimmed = line.trim();
        if regex::Regex::new(r"^\d+\.\s").unwrap().is_match(trimmed) {
            result.has_ordered_list = true;
        }
        if trimmed.starts_with('|') && trimmed.len() > 2 {
            result.has_table = true;
        }
    }

    result
}
