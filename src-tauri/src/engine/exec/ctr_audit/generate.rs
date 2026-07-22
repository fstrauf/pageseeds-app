/// Typed CTR fix patch generation.
///
/// 1. Load CtrRecommendation from task artifacts.
/// 2. Read the target MDX file.
/// 3. Prefer a deterministic `CtrFixPatch` when analyze already provided write-ready
///    recommended strings (validated via normalize + write + consistency checks).
/// 4. Otherwise build a focused prompt and call Rig structured extraction.
/// 5. Normalize and validate the patch; one repair extraction if needed.
/// 6. Return the typed patch JSON as step output.
use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::models::ctr::{CtrFixPatch, CtrRecommendation};
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};
use crate::rig::provider::LlmBackend;

pub(crate) async fn exec_ctr_fix_generate(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
    // Deterministic path first — no LLM backend required.
    let (rec, original_content) = match load_recommendation_and_content(task, project_path) {
        Ok(v) => v,
        Err(result) => return result,
    };

    if let Some(result) = try_deterministic_generate_result(task, &rec, &original_content) {
        return result;
    }

    let backend =
        match crate::rig::provider::resolve_backend(agent_provider, None, None, None).await {
            Ok(b) => b,
            Err(e) => {
                return StepResult::fail(format!("Could not resolve LLM backend: {}", e));
            }
        };

    match &backend {
        LlmBackend::KimiDirect => {
            return StepResult::fail("Structured extraction is not supported with KimiDirect (CLI fallback). \
                 Please ensure the Kimi bridge is running or use another provider."
                    .to_string());
        }
        _ => {}
    }

    exec_ctr_fix_generate_llm(task, project_path, &backend, &rec, &original_content).await
}

/// Core logic, testable with a mocked backend.
///
/// Tries the deterministic recommendation path first (works even with unreachable
/// backends). Falls back to LLM structured extraction only when recommendations
/// are not write-ready.
pub(crate) async fn exec_ctr_fix_generate_with_backend(
    task: &Task,
    project_path: &str,
    backend: &LlmBackend,
) -> StepResult {
    let (rec, original_content) = match load_recommendation_and_content(task, project_path) {
        Ok(v) => v,
        Err(result) => return result,
    };

    if let Some(result) = try_deterministic_generate_result(task, &rec, &original_content) {
        return result;
    }

    exec_ctr_fix_generate_llm(task, project_path, backend, &rec, &original_content).await
}

fn load_recommendation_and_content(
    task: &Task,
    project_path: &str,
) -> Result<(CtrRecommendation, String), StepResult> {
    let rec = match super::extract_recommendation(task) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return Err(StepResult::fail(
                "No ctr_recommendations artifact found on task".to_string(),
            ));
        }
        Err(e) => {
            return Err(StepResult::fail(format!(
                "ctr_recommendations artifact exists but is invalid: {}. \
                 This usually means the agent returned an unexpected JSON shape.",
                e
            )));
        }
    };

    let repo_root = Path::new(project_path);
    let file_path =
        match crate::engine::exec::audit_health::resolve_content_file(repo_root, &rec.file) {
            Some(p) => p,
            None => {
                return Err(StepResult::fail(format!(
                    "File not found: {}. Run sanitize_content to repair paths.",
                    rec.file
                )));
            }
        };

    let original_content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(_e) => {
            return Err(StepResult::fail(format!(
                "File not found: {}",
                file_path.display()
            )));
        }
    };

    Ok((rec, original_content))
}

fn try_deterministic_generate_result(
    task: &Task,
    rec: &CtrRecommendation,
    original_content: &str,
) -> Option<StepResult> {
    let patch = super::try_patch_from_recommendation(rec, original_content, task)?;

    let patch_json = match serde_json::to_string_pretty(&patch) {
        Ok(s) => s,
        Err(e) => {
            return Some(StepResult::fail(format!(
                "Failed to serialize CtrFixPatch: {}",
                e
            )));
        }
    };

    Some(StepResult {
        success: true,
        message: format!(
            "Generated typed CtrFixPatch for {} (from recommendation, no LLM)",
            rec.file
        ),
        output: Some(patch_json),
        artifact_key: None,
    })
}

