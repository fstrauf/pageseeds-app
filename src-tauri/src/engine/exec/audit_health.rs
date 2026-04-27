/// Shared health-check logic for SEO audit workflows.
///
/// Provides deterministic checks for common CTR / on-page SEO issues,
/// plus utilities for reading article excerpts and FAQ schema detection.

use std::path::{Path, PathBuf};

/// Resolve a content file path, trying `.md` ↔ `.mdx` extension fallback.
///
/// If the exact path does not exist, attempts the alternate extension:
/// - `foo.mdx` → tries `foo.md`
/// - `foo.md`  → tries `foo.mdx`
pub fn resolve_content_file(repo_root: &Path, file_ref: &str) -> Option<PathBuf> {
    if file_ref.is_empty() {
        return None;
    }

    let p = Path::new(file_ref);
    let full = if p.is_absolute() {
        p.to_path_buf()
    } else {
        repo_root.join(p)
    };

    if full.exists() {
        return Some(full);
    }

    // Try alternate extension
    let parent = full.parent()?;
    let stem = full.file_stem()?;
    let ext = full.extension();

    let alt = if ext == Some(std::ffi::OsStr::new("mdx")) {
        parent.join(format!("{}.md", stem.to_string_lossy()))
    } else if ext == Some(std::ffi::OsStr::new("md")) {
        parent.join(format!("{}.mdx", stem.to_string_lossy()))
    } else {
        return None;
    };

    if alt.exists() {
        Some(alt)
    } else {
        None
    }
}

/// Result of running all deterministic health checks on a single article.
#[derive(Debug, Clone, Default)]
pub struct ArticleHealth {
    pub title_ok: bool,
    pub meta_ok: bool,
    pub snippet_ok: bool,
    pub faq_ok: bool,
    pub file_found: bool,
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
        self.file_found && self.title_ok && self.meta_ok && self.snippet_ok && self.faq_ok
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
/// | file_found       | MDX file exists on disk                          |
/// | title_ok         | title.len() <= 55                                |
/// | meta_ok          | meta.len() >= 130 && meta.len() <= 155           |
/// | snippet_ok       | word_count >= 40 && word_count <= 60 && (has_keyword \|\| has '?') |
/// | faq_ok           | has_faq_schema == true                           |
pub const TITLE_MAX_LEN: usize = 55;
pub const META_MIN_LEN: usize = 130;
pub const META_MAX_LEN: usize = 155;
pub const SNIPPET_MIN_WORDS: usize = 40;
pub const SNIPPET_MAX_WORDS: usize = 60;

pub fn check_article_health(
    title: &str,
    meta: &str,
    first_paragraph: &str,
    target_keyword: &str,
    has_faq_schema: bool,
    file_found: bool,
) -> ArticleHealth {
    let title_ok = title.len() <= TITLE_MAX_LEN;
    let meta_ok = !meta.is_empty() && meta.len() >= META_MIN_LEN && meta.len() <= META_MAX_LEN;

    let snippet_word_count = first_paragraph.split_whitespace().count();
    let first_lower = first_paragraph.to_lowercase();
    let keyword_lower = target_keyword.to_lowercase();
    let snippet_has_keyword_or_question =
        keyword_lower.is_empty() || first_lower.contains(&keyword_lower) || first_paragraph.contains('?');
    let snippet_ok = snippet_word_count >= SNIPPET_MIN_WORDS && snippet_word_count <= SNIPPET_MAX_WORDS && snippet_has_keyword_or_question;

    let faq_ok = has_faq_schema;

    let mut issues = Vec::new();
    if !file_found {
        issues.push("file_not_found".to_string());
    }
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
        file_found,
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

/// Read an MDX file and extract (title, meta_description, first_paragraph, h1, has_faq_schema, file_found).
pub fn read_article_excerpt(project_path: &str, file_ref: &str) -> (String, String, String, String, bool, bool) {
    if file_ref.is_empty() {
        return (
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            false,
            false,
        );
    }

    let repo_root = Path::new(project_path);
    let full = match resolve_content_file(repo_root, file_ref) {
        Some(p) => p,
        None => {
            log::warn!("[audit_health] Could not resolve file: {}", file_ref);
            return (
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                false,
                false,
            );
        }
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
                false,
            );
        }
    };

    // Use frontmatter::split_mdx for canonical frontmatter/body split
    let (frontmatter_str, body) = match crate::content::frontmatter::split_mdx(&content) {
        Some((fm, b)) => (fm, b),
        None => ("", content.as_str()),
    };

    // Extract title and description from top-level scalar fields
    let scalars = crate::content::frontmatter::top_level_scalars(frontmatter_str);
    let title = scalars
        .iter()
        .find(|s| s.key == "title")
        .and_then(|s| {
            let v = s.raw_value.trim_matches('"').trim_matches('\'');
            if !v.is_empty() { Some(v.to_string()) } else { None }
        })
        .unwrap_or_default();
    let meta_description = scalars
        .iter()
        .find(|s| s.key == "description")
        .and_then(|s| {
            let v = s.raw_value.trim_matches('"').trim_matches('\'');
            if !v.is_empty() { Some(v.to_string()) } else { None }
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

    // Extract first_paragraph using the MDX-aware paragraph finder.
    // This skips imports, exports, JSX components, and headings.
    let first_paragraph = crate::content::cleaner::find_first_paragraph_range(body)
        .map(|(start, end)| body[start..end].to_string())
        .unwrap_or_default();

    let has_faq = has_faq_schema(&content);

    (title, meta_description, first_paragraph, h1, has_faq, true)
}

/// Check whether an MDX file contains FAQ schema (JSON-LD FAQPage, markdown FAQ section,
/// or frontmatter `faq:` YAML list).
pub fn has_faq_schema(content: &str) -> bool {
    // 1. Check frontmatter for `faq:` YAML list
    if let Some((fm_raw, _)) = crate::content::frontmatter::split_mdx(content) {
        if let Ok(fm) = crate::content::frontmatter::parse(fm_raw) {
            if let Some(faq) = fm.parsed.get("faq") {
                if faq.is_sequence() && !faq.as_sequence().unwrap().is_empty() {
                    return true;
                }
            }
        }
    }

    // 2. Check for JSON-LD FAQPage schema in body
    let content_lower = content.to_lowercase();
    if content_lower.contains("faqpage")
        || content_lower.contains("\"@type\": \"question\"")
        || content_lower.contains("'@type': 'question'")
        || content_lower.contains("\"@type\":\"question\"")
    {
        return true;
    }

    // 3. Check for markdown FAQ headings
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
