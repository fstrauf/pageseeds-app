/// Typed content fix patch generation using Rig structured extraction.
///
/// 1. Load content fix context from task artifacts.
/// 2. Build a focused prompt with skill context + recommendations + current content.
/// 3. Call `rig::extraction::extract_with_backend::<ContentFixPatch>()`.
/// 4. Normalize and validate the patch.
/// 5. If invalid, perform exactly one repair extraction.
/// 6. Return the typed patch JSON as step output.
use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::models::content_review::{ContentFixPatch, ReviewSuggestion};
use crate::models::task::Task;
use crate::rig::provider::LlmBackend;

pub(crate) async fn exec_fix_content_article_generate(
    _step: &crate::engine::workflows::WorkflowStep,
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
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

    exec_fix_content_article_generate_with_backend(task, project_path, &backend).await
}

/// Core logic, testable with a mocked backend.
pub(crate) async fn exec_fix_content_article_generate_with_backend(
    task: &Task,
    project_path: &str,
    backend: &LlmBackend,
) -> StepResult {
    // 1. Load context
    let context = match extract_context(task) {
        Ok(Some(c)) => c,
        Ok(None) => {
            return StepResult::fail("No content_fix_context artifact found on task".to_string());
        }
        Err(e) => {
            return StepResult::fail(format!(
                    "content_fix_context artifact exists but is invalid: {}. \
                     This usually means the context step produced unexpected JSON.",
                    e
                ));
        }
    };

    // 2. Read file
    let repo_root = Path::new(project_path);
    let file_path = match crate::engine::exec::audit_health::resolve_content_file(repo_root, &context.file) {
        Some(p) => p,
        None => {
            return StepResult::fail(format!(
                    "File not found: {}. Run sanitize_content to repair paths.",
                    context.file
                ));
        }
    };

    let original_content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(_e) => {
            return StepResult::fail(format!("File not found: {}", file_path.display()));
        }
    };

    // 3. Build prompt
    let prompt = match build_fix_prompt(task, project_path, &context, &original_content) {
        Ok(p) => p,
        Err(e) => {
            return StepResult::fail(format!("Failed to build content fix prompt: {}", e));
        }
    };

    // 4. Extract structured patch — scope agentic backends to the project so
    // the kimi CLI agent's file tools verify slugs/files against the real
    // project, not the app process cwd (placeholder work_dir from resolve_backend).
    let scoped_backend = backend.scoped_to_project(project_path);
    let mut patch = match crate::rig::extraction::extract_with_backend::<ContentFixPatch>(
        &scoped_backend,
        &prompt,
        Some(
            "You are a content SEO optimization assistant. \
             Return only a valid ContentFixPatch by calling the submit tool. \
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
                    "Structured extraction failed for ContentFixPatch: {}. \
                     If you are using KimiDirect, please switch to a structured-output provider (Kimi bridge, Claude, OpenAI, or Ollama).",
                    e
                ));
        }
    };

    // 5. Normalize and validate
    let target_kw = context.target_keyword.as_deref();
    let repairs = normalize_patch_before_validation(&mut patch, &original_content);
    let errors = validate_patch_before_write(&patch, &original_content, target_kw, &task.project_id, project_path, &context.available_link_slugs);

    // 6. One repair attempt if needed
    if !errors.is_empty() {
        log::info!(
            "[fix_content_generate] First patch invalid for {}: {}. Attempting repair.",
            context.file,
            errors.join("; ")
        );

        match repair_content_fix_patch_with_backend(&scoped_backend, &prompt, &patch, &errors).await {
            Ok(mut repaired) => {
                let _repair_notes = normalize_patch_before_validation(&mut repaired, &original_content);
                let repair_errors = validate_patch_before_write(&repaired, &original_content, target_kw, &task.project_id, project_path, &context.available_link_slugs);

                if !repair_errors.is_empty() {
                    return StepResult::fail(format!(
                            "Content fix patch failed validation after repair: {}. No changes written.",
                            repair_errors.join("; ")
                        ));
                }
                patch = repaired;
            }
            Err(e) => {
                return StepResult::fail(format!(
                        "Content fix patch repair extraction failed: {}. No changes written.",
                        e
                    ));
            }
        }
    }

    // 7. Return patch as JSON
    let patch_json = match serde_json::to_string_pretty(&patch) {
        Ok(s) => s,
        Err(e) => {
            return StepResult::fail(format!("Failed to serialize ContentFixPatch: {}", e));
        }
    };

    let repair_msg = if repairs.is_empty() {
        String::new()
    } else {
        format!(" (normalized: {})", repairs.join(", "))
    };

    StepResult {
        success: true,
        message: format!("Generated typed ContentFixPatch for {}{}", context.file, repair_msg),
        output: Some(patch_json),
        artifact_key: None,
    }
}

