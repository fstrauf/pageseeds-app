/// Shared health-check logic for SEO audit workflows.
///
/// Provides deterministic checks for common CTR / on-page SEO issues,
/// plus utilities for reading article excerpts and FAQ schema detection.

use std::path::Path;

/// Result of running all deterministic health checks on a single article.
#[derive(Debug, Clone, Default)]
pub struct ArticleHealth {
    pub title_ok: bool,
    pub meta_ok: bool,
    pub snippet_ok: bool,
    pub faq_ok: bool,
    /// List of issue keys for checks that FAILED.
    pub issues: Vec<String>,
    /// Number of words in the first paragraph.
    pub snippet_word_count: usize,
    /// Whether the first paragraph contains the target keyword or a question mark.
    pub snippet_has_keyword_or_question: bool,
}

impl ArticleHealth {
    /// True if ALL checks pass (article is healthy).
    pub fn all_ok(&self) -> bool {
        self.title_ok && self.meta_ok && self.snippet_ok && self.faq_ok
    }

    /// Human-readable summary of which checks failed.
    pub fn summary(&self) -> String {
        if self.all_ok() {
            return "healthy".to_string();
        }
        self.issues.join(", ")
    }
}

/// Run deterministic health checks against an article's current MDX state.
///
/// | Check            | Pass condition                                   |
/// |------------------|--------------------------------------------------|
/// | title_ok         | title.len() <= 60                                |
/// | meta_ok          | !meta.is_empty() && meta.len() >= 50             |
/// | snippet_ok       | word_count >= 30 && (has_keyword \|\| has '?')    |
/// | faq_ok           | has_faq_schema == true                           |
pub fn check_article_health(
    title: &str,
    meta: &str,
    first_paragraph: &str,
    target_keyword: &str,
    has_faq_schema: bool,
) -> ArticleHealth {
    let title_ok = title.len() <= 60;
    let meta_ok = !meta.is_empty() && meta.len() >= 50;

    let snippet_word_count = first_paragraph.split_whitespace().count();
    let first_lower = first_paragraph.to_lowercase();
    let keyword_lower = target_keyword.to_lowercase();
    let snippet_has_keyword_or_question =
        keyword_lower.is_empty() || first_lower.contains(&keyword_lower) || first_paragraph.contains('?');
    let snippet_ok = snippet_word_count >= 30 && snippet_has_keyword_or_question;

    let faq_ok = has_faq_schema;

    let mut issues = Vec::new();
    if !title_ok {
        issues.push("title_too_long".to_string());
    }
    if !meta_ok {
        issues.push("meta_too_short".to_string());
    }
    if !snippet_ok {
        issues.push("snippet_suboptimal".to_string());
    }
    if !faq_ok {
        issues.push("missing_faq_schema".to_string());
    }

    ArticleHealth {
        title_ok,
        meta_ok,
        snippet_ok,
        faq_ok,
        issues,
        snippet_word_count,
        snippet_has_keyword_or_question,
    }
}

/// Compute a simple content hash for detecting changes.
///
/// Hashes the concatenation of title + meta + first_paragraph so that
/// any change to health-relevant fields invalidates the previous audit state.
pub fn compute_content_hash(title: &str, meta: &str, first_paragraph: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    title.hash(&mut hasher);
    meta.hash(&mut hasher);
    first_paragraph.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

/// Read an MDX file and extract (title, meta_description, first_paragraph, h1, has_faq_schema).
pub fn read_article_excerpt(project_path: &str, file_ref: &str) -> (String, String, String, String, bool) {
    if file_ref.is_empty() {
        return (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            false,
        );
    }

    let repo_root = Path::new(project_path);
    let p = Path::new(file_ref);
    let full = if p.is_absolute() {
        p.to_path_buf()
    } else {
        repo_root.join(p)
    };

    let content = match std::fs::read_to_string(&full) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[audit_health] Could not read {}: {}", full.display(), e);
            return (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                false,
            );
        }
    };

    // Use cleaner::parse_frontmatter to split frontmatter and body
    let (frontmatter_str, body) = match crate::content::cleaner::parse_frontmatter(&content) {
        Some((fm, b)) => (fm, b),
        None => ("", content.as_str()),
    };

    // Extract title from frontmatter
    let title = frontmatter_str
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("title:") {
                let val = rest.trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
            None
        })
        .unwrap_or_default();

    // Extract meta_description from frontmatter
    let meta_description = frontmatter_str
        .lines()
        .find_map(|line| {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("description:") {
                let val = rest.trim().trim_matches('"').trim_matches('\'');
                if !val.is_empty() {
                    return Some(val.to_string());
                }
            }
            None
        })
        .unwrap_or_default();

    // Extract h1: first line starting with "# " in body
    let h1 = body
        .lines()
        .find(|l| {
            let t = l.trim_start();
            t.starts_with("# ") && !t.starts_with("## ")
        })
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .unwrap_or_default();

    // Extract first_paragraph: first non-empty, non-heading line
    let first_paragraph = body
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("---"))
        .unwrap_or("")
        .to_string();

    let has_faq = has_faq_schema(&content);

    (title, meta_description, first_paragraph, h1, has_faq)
}

/// Check whether an MDX file contains FAQ schema (JSON-LD FAQPage or markdown FAQ section).
pub fn has_faq_schema(content: &str) -> bool {
    // 1. Check for JSON-LD FAQPage schema
    let content_lower = content.to_lowercase();
    if content_lower.contains("faqpage")
        || content_lower.contains("\"@type\": \"question\"")
        || content_lower.contains("'@type': 'question'")
        || content_lower.contains("\"@type\":\"question\"")
    {
        return true;
    }

    // 2. Check for markdown FAQ headings
    content.lines().any(|line| {
        let trimmed = line.trim().to_lowercase();
        trimmed.starts_with("# faq")
            || trimmed.starts_with("## faq")
            || trimmed.starts_with("### faq")
            || trimmed.starts_with("# frequently asked questions")
            || trimmed.starts_with("## frequently asked questions")
            || trimmed.starts_with("### frequently asked questions")
    })
}
