use crate::models::reddit::ValidationResult;

/// Validate a Reddit reply against base rules (length, URLs, markdown links, sentence count, word count).
pub fn validate_reply(text: &str) -> ValidationResult {
    let text = text.trim();

    if text.len() < 10 {
        return ValidationResult {
            valid: false,
            error: Some("Reply is too short (minimum 10 characters).".to_string()),
        };
    }
    if text.contains("http://") || text.contains("https://") {
        return ValidationResult {
            valid: false,
            error: Some("Reply must not contain URLs.".to_string()),
        };
    }
    if regex::Regex::new(r"\[.+?\]\(.+?\)").unwrap().is_match(text) {
        return ValidationResult {
            valid: false,
            error: Some("Reply must not contain markdown links.".to_string()),
        };
    }
    let sentences: Vec<&str> = text
        .split(['.', '!', '?'])
        .filter(|s| !s.trim().is_empty())
        .collect();
    if sentences.len() < 3 {
        return ValidationResult {
            valid: false,
            error: Some(format!(
                "{} sentence(s) — minimum 3 required.",
                sentences.len()
            )),
        };
    }
    if sentences.len() > 5 {
        return ValidationResult {
            valid: false,
            error: Some(format!(
                "{} sentences — maximum 5 allowed.",
                sentences.len()
            )),
        };
    }
    let word_count = crate::content::ops::count_words(text);
    if word_count < 30 {
        return ValidationResult {
            valid: false,
            error: Some(format!("{} words — minimum 30 recommended.", word_count)),
        };
    }
    if word_count > 250 {
        return ValidationResult {
            valid: false,
            error: Some(format!("{} words — maximum 250 recommended.", word_count)),
        };
    }
    ValidationResult {
        valid: true,
        error: None,
    }
}

/// Validate a reply against project-specific guardrails (mention stance).
///
/// Checks whether the reply mentions the product by name when the project's
/// Reddit config requires it.
pub fn validate_project_stance(text: &str, automation_dir: &std::path::Path) -> ValidationResult {
    let Ok(cfg) = crate::reddit::config::load_reddit_config(automation_dir) else {
        return ValidationResult {
            valid: true,
            error: None,
        };
    };

    if cfg.mention_stance == crate::reddit::config::MentionStance::Required {
        if let Some(product) = &cfg.product_name {
            if !text.to_lowercase().contains(&product.to_lowercase()) {
                return ValidationResult {
                    valid: false,
                    error: Some(format!(
                        "Reply must mention \"{}\" by name (mention stance: REQUIRED).",
                        product
                    )),
                };
            }
        }
    }

    ValidationResult {
        valid: true,
        error: None,
    }
}
