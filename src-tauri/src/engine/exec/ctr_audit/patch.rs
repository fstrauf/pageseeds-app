/// Shared CTR fix patch normalization and validation.
///
/// Used by both `exec_ctr_fix_generate` (to validate before returning) and
/// `exec_ctr_fix_apply` (final safety boundary before writing).
use crate::models::ctr::{CtrFixPatch, CtrRecommendation};
use crate::models::task::Task;

/// Normalize a patch in-place before validation.
///
/// Returns a list of human-readable repair notes.
pub(crate) fn normalize_patch_before_validation(
    patch: &mut CtrFixPatch,
    task: &Task,
) -> Vec<String> {
    let mut repairs = Vec::new();
    let recommendation = extract_recommendation(task).ok().flatten();
    let target_keyword = recommendation
        .as_ref()
        .map(|rec| rec.target_keyword.trim())
        .unwrap_or("");

    if let Some(title) = patch.changes.title.as_mut() {
        normalize_whitespace_in_place(title, "title whitespace", &mut repairs);
        if title.chars().count() > crate::engine::exec::audit_health::TITLE_MAX_LEN {
            if let Some(shortened) =
                shorten_to_char_limit(title, crate::engine::exec::audit_health::TITLE_MAX_LEN)
            {
                if shortened.chars().count() <= crate::engine::exec::audit_health::TITLE_MAX_LEN {
                    *title = shortened;
                    repairs.push(format!(
                        "shortened title to <= {} chars",
                        crate::engine::exec::audit_health::TITLE_MAX_LEN
                    ));
                }
            }
        }
    }

    if let Some(description) = patch.changes.description.as_mut() {
        normalize_whitespace_in_place(description, "description whitespace", &mut repairs);
        if description.chars().count() > crate::engine::exec::audit_health::META_MAX_LEN {
            if let Some(shortened) = shorten_meta_description(
                description,
                crate::engine::exec::audit_health::META_MIN_LEN,
                crate::engine::exec::audit_health::META_MAX_LEN,
            ) {
                *description = shortened;
                repairs.push(format!(
                    "shortened description to {}-{} chars",
                    crate::engine::exec::audit_health::META_MIN_LEN,
                    crate::engine::exec::audit_health::META_MAX_LEN
                ));
            }
        }
    }

    if let Some(first_paragraph) = patch.changes.first_paragraph.as_mut() {
        normalize_whitespace_in_place(first_paragraph, "first_paragraph whitespace", &mut repairs);

        let word_count = first_paragraph.split_whitespace().count();
        let max_words = crate::engine::exec::audit_health::SNIPPET_MAX_WORDS;
        if word_count > max_words && word_count <= max_words + 5 {
            *first_paragraph = first_paragraph
                .split_whitespace()
                .take(max_words)
                .collect::<Vec<_>>()
                .join(" ");
            repairs.push(format!("trimmed first_paragraph to {} words", max_words));
        }

        if !has_keyword_or_question(first_paragraph, target_keyword) {
            add_keyword_or_question_marker(first_paragraph, target_keyword);
            repairs.push("added first_paragraph keyword/question marker".to_string());
        }
    }

    if let Some(questions) = patch.changes.faq_questions.as_mut() {
        for question in questions {
            normalize_whitespace_in_place(
                &mut question.question,
                "faq question whitespace",
                &mut repairs,
            );
            normalize_whitespace_in_place(
                &mut question.answer,
                "faq answer whitespace",
                &mut repairs,
            );
            let trimmed = question.question.trim_end();
            if !trimmed.is_empty() && !trimmed.ends_with('?') {
                question.question = format!("{}?", trimmed.trim_end_matches(&['.', '!', ':'][..]));
                repairs.push("added FAQ question mark".to_string());
            }
        }
    }

    if let Some(snippet) = patch.changes.snippet_patch.as_mut() {
        normalize_whitespace_in_place(
            &mut snippet.heading,
            "snippet heading whitespace",
            &mut repairs,
        );
        normalize_whitespace_in_place(
            &mut snippet.answer_paragraph,
            "snippet answer whitespace",
            &mut repairs,
        );
        let word_count = snippet.answer_paragraph.split_whitespace().count();
        let max_words = crate::engine::exec::audit_health::SNIPPET_MAX_WORDS;
        if word_count > max_words && word_count <= max_words + 5 {
            snippet.answer_paragraph = snippet
                .answer_paragraph
                .split_whitespace()
                .take(max_words)
                .collect::<Vec<_>>()
                .join(" ");
            repairs.push(format!(
                "trimmed snippet answer_paragraph to {} words",
                max_words
            ));
        }
    }

    repairs
}