// ─── Context loading ──────────────────────────────────────────────────────────

struct FixContext {
    pub article_id: i64,
    pub file: String,
    pub article_title: String,
    pub target_keyword: Option<String>,
    pub suggestions: Vec<ReviewSuggestion>,
    /// Deterministic valid internal link targets, enriched by the context
    /// step from `task_store::load_valid_link_targets`. Empty for historical
    /// artifacts or when the lookup failed — the prompt then falls back to
    /// the "do not link when unsure" rule.
    pub available_link_slugs: Vec<String>,
}

fn extract_context(task: &Task) -> Result<Option<FixContext>, String> {
    let context_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "content_fix_context")
        .and_then(|a| a.content.as_deref())
        .unwrap_or("");

    if context_json.is_empty() {
        return Ok(None);
    }

    let value: serde_json::Value = serde_json::from_str(context_json).map_err(|e| e.to_string())?;

    let article_id = value["article_id"].as_i64().unwrap_or(0);
    let file = value["article_file"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let article_title = value["article_title"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let target_keyword = value["target_keyword"]
        .as_str()
        .map(|s| normalize_target_keyword(s, article_id));

    let suggestions: Vec<ReviewSuggestion> = value["suggestions"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|s| serde_json::from_value(s.clone()).ok())
        .collect();

    let available_link_slugs: Vec<String> = value["available_link_slugs"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|s| s.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    Ok(Some(FixContext {
        article_id,
        file,
        article_title,
        target_keyword,
        suggestions,
        available_link_slugs,
    }))
}

// ─── Prompt builder ───────────────────────────────────────────────────────────

fn build_fix_prompt(
    _task: &Task,
    project_path: &str,
    context: &FixContext,
    original_content: &str,
) -> Result<String, String> {
    let repo_root = Path::new(project_path);

    // Load skill if available
    let skill_content = crate::engine::skills::load_skill(repo_root, "content-fix-apply")
        .map(|s| s.content)
        .unwrap_or_else(|| "Apply SEO content fixes to improve title, meta description, intro, internal links, FAQ, EEAT, and CTA.".to_string());

    // Parse current excerpt
    let (current_title, current_meta, current_first) =
        crate::engine::exec::ctr_audit::parse_content_excerpt(original_content);
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
    let intro_min = crate::engine::exec::audit_health::SNIPPET_MIN_WORDS;
    let intro_max = crate::engine::exec::audit_health::SNIPPET_MAX_WORDS;

    let suggestions_json = serde_json::to_string_pretty(&context.suggestions).map_err(|e| e.to_string())?;

    // The valid-target list is guaranteed by Rust context enrichment (same
    // source as validate_patch_before_write enforces). When it is non-empty
    // the model may ONLY link from it; when it is empty (historical artifact
    // or failed lookup) the old "do not link when unsure" rule applies.
    let (link_targets_section, link_rule) = if context.available_link_slugs.is_empty() {
        (
            String::new(),
            "Only link to articles that actually exist in this project. If you are unsure whether a target exists, do NOT include it.".to_string(),
        )
    } else {
        (
            format!(
                "### Valid internal link targets in this project\n```\n{}\n```\n\n",
                context.available_link_slugs.join("\n")
            ),
            "Only link to slugs from the valid internal link target list above — every slug in that list is guaranteed to exist in this project. Never invent a target that is not on the list.".to_string(),
        )
    };

    let prompt = format!(
        r#"## Skill

{skill_content}

## Current File State

- file: {file}
- article_id: {article_id}
- article_title: {article_title}
- target_keyword: {target_keyword}

### Current title
```{current_title}```
(title is {title_len} chars; max allowed: {title_max})

### Current meta description
```{current_meta}```
(meta is {meta_len} chars; allowed range: {meta_min}-{meta_max})

### Current first paragraph
```{current_first}```
(first paragraph is {first_words} words)

### Has frontmatter FAQ
{has_faq}

{link_targets_section}### Body excerpt
```
{body_excerpt}
```

## Recommendations

```json
{suggestions_json}
```

## Instructions

You must produce a ContentFixPatch JSON that addresses every suggestion listed above, **unless the current file state already satisfies the requirement**.

Validation rules (enforced by Rust):
- title: must be ≤ {title_max} chars if provided
- description: must be {meta_min}-{meta_max} chars if provided
- intro: should be {intro_min}-{intro_max} words if provided
- faq_questions: must be 3-5 questions if provided and file has no existing frontmatter FAQ

**CRITICAL — Keyword placement**: The target keyword is "{target_keyword}". Whenever you generate a new title, H1, meta description, or intro, you MUST naturally include the target keyword in the text. This applies to ALL changes in those fields, not just keyword-specific recommendations:
- If generating a new title: the target keyword must appear in the title
- If generating a new H1: the target keyword must appear in the H1
- If generating a new meta description: the target keyword must appear in the description
- If generating a new intro: the target keyword must appear in the first paragraph

**CRITICAL — Internal links format**:
- If you include `internal_links`, each entry must use the bare slug as `target_slug` (e.g., `"my-post"`), NEVER `/blog/my-post` or `blog/my-post`.
- The Rust code automatically wraps it as `/blog/<slug>` when writing the file.
- {link_rule}
- Example CORRECT: `{{"anchor_text": "learn more", "target_slug": "options-trading-basics"}}`
- Example WRONG: `{{"anchor_text": "learn more", "target_slug": "/blog/options-trading-basics"}}`

Only include fields that need to change. Do not include title/description/intro/h1 changes if those fixes were not requested.
"#,
        skill_content = skill_content.trim(),
        file = context.file,
        article_id = context.article_id,
        article_title = context.article_title,
        target_keyword = context.target_keyword.as_deref().unwrap_or("(none)"),
        current_title = current_title,
        title_len = current_title.chars().count(),
        title_max = title_max,
        current_meta = current_meta,
        meta_len = current_meta.chars().count(),
        meta_min = meta_min,
        meta_max = meta_max,
        current_first = current_first,
        first_words = crate::content::ops::count_words(&current_first),
        intro_min = intro_min,
        intro_max = intro_max,
        has_faq = if has_faq {
            "yes — do NOT generate faq_questions"
        } else {
            "no — generate faq_questions if requested"
        },
        body_excerpt = body_excerpt.trim(),
        suggestions_json = suggestions_json,
    );

    Ok(prompt)
}

// ─── Keyword normalization ────────────────────────────────────────────────────

/// Truncate overly long target keywords (full questions, sentences, paragraphs)
/// that were mistakenly stored as "target_keyword" instead of a short phrase.
///
/// A keyword longer than 100 characters is almost certainly not a keyword — it is
/// a full question or description that snuck in from the content pipeline. Truncate
/// it so downstream prompts don't feed 50-word "keywords" to the AI.
pub(crate) fn normalize_target_keyword(kw: &str, article_id: i64) -> String {
    const MAX_KEYWORD_CHARS: usize = 100;
    if kw.len() <= MAX_KEYWORD_CHARS {
        return kw.trim().to_string();
    }
    log::warn!(
        "[fix_generate] target_keyword for article {} is {} chars (max {}). Truncating.",
        article_id,
        kw.len(),
        MAX_KEYWORD_CHARS
    );
    let truncated: String = kw.chars().take(MAX_KEYWORD_CHARS).collect();
    format!("{}…", truncated.trim_end())
}

// ─── Patch normalization ─────────────────────────────────────────────────────

fn normalize_patch_before_validation(patch: &mut ContentFixPatch, _original_content: &str) -> Vec<String> {
    let mut notes = Vec::new();
    let title_max = crate::engine::exec::audit_health::TITLE_MAX_LEN;
    let meta_min = crate::engine::exec::audit_health::META_MIN_LEN;
    let meta_max = crate::engine::exec::audit_health::META_MAX_LEN;

    // Trim whitespace from string fields
    if let Some(ref mut t) = patch.changes.title {
        *t = t.trim().to_string();
    }
    if let Some(ref mut d) = patch.changes.description {
        *d = d.trim().to_string();
    }
    if let Some(ref mut i) = patch.changes.intro {
        *i = i.trim().to_string();
    }
    if let Some(ref mut h) = patch.changes.h1 {
        *h = h.trim().to_string();
    }

    // Auto-truncate title if over limit (LLMs are bad at exact char counting)
    if let Some(ref mut t) = patch.changes.title {
        if t.chars().count() > title_max {
            if let Some(shortened) = shorten_to_char_limit(t, title_max) {
                notes.push(format!(
                    "title truncated from {} to {} chars",
                    t.chars().count(),
                    shortened.chars().count()
                ));
                *t = shortened;
            }
        }
    }

    // Auto-truncate description if over limit
    if let Some(ref mut d) = patch.changes.description {
        let len = d.chars().count();
        if len > meta_max {
            if let Some(shortened) = shorten_meta_description(d, meta_min, meta_max) {
                notes.push(format!(
                    "description truncated from {} to {} chars",
                    len,
                    shortened.chars().count()
                ));
                *d = shortened;
            }
        }
    }

    // Auto-pad description if slightly under minimum (LLMs often undershoot)
    if let Some(ref mut d) = patch.changes.description {
        let len = d.chars().count();
        if len < meta_min && len >= 100 {
            let padding_needed = meta_min.saturating_sub(len);
            let suffixes = [
                " Discover expert tips and practical advice in our complete guide.",
                " Learn everything you need to know with our detailed breakdown.",
                " Explore proven strategies and actionable insights inside.",
            ];
            for suffix in &suffixes {
                let candidate = format!("{} {}", d.trim_end_matches('.'), suffix);
                let candidate_len = candidate.chars().count();
                if candidate_len >= meta_min && candidate_len <= meta_max {
                    notes.push(format!(
                        "description padded from {} to {} chars",
                        len, candidate_len
                    ));
                    *d = candidate;
                    break;
                }
            }
        }
    }

    // Normalize internal_links slugs — strip any /blog/ prefix the agent may have included
    if let Some(ref mut links) = patch.changes.internal_links {
        for link in links {
            let normalized = crate::content::slug::normalize_url_slug(&link.target_slug);
            if normalized != link.target_slug {
                notes.push(format!(
                    "internal_links slug normalized: '{}' -> '{}'",
                    link.target_slug, normalized
                ));
                link.target_slug = normalized;
            }
        }
    }

    notes
}

/// Smart word-boundary truncation to fit within a char limit.
fn shorten_to_char_limit(value: &str, max_chars: usize) -> Option<String> {
    let clean = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() <= max_chars {
        return Some(clean);
    }

    let mut shortened = String::new();
    for word in clean.split_whitespace() {
        let candidate = if shortened.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", shortened, word)
        };
        if candidate.chars().count() > max_chars {
            break;
        }
        shortened = candidate;
    }

    let shortened = shortened
        .trim_end_matches(&[' ', ',', ';', ':', '-', '|'][..])
        .trim()
        .to_string();

    if shortened.is_empty() {
        None
    } else {
        Some(shortened)
    }
}

/// Truncate meta description to fit range, adding a period if needed.
fn shorten_meta_description(value: &str, min_chars: usize, max_chars: usize) -> Option<String> {
    let mut shortened = shorten_to_char_limit(value, max_chars)?;
    shortened = shortened
        .trim_end_matches(&[' ', ',', ';', ':', '-'][..])
        .trim()
        .to_string();

    if !shortened.ends_with('.') && !shortened.ends_with('!') && !shortened.ends_with('?') {
        if shortened.chars().count() < max_chars {
            shortened.push('.');
        }
    }

    let len = shortened.chars().count();
    if len >= min_chars && len <= max_chars {
        Some(shortened)
    } else {
        // If even the truncated version is under min, still return it —
        // validation will catch the under-length case, but at least we
        // fixed the over-length case.
        Some(shortened)
    }
}

// ─── Patch validation ────────────────────────────────────────────────────────

fn validate_patch_before_write(
    patch: &ContentFixPatch,
    original_content: &str,
    target_keyword: Option<&str>,
    project_id: &str,
    project_path: &str,
    available_link_slugs: &[String],
) -> Vec<String> {
    let mut errors = Vec::new();
    let title_max = crate::engine::exec::audit_health::TITLE_MAX_LEN;
    let meta_min = crate::engine::exec::audit_health::META_MIN_LEN;
    let meta_max = crate::engine::exec::audit_health::META_MAX_LEN;
    let intro_min = crate::engine::exec::audit_health::SNIPPET_MIN_WORDS;
    let intro_max = crate::engine::exec::audit_health::SNIPPET_MAX_WORDS;
    let has_faq = crate::engine::exec::audit_health::has_frontmatter_faq(original_content);

    // Keywords are normalized to titleable length at the GSC backfill
    // boundary (issue #74), so no length-based skip is needed here — an
    // empty keyword simply disables the keyword placement checks.
    let kw_for_check: Option<String> = target_keyword
        .map(|kw| kw.trim().to_lowercase())
        .filter(|kw| !kw.is_empty());

    if let Some(ref t) = patch.changes.title {
        if t.chars().count() > title_max {
            errors.push(format!(
                "title is {} chars (max {})",
                t.chars().count(),
                title_max
            ));
        }
        if let Some(ref kw) = kw_for_check {
            if !crate::content::keyword_match::keyword_present(&t.to_lowercase(), kw) {
                errors.push(format!(
                    "title does not contain target keyword '{}'",
                    kw
                ));
            }
        }
    }

    if let Some(ref d) = patch.changes.description {
        let len = d.chars().count();
        if len < meta_min || len > meta_max {
            errors.push(format!(
                "description is {} chars (expected {}-{})",
                len, meta_min, meta_max
            ));
        }
        if let Some(ref kw) = kw_for_check {
            if !crate::content::keyword_match::keyword_present(&d.to_lowercase(), kw) {
                errors.push(format!(
                    "description does not contain target keyword '{}'",
                    kw
                ));
            }
        }
    }

    if let Some(ref intro) = patch.changes.intro {
        let word_count = crate::content::ops::count_words(intro);
        if word_count < intro_min || word_count > intro_max {
            errors.push(format!(
                "intro is {} words (expected {}-{})",
                word_count, intro_min, intro_max
            ));
        }
        if let Some(ref kw) = kw_for_check {
            if !crate::content::keyword_match::keyword_present(&intro.to_lowercase(), kw) {
                errors.push(format!(
                    "intro does not contain target keyword '{}'",
                    kw
                ));
            }
        }
    }

    if let Some(ref faq) = patch.changes.faq_questions {
        if has_faq {
            errors.push("faq_questions provided but file already has frontmatter FAQ".to_string());
        }
        let count = faq.len();
        if count < 3 || count > 5 {
            errors.push(format!("faq_questions has {} items (expected 3-5)", count));
        }
        for (i, q) in faq.iter().enumerate() {
            if q.question.trim().is_empty() {
                errors.push(format!("faq_questions[{}].question is empty", i));
            }
            if q.answer.trim().is_empty() {
                errors.push(format!("faq_questions[{}].answer is empty", i));
            }
        }
    }

    // Validate internal_links: target slugs must exist in the project and not
    // be redirected away. Exact match first (resolve_slug) so verbatim-existing
    // slugs are never destructively normalized.
    if let Some(ref links) = patch.changes.internal_links {
        // Enforce the same list the prompt advertised (context artifact) when
        // available — single source of truth, and independent of the app DB.
        // Fall back to a live DB lookup for legacy artifacts without the field.
        let valid_slugs: std::collections::HashSet<String> = if !available_link_slugs.is_empty() {
            available_link_slugs.iter().cloned().collect()
        } else if let Ok(db) = rusqlite::Connection::open(crate::db::default_db_path()) {
            crate::engine::task_store::load_valid_link_targets(&db, project_id, project_path)
                .unwrap_or_default()
        } else {
            std::collections::HashSet::new()
        };

        for (i, link) in links.iter().enumerate() {
            if link.target_slug.is_empty() {
                errors.push(format!("internal_links[{}].target_slug is empty", i));
            } else if link.anchor_text.trim().is_empty() {
                errors.push(format!("internal_links[{}].anchor_text is empty", i));
            } else if crate::content::slug::resolve_slug(&link.target_slug, &valid_slugs).is_none()
            {
                errors.push(format!(
                    "internal_links[{}].target_slug '{}' does not match any article in this project",
                    i, link.target_slug
                ));
            }
        }
    }

    errors
}

// ─── Repair prompt ────────────────────────────────────────────────────────────

async fn repair_content_fix_patch_with_backend(
    backend: &LlmBackend,
    original_prompt: &str,
    invalid_patch: &ContentFixPatch,
    errors: &[String],
) -> Result<ContentFixPatch, String> {
    let repair_prompt = format!(
        r#"The following ContentFixPatch failed validation.

## Original Prompt

{original_prompt}

## Invalid Patch

```json
{patch_json}
```

## Validation Errors

{errors}

## Instructions

Fix the patch so it passes all validation rules. Return only the corrected ContentFixPatch by calling the submit tool. Address every error listed above."#,
        original_prompt = original_prompt,
        patch_json = serde_json::to_string_pretty(invalid_patch).map_err(|e| e.to_string())?,
        errors = errors
            .iter()
            .map(|e| format!("- {}", e))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    crate::rig::extraction::extract_with_backend::<ContentFixPatch>(
        backend,
        &repair_prompt,
        Some(
         "You are correcting a previously invalid ContentFixPatch. \
              Return only the corrected ContentFixPatch by calling the submit tool.",
        ),
        Some("acp"),
        None,
    )
    .await
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, TaskArtifact, TaskReviewSurface, TaskRun,
        TaskRunPolicy, TaskStatus,
    };

    const SAMPLE_MDX: &str = "---\ntitle: \"Container Gardening Basics\"\ndescription: \"A solid meta description.\"\ndate: \"2026-01-01\"\n---\n\n# Container Gardening Basics\n\nAn intro paragraph about container gardening with enough words to matter.\n";

    fn task_with_context_artifact(context: serde_json::Value) -> Task {
        let now = chrono::Utc::now().to_rfc3339();
        Task {
            id: "task-fix-gen".to_string(),
            project_id: "p1".to_string(),
            task_type: "fix_content_article".to_string(),
            phase: "fix".to_string(),
            status: TaskStatus::InProgress,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::None,
            title: Some("Fix article".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![TaskArtifact {
                key: "content_fix_context".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("fix_content_article".to_string()),
                content: Some(context.to_string()),
            }],
            run: TaskRun::default(),
            created_at: now.clone(),
            not_before: None,
            updated_at: now,
        }
    }

    fn context_json(extra: serde_json::Value) -> serde_json::Value {
        let mut ctx = serde_json::json!({
            "article_id": 7,
            "article_file": "content/blog/slug.mdx",
            "article_title": "Some Article",
            "target_keyword": "container gardening",
            "suggestions": [],
        });
        if let serde_json::Value::Object(extra) = extra {
            for (k, v) in extra {
                ctx[k] = v;
            }
        }
        ctx
    }

    fn fix_context(link_slugs: Vec<String>) -> FixContext {
        FixContext {
            article_id: 7,
            file: "content/blog/slug.mdx".to_string(),
            article_title: "Some Article".to_string(),
            target_keyword: Some("container gardening".to_string()),
            suggestions: vec![],
            available_link_slugs: link_slugs,
        }
    }

    #[test]
    fn extract_context_reads_available_link_slugs() {
        let task = task_with_context_artifact(context_json(serde_json::json!({
            "available_link_slugs": ["alpha-post", "beta-post"]
        })));
        let ctx = extract_context(&task).unwrap().unwrap();
        assert_eq!(ctx.available_link_slugs, vec!["alpha-post", "beta-post"]);
    }

    #[test]
    fn extract_context_tolerates_missing_link_slugs() {
        // Historical artifacts predate the enrichment — absence must degrade
        // to an empty list, not an error.
        let task = task_with_context_artifact(context_json(serde_json::json!({})));
        let ctx = extract_context(&task).unwrap().unwrap();
        assert!(ctx.available_link_slugs.is_empty());
    }

    #[test]
    fn prompt_lists_valid_targets_and_list_only_rule() {
        let task = task_with_context_artifact(context_json(serde_json::json!({})));
        let ctx = fix_context(vec!["alpha-post".to_string(), "beta-post".to_string()]);
        let prompt = build_fix_prompt(&task, "/tmp", &ctx, SAMPLE_MDX).unwrap();
        assert!(prompt.contains("Valid internal link targets in this project"));
        assert!(prompt.contains("alpha-post"));
        assert!(prompt.contains("beta-post"));
        assert!(prompt.contains("Only link to slugs from the valid internal link target list"));
        assert!(!prompt.contains("If you are unsure whether a target exists"));
    }

    #[test]
    fn prompt_without_targets_keeps_unsure_rule() {
        let task = task_with_context_artifact(context_json(serde_json::json!({})));
        let ctx = fix_context(vec![]);
        let prompt = build_fix_prompt(&task, "/tmp", &ctx, SAMPLE_MDX).unwrap();
        assert!(!prompt.contains("Valid internal link targets in this project"));
        assert!(prompt.contains("If you are unsure whether a target exists, do NOT include it"));
    }
}