/// LLM structured-extraction path after deterministic short-circuit failed.
async fn exec_ctr_fix_generate_llm(
    task: &Task,
    project_path: &str,
    backend: &LlmBackend,
    rec: &CtrRecommendation,
    original_content: &str,
) -> StepResult {
    let prompt = match build_ctr_fix_prompt(task, project_path, rec, original_content) {
        Ok(p) => p,
        Err(e) => {
            return StepResult::fail(format!("Failed to build CTR fix prompt: {}", e));
        }
    };

    let mut patch = match crate::rig::extraction::extract_with_backend::<CtrFixPatch>(
        backend,
        &prompt,
        Some(
            "You are a CTR optimization assistant. \
             Return only a valid CtrFixPatch by calling the submit tool. \
             Every requested fix must be represented in the patch unless the current file already satisfies the requirement.",
        ),
        Some("acp"),
        None,
    )
    .await
    {
        Ok(p) => p,
        Err(e) => {
            return StepResult::fail(format!(
                    "Structured extraction failed for CtrFixPatch: {}. \
                     If you are using KimiDirect, please switch to a structured-output provider (Kimi bridge, Claude, OpenAI, or Ollama).",
                    e
                ));
        }
    };

    let mut repairs = super::normalize_patch_before_validation(&mut patch, task);
    let mut errors = super::validate_patch_before_write(&patch, task, original_content);
    if !errors.is_empty() {
        let pruned = super::prune_invalid_change_fields(&mut patch, &errors);
        if !pruned.is_empty() {
            repairs.extend(pruned);
            errors = super::validate_patch_before_write(&patch, task, original_content);
        }
    }

    let consistency_errors =
        super::validate_patch_against_recommendation(&patch, rec, original_content);
    errors.extend(consistency_errors);

    if !errors.is_empty() {
        log::info!(
            "[ctr_fix_generate] First patch invalid for {}: {}. Attempting repair.",
            rec.file,
            errors.join("; ")
        );

        match repair_ctr_fix_patch_with_backend(backend, &prompt, &patch, &errors).await {
            Ok(mut repaired) => {
                let mut repair_notes =
                    super::normalize_patch_before_validation(&mut repaired, task);
                let mut repair_errors =
                    super::validate_patch_before_write(&repaired, task, original_content);
                if !repair_errors.is_empty() {
                    let pruned = super::prune_invalid_change_fields(&mut repaired, &repair_errors);
                    if !pruned.is_empty() {
                        repair_notes.extend(pruned);
                        repair_errors =
                            super::validate_patch_before_write(&repaired, task, original_content);
                    }
                }
                let consistency_errors2 =
                    super::validate_patch_against_recommendation(&repaired, rec, original_content);
                repair_errors.extend(consistency_errors2);

                if !repair_errors.is_empty() {
                    return StepResult::fail(format!(
                            "CTR fix patch failed validation after repair: {}. No changes written.",
                            repair_errors.join("; ")
                        ));
                }
                repairs.extend(repair_notes);
                patch = repaired;
            }
            Err(e) => {
                return StepResult::fail(format!(
                        "CTR fix patch repair extraction failed: {}. No changes written.",
                        e
                    ));
            }
        }
    }

    let patch_json = match serde_json::to_string_pretty(&patch) {
        Ok(s) => s,
        Err(e) => {
            return StepResult::fail(format!("Failed to serialize CtrFixPatch: {}", e));
        }
    };

    let repair_msg = if repairs.is_empty() {
        String::new()
    } else {
        format!(" (normalized: {})", repairs.join(", "))
    };

    StepResult {
        success: true,
        message: format!("Generated typed CtrFixPatch for {}{}", rec.file, repair_msg),
        output: Some(patch_json),
        artifact_key: None,
    }
}

// ─── Prompt builder ───────────────────────────────────────────────────────────