/// Validate a patch against deterministic rules.
///
/// Returns a list of error messages. Empty list means the patch is valid.
pub(crate) fn validate_patch_before_write(
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

/// Parse title, meta, and first paragraph from raw MDX content.
pub(crate) fn parse_content_excerpt(content: &str) -> (String, String, String) {
    let (frontmatter_str, body) = match crate::content::frontmatter::split_mdx(content) {
        Some((fm, b)) => (fm, b),
        None => ("", content),
    };

    let scalars = crate::content::frontmatter::top_level_scalars(frontmatter_str);
    let title = scalars
        .iter()
        .find(|s| s.key == "title")
        .and_then(|s| {
            let v = s.raw_value.trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                Some(v.to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();

    let meta = scalars
        .iter()
        .find(|s| s.key == "description")
        .and_then(|s| {
            let v = s.raw_value.trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                Some(v.to_string())
            } else {
                None
            }
        })
        .unwrap_or_default();

    let first_paragraph = crate::content::cleaner::find_first_paragraph_range(body)
        .map(|(start, end)| body[start..end].to_string())
        .unwrap_or_default();

    (title, meta, first_paragraph)
}

/// Validate that the patch satisfies the recommendations (requested fixes are represented).
pub(crate) fn validate_patch_against_recommendation(
    patch: &CtrFixPatch,
    rec: &CtrRecommendation,
    original_content: &str,
) -> Vec<String> {
    let mut errors = Vec::new();

    if patch.article_id != rec.article_id {
        errors.push(format!(
            "patch.article_id ({}) does not match recommendation.article_id ({})",
            patch.article_id, rec.article_id
        ));
    }

    if patch.file != rec.file {
        errors.push(format!(
            "patch.file ('{}') does not match recommendation.file ('{}')",
            patch.file, rec.file
        ));
    }

    let fixes: Vec<crate::models::ctr::CtrFixType> =
        rec.fixes.iter().map(|f| f.fix_type.clone()).collect();

    let has_title_issue = fixes.contains(&crate::models::ctr::CtrFixType::TitleRewrite);
    let has_meta_issue = fixes.contains(&crate::models::ctr::CtrFixType::MetaDescription);
    let has_snippet_issue = fixes.contains(&crate::models::ctr::CtrFixType::SnippetBait);
    let has_faq_issue = fixes.contains(&crate::models::ctr::CtrFixType::FaqSchema);

    let (current_title, current_meta, current_first) = parse_content_excerpt(original_content);

    // Title: if title_rewrite was requested, patch must have title unless current title passes.
    if has_title_issue && patch.changes.title.is_none() {
        if current_title.chars().count() > crate::engine::exec::audit_health::TITLE_MAX_LEN {
            errors.push(
                "title_rewrite was requested but patch has no title and current title is too long"
                    .to_string(),
            );
        }
    }

    // Meta: if meta_description was requested, patch must have description unless current passes.
    if has_meta_issue && patch.changes.description.is_none() {
        let meta_len = current_meta.chars().count();
        if meta_len < crate::engine::exec::audit_health::META_MIN_LEN
            || meta_len > crate::engine::exec::audit_health::META_MAX_LEN
        {
            errors.push(
                "meta_description was requested but patch has no description and current meta is out of range".to_string(),
            );
        }
    }

    // Snippet: if snippet_bait was requested, patch must have first_paragraph or snippet_patch unless current passes.
    if has_snippet_issue
        && patch.changes.first_paragraph.is_none()
        && patch.changes.snippet_patch.is_none()
    {
        let word_count = current_first.split_whitespace().count();
        if word_count < crate::engine::exec::audit_health::SNIPPET_MIN_WORDS
            || word_count > crate::engine::exec::audit_health::SNIPPET_MAX_WORDS
        {
            errors.push(
                "snippet_bait was requested but patch has no first_paragraph or snippet_patch and current snippet is out of range".to_string(),
            );
        }
    }

    // FAQ: if faq_schema was requested, patch must have faq_questions unless file already has FAQ.
    if has_faq_issue && patch.changes.faq_questions.is_none() {
        if !crate::engine::exec::audit_health::has_frontmatter_faq(original_content) {
            errors.push(
                "faq_schema was requested but patch has no faq_questions and file has no frontmatter FAQ"
                    .to_string(),
            );
        }
    }

    // Error on unrequested title/meta/first_paragraph changes (not snippet_patch/faq since those can be additive)
    if patch.changes.title.is_some() && !has_title_issue {
        errors.push("patch includes title change but title_rewrite was not requested".to_string());
    }
    if patch.changes.description.is_some() && !has_meta_issue {
        errors.push(
            "patch includes description change but meta_description was not requested".to_string(),
        );
    }
    if patch.changes.first_paragraph.is_some() && !has_snippet_issue {
        errors.push(
            "patch includes first_paragraph change but snippet_bait was not requested".to_string(),
        );
    }

    errors
}

/// Extract the single CtrRecommendation from the task's ctr_recommendations artifact.
pub(crate) fn extract_recommendation(task: &Task) -> Result<Option<CtrRecommendation>, String> {
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

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn normalize_whitespace_in_place(value: &mut String, note: &str, repairs: &mut Vec<String>) {
    let normalized = collapse_whitespace(value);
    if normalized != *value {
        *value = normalized;
        repairs.push(note.to_string());
    }
}

fn collapse_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn shorten_to_char_limit(value: &str, max_chars: usize) -> Option<String> {
    let clean = collapse_whitespace(value);
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
        None
    }
}

fn has_keyword_or_question(value: &str, target_keyword: &str) -> bool {
    target_keyword.is_empty()
        || value
            .to_lowercase()
            .contains(&target_keyword.to_lowercase())
        || value.contains('?')
}

fn add_keyword_or_question_marker(value: &mut String, target_keyword: &str) {
    let keyword_word_count = target_keyword.split_whitespace().count();
    let word_count = value.split_whitespace().count();
    let max_words = crate::engine::exec::audit_health::SNIPPET_MAX_WORDS;
    if !target_keyword.is_empty() && word_count + keyword_word_count <= max_words {
        if !value.ends_with(' ') {
            value.push(' ');
        }
        value.push_str(target_keyword.trim());
        value.push('?');
        return;
    }

    let trimmed = value.trim_end();
    if trimmed.ends_with('.') || trimmed.ends_with('!') || trimmed.ends_with(':') {
        let mut chars: Vec<char> = trimmed.chars().collect();
        if let Some(last) = chars.last_mut() {
            *last = '?';
        }
        *value = chars.into_iter().collect();
    } else if !trimmed.ends_with('?') {
        *value = format!("{}?", trimmed);
    }
}