pub(crate) fn build_ctr_fix_prompt(
    _task: &Task,
    project_path: &str,
    rec: &CtrRecommendation,
    original_content: &str,
) -> Result<String, String> {
    let repo_root = Path::new(project_path);

    // Load skill if available
    let skill_content = crate::engine::skills::load_skill(repo_root, "ctr-fix-apply")
        .map(|s| s.content)
        .unwrap_or_else(|| "Apply CTR fixes to improve title, meta description, first paragraph, FAQ schema, and snippet bait.".to_string());

    // Parse current excerpt
    let (current_title, current_meta, current_first) =
        super::patch::parse_content_excerpt(original_content);
    let has_faq = crate::engine::exec::audit_health::has_frontmatter_faq(original_content);

    // Build body excerpt (first ~3_000 chars of body, skipping frontmatter)
    const BODY_EXCERPT_CHARS: usize = 3_000;
    let body_excerpt = crate::content::frontmatter::split_mdx(original_content)
        .map(|(_, b)| {
            let truncated: String = b.chars().take(BODY_EXCERPT_CHARS).collect();
            if b.len() > BODY_EXCERPT_CHARS {
                format!("{}...", truncated)
            } else {
                truncated
            }
        })
        .unwrap_or_else(|| "(could not parse body)".to_string());

    let title_max = crate::engine::exec::audit_health::TITLE_MAX_LEN;
    let meta_min = crate::engine::exec::audit_health::META_MIN_LEN;
    let meta_max = crate::engine::exec::audit_health::META_MAX_LEN;
    let snippet_min = crate::engine::exec::audit_health::SNIPPET_MIN_WORDS;
    let snippet_max = crate::engine::exec::audit_health::SNIPPET_MAX_WORDS;

    let prompt = format!(
        r#"## Skill

{skill_content}

## Current File State

- file: {file}
- article_id: {article_id}
- target_keyword: {target_keyword}

### Current title
```{current_title}```
(title is {title_len} chars; max allowed: {title_max})

### Current meta description
```{current_meta}```
(meta is {meta_len} chars; allowed range: {meta_min}-{meta_max})

### Current first paragraph
```{current_first}```
(first paragraph is {first_words} words; allowed range: {snippet_min}-{snippet_max})

### Has frontmatter FAQ
{has_faq}

### Body excerpt
```
{body_excerpt}
```

## CTR Recommendation

```json
{rec_json}
```

## Instructions

You must produce a CtrFixPatch JSON that addresses every fix listed in the recommendation above, **unless the current file state already satisfies the requirement**.

Validation rules (enforced by Rust):
- title: must be ≤ {title_max} chars if provided
- description: must be {meta_min}-{meta_max} chars if provided
- first_paragraph: must be {snippet_min}-{snippet_max} words and contain the target keyword or a question mark
- faq_questions: must be 3-5 questions if provided and file has no existing frontmatter FAQ
- snippet_patch.answer_paragraph: must be {snippet_min}-{snippet_max} words

Only include fields that need to change. Do not include title/description/first_paragraph changes if those fixes were not requested.
"#,
        skill_content = skill_content.trim(),
        file = rec.file,
        article_id = rec.article_id,
        target_keyword = rec.target_keyword,
        current_title = current_title,
        title_len = current_title.chars().count(),
        title_max = title_max,
        current_meta = current_meta,
        meta_len = current_meta.chars().count(),
        meta_min = meta_min,
        meta_max = meta_max,
        current_first = current_first,
        first_words = crate::content::ops::count_words(&current_first),
        snippet_min = snippet_min,
        snippet_max = snippet_max,
        has_faq = if has_faq {
            "yes — do NOT generate faq_questions"
        } else {
            "no — generate faq_questions if requested"
        },
        body_excerpt = body_excerpt.trim(),
        rec_json = serde_json::to_string_pretty(rec).map_err(|e| e.to_string())?,
    );

    Ok(prompt)
}

// ─── Repair prompt ────────────────────────────────────────────────────────────

async fn repair_ctr_fix_patch_with_backend(
    backend: &LlmBackend,
    original_prompt: &str,
    invalid_patch: &CtrFixPatch,
    errors: &[String],
) -> Result<CtrFixPatch, String> {
    let repair_prompt = format!(
        r#"The following CtrFixPatch failed validation.

## Original Prompt

{original_prompt}

## Invalid Patch

```json
{patch_json}
```

## Validation Errors

{errors}

## Instructions

Fix the patch so it passes all validation rules. Return only the corrected CtrFixPatch by calling the submit tool. Address every error listed above."#,
        original_prompt = original_prompt,
        patch_json = serde_json::to_string_pretty(invalid_patch).map_err(|e| e.to_string())?,
        errors = errors
            .iter()
            .map(|e| format!("- {}", e))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    crate::rig::extraction::extract_with_backend::<CtrFixPatch>(
        backend,
        &repair_prompt,
        Some(
         "You are correcting a previously invalid CtrFixPatch. \
              Return only the corrected CtrFixPatch by calling the submit tool.",
        ),
        Some("acp"),
        None,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_ctr_fix_prompt_contains_thresholds() {
        let mdx = r#"---
title: "Old Title"
description: "Old description that is definitely long enough to pass the meta check because it has many words."
date: "2024-01-01"
---

# Old Title

One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight twentynine thirty thirtyone thirtytwo thirtythree thirtyfour thirtyfive thirtysix thirtyseven thirtyeight thirtynine forty.
"#;

        let rec = CtrRecommendation {
            article_id: 1,
            url_slug: "test-article".to_string(),
            file: "content/test.mdx".to_string(),
            priority: None,
            expected_ctr_improvement: None,
            target_keyword: "test keyword".to_string(),
            fixes: vec![],
        };

        let task = crate::models::task::Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let prompt = build_ctr_fix_prompt(&task, "/tmp/fake", &rec, mdx).unwrap();

        // Should contain current thresholds, not hard-coded magic numbers
        assert!(
            prompt.contains(&format!(
                "max allowed: {}",
                crate::engine::exec::audit_health::TITLE_MAX_LEN
            )),
            "prompt should include TITLE_MAX_LEN"
        );
        assert!(
            prompt.contains(&format!(
                "allowed range: {}-{}",
                crate::engine::exec::audit_health::META_MIN_LEN,
                crate::engine::exec::audit_health::META_MAX_LEN
            )),
            "prompt should include META range"
        );
        assert!(
            prompt.contains(&format!(
                "allowed range: {}-{}",
                crate::engine::exec::audit_health::SNIPPET_MIN_WORDS,
                crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
            )),
            "prompt should include snippet word range"
        );
        assert!(
            prompt.contains("test keyword"),
            "prompt should include target keyword"
        );
        assert!(
            prompt.contains("content/test.mdx"),
            "prompt should include file path"
        );
    }

    #[tokio::test]
    async fn exec_ctr_fix_generate_deterministic_no_llm() {
        // Concrete write-ready recommendations + unreachable backend must succeed
        // without any network call (proves no LLM path).
        let backend = crate::rig::provider::LlmBackend::KimiBridge {
            base_url: "http://127.0.0.1:1/v1".to_string(),
            model: "unreachable".to_string(),
        };

        let path = std::env::temp_dir()
            .join(format!("ctr_gen_deterministic_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&path);
        let content_dir = std::path::Path::new(&path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        let mdx = r#"---
title: "Test Article | Brand | Brand -- Tagline That Is Too Long For SERP"
description: "A short desc"
date: "2024-01-01"
---

# Test Article

This is the first paragraph of the test article. It contains some content.
"#;
        std::fs::write(content_dir.join("001_test_article.mdx"), mdx).unwrap();

        let title = "Best CSP Stocks Guide";
        let meta = "Discover the best cash-secured put stocks for consistent income. \
Compare risk, premiums, and entry strategies in this practical guide for option sellers.";
        assert!(
            (crate::engine::exec::audit_health::META_MIN_LEN
                ..=crate::engine::exec::audit_health::META_MAX_LEN)
                .contains(&meta.chars().count())
        );

        let rec = CtrRecommendation {
            article_id: 1,
            url_slug: "test-article".to_string(),
            file: "content/001_test_article.mdx".to_string(),
            priority: None,
            expected_ctr_improvement: None,
            target_keyword: "csp stocks".to_string(),
            fixes: vec![
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::TitleRewrite,
                    current: Some("long title".to_string()),
                    recommended: serde_json::json!(title),
                    reason: None,
                },
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::MetaDescription,
                    current: Some("A short desc".to_string()),
                    recommended: serde_json::json!(meta),
                    reason: None,
                },
            ],
        };

        let task = crate::models::task::Task {
            id: "task-deterministic".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_recommendations".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_audit".to_string()),
                content: Some(serde_json::to_string(&rec).unwrap()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_generate_with_backend(&task, &path, &backend).await;
        assert!(result.success, "Deterministic generate failed: {}", result.message);
        assert!(
            result.message.contains("from recommendation, no LLM"),
            "Expected deterministic path message, got: {}",
            result.message
        );

        let patch: CtrFixPatch = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(patch.article_id, 1);
        assert_eq!(patch.file, "content/001_test_article.mdx");
        assert_eq!(patch.changes.title.as_deref(), Some(title));
        assert_eq!(patch.changes.description.as_deref(), Some(meta));

        let _ = std::fs::remove_dir_all(&path);
    }

    #[tokio::test]
    async fn exec_ctr_fix_generate_no_recommendation() {
        let task = crate::models::task::Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let backend = crate::rig::provider::LlmBackend::KimiBridge {
            base_url: "http://localhost:9999/v1".to_string(),
            model: "test".to_string(),
        };

        let result = exec_ctr_fix_generate_with_backend(&task, "/tmp/fake", &backend).await;
        assert!(!result.success);
        assert!(
            result.message.contains("No ctr_recommendations"),
            "Expected missing recommendation error, got: {}",
            result.message
        );
    }

    #[tokio::test]
    async fn exec_ctr_fix_generate_success_mocked() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        let patch_json = r#"{"article_id":1,"file":"content/001_test_article.mdx","changes":{"title":"Good Title","description":"This is a very good meta description that is definitely longer than one hundred and thirty characters so it passes the strict health check.","first_paragraph":"One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight twentynine thirty thirtyone thirtytwo thirtythree thirtyfour thirtyfive thirtysix thirtyseven thirtyeight thirtynine forty test article."}}"#;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "created": 1677652288,
                "model": "test-model",
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": patch_json
                        },
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 25,
                    "completion_tokens": 15,
                    "total_tokens": 40
                }
            })))
            .mount(&mock_server)
            .await;

        let backend = crate::rig::provider::LlmBackend::KimiBridge {
            base_url: format!("{}/v1", mock_server.uri()),
            model: "test-model".to_string(),
        };

        let path = std::env::temp_dir()
            .join(format!("ctr_gen_test_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&path);
        let content_dir = std::path::Path::new(&path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        let mdx = r#"---
title: "Test Article | Brand | Brand -- Tagline"
description: "A short desc"
date: "2024-01-01"
---

# Test Article | Brand | Brand -- Tagline

This is the first paragraph of the test article. It contains some content.

## Section 1

More content here.
"#;
        std::fs::write(content_dir.join("001_test_article.mdx"), mdx).unwrap();

        let rec = CtrRecommendation {
            article_id: 1,
            url_slug: "test-article".to_string(),
            file: "content/001_test_article.mdx".to_string(),
            priority: None,
            expected_ctr_improvement: None,
            target_keyword: "test article".to_string(),
            fixes: vec![
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::TitleRewrite,
                    current: Some("Test Article | Brand | Brand -- Tagline".to_string()),
                    recommended: serde_json::json!("Good Title"),
                    reason: None,
                },
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::MetaDescription,
                    current: Some("A short desc".to_string()),
                    recommended: serde_json::json!("This is a very good meta description that is definitely longer than one hundred and thirty characters so it passes the strict health check."),
                    reason: None,
                },
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::SnippetBait,
                    current: Some("This is the first paragraph...".to_string()),
                    recommended: serde_json::json!("One two three..."),
                    reason: None,
                },
            ],
        };

        let task = crate::models::task::Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_recommendations".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_audit".to_string()),
                content: Some(serde_json::to_string(&rec).unwrap()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_generate_with_backend(&task, &path, &backend).await;
        assert!(result.success, "Generate failed: {}", result.message);

        let patch: CtrFixPatch = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(patch.article_id, 1);
        assert_eq!(patch.file, "content/001_test_article.mdx");
        assert!(patch.changes.title.is_some());

        let _ = std::fs::remove_dir_all(&path);
    }

    #[tokio::test]
    async fn exec_ctr_fix_generate_repair_success_mocked() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // First response: invalid patch (description too short — normalization cannot fix this)
        let bad_patch_json = r#"{"article_id":1,"file":"content/001_test_article.mdx","changes":{"title":"Good Title","description":"Too short","first_paragraph":"One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight twentynine thirty thirtyone thirtytwo thirtythree thirtyfour thirtyfive thirtysix thirtyseven thirtyeight thirtynine forty test article."}}"#;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "created": 1677652288,
                "model": "test-model",
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": bad_patch_json
                        },
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 25,
                    "completion_tokens": 15,
                    "total_tokens": 40
                }
            })))
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        // Second response: valid repair
        let good_patch_json = r#"{"article_id":1,"file":"content/001_test_article.mdx","changes":{"title":"Good Title","description":"This is a very good meta description that is definitely longer than one hundred and thirty characters so it passes the strict health check.","first_paragraph":"One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight twentynine thirty thirtyone thirtytwo thirtythree thirtyfour thirtyfive thirtysix thirtyseven thirtyeight thirtynine forty test article."}}"#;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl-test2",
                "object": "chat.completion",
                "created": 1677652289,
                "model": "test-model",
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": good_patch_json
                        },
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 30,
                    "completion_tokens": 20,
                    "total_tokens": 50
                }
            })))
            .mount(&mock_server)
            .await;

        let backend = crate::rig::provider::LlmBackend::KimiBridge {
            base_url: format!("{}/v1", mock_server.uri()),
            model: "test-model".to_string(),
        };

        let path = std::env::temp_dir()
            .join(format!("ctr_gen_repair_test_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&path);
        let content_dir = std::path::Path::new(&path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        let mdx = r#"---
title: "Test Article | Brand | Brand -- Tagline"
description: "A short desc"
date: "2024-01-01"
---

# Test Article | Brand | Brand -- Tagline

This is the first paragraph of the test article. It contains some content.

## Section 1

More content here.
"#;
        std::fs::write(content_dir.join("001_test_article.mdx"), mdx).unwrap();

        let rec = CtrRecommendation {
            article_id: 1,
            url_slug: "test-article".to_string(),
            file: "content/001_test_article.mdx".to_string(),
            priority: None,
            expected_ctr_improvement: None,
            target_keyword: "test article".to_string(),
            fixes: vec![
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::TitleRewrite,
                    current: Some("Test Article | Brand | Brand -- Tagline".to_string()),
                    recommended: serde_json::json!("Good Title"),
                    reason: None,
                },
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::MetaDescription,
                    current: Some("A short desc".to_string()),
                    recommended: serde_json::json!("This is a very good meta description that is definitely longer than one hundred and thirty characters so it passes the strict health check."),
                    reason: None,
                },
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::SnippetBait,
                    current: Some("This is the first paragraph...".to_string()),
                    recommended: serde_json::json!("One two three..."),
                    reason: None,
                },
            ],
        };

        let task = crate::models::task::Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_recommendations".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_audit".to_string()),
                content: Some(serde_json::to_string(&rec).unwrap()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_generate_with_backend(&task, &path, &backend).await;
        assert!(
            result.success,
            "Generate with repair failed: {}",
            result.message
        );
        assert!(
            result.message.contains("repair")
                || result.message.contains("normalized")
                || result.message.contains("Generated typed"),
            "Expected success after repair, got: {}",
            result.message
        );

        let patch: CtrFixPatch = serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(patch.changes.title.as_deref().unwrap(), "Good Title");

        let _ = std::fs::remove_dir_all(&path);
    }

    #[tokio::test]
    async fn exec_ctr_fix_generate_repair_fails_mocked() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // First response: invalid patch (description too short)
        let bad_patch_json_1 = r#"{"article_id":1,"file":"content/001_test_article.mdx","changes":{"title":"Good Title","description":"Too short","first_paragraph":"One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight twentynine thirty thirtyone thirtytwo thirtythree thirtyfour thirtyfive thirtysix thirtyseven thirtyeight thirtynine forty test article."}}"#;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl-test",
                "object": "chat.completion",
                "created": 1677652288,
                "model": "test-model",
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": bad_patch_json_1
                        },
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 25,
                    "completion_tokens": 15,
                    "total_tokens": 40
                }
            })))
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        // Second response: STILL invalid (description still too short)
        let bad_patch_json_2 = r#"{"article_id":1,"file":"content/001_test_article.mdx","changes":{"title":"Good Title","description":"Still bad","first_paragraph":"One two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone twentytwo twentythree twentyfour twentyfive twentysix twentyseven twentyeight twentynine thirty thirtyone thirtytwo thirtythree thirtyfour thirtyfive thirtysix thirtyseven thirtyeight thirtynine forty test article."}}"#;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "chatcmpl-test2",
                "object": "chat.completion",
                "created": 1677652289,
                "model": "test-model",
                "choices": [
                    {
                        "index": 0,
                        "message": {
                            "role": "assistant",
                            "content": bad_patch_json_2
                        },
                        "finish_reason": "stop"
                    }
                ],
                "usage": {
                    "prompt_tokens": 30,
                    "completion_tokens": 20,
                    "total_tokens": 50
                }
            })))
            .mount(&mock_server)
            .await;

        let backend = crate::rig::provider::LlmBackend::KimiBridge {
            base_url: format!("{}/v1", mock_server.uri()),
            model: "test-model".to_string(),
        };

        let path = std::env::temp_dir()
            .join(format!("ctr_gen_repair_fail_test_{}", std::process::id()))
            .to_string_lossy()
            .to_string();
        let _ = std::fs::remove_dir_all(&path);
        let content_dir = std::path::Path::new(&path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        let mdx = r#"---
title: "Test Article | Brand | Brand -- Tagline"
description: "A short desc"
date: "2024-01-01"
---

# Test Article | Brand | Brand -- Tagline

This is the first paragraph of the test article. It contains some content.

## Section 1

More content here.
"#;
        std::fs::write(content_dir.join("001_test_article.mdx"), mdx).unwrap();

        // Recommended values are intentionally not write-ready (short meta, short
        // snippet, overlong title) so the deterministic map cannot short-circuit
        // and the LLM → repair path is actually exercised.
        let rec = CtrRecommendation {
            article_id: 1,
            url_slug: "test-article".to_string(),
            file: "content/001_test_article.mdx".to_string(),
            priority: None,
            expected_ctr_improvement: None,
            target_keyword: "test article".to_string(),
            fixes: vec![
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::TitleRewrite,
                    current: Some("Test Article | Brand | Brand -- Tagline".to_string()),
                    recommended: serde_json::json!(
                        "This title is deliberately far too long for the 55 character limit and cannot be auto-shortened to a usable value without losing meaning entirely"
                    ),
                    reason: None,
                },
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::MetaDescription,
                    current: Some("A short desc".to_string()),
                    recommended: serde_json::json!("Too short"),
                    reason: None,
                },
                crate::models::ctr::CtrFix {
                    fix_type: crate::models::ctr::CtrFixType::SnippetBait,
                    current: Some("This is the first paragraph...".to_string()),
                    recommended: serde_json::json!("One two three..."),
                    reason: None,
                },
            ],
        };

        let task = crate::models::task::Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "fix_ctr_article".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Fix test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "ctr_recommendations".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("ctr_audit".to_string()),
                content: Some(serde_json::to_string(&rec).unwrap()),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_ctr_fix_generate_with_backend(&task, &path, &backend).await;
        assert!(
            !result.success,
            "Should fail when repair also produces invalid patch, got success: {}",
            result.message
        );
        assert!(
            result.message.contains("repair") || result.message.contains("validation after repair"),
            "Expected repair failure message, got: {}",
            result.message
        );

        let _ = std::fs::remove_dir_all(&path);
    }
}
