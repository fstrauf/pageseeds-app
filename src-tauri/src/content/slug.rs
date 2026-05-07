/// Centralized URL slug utilities.
///
/// This module is the single source of truth for slug extraction, normalization,
/// validation, and link formatting. All code that works with `url_slug` values
/// should go through here instead of re-implementing ad-hoc rules.
use regex::Regex;
use std::sync::OnceLock;

// ═══════════════════════════════════════════════════════════════════════════════
// Shared regexes (compiled once)
// ═══════════════════════════════════════════════════════════════════════════════

fn numeric_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d+[_\-]+").unwrap())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

/// Detectable issues in a url_slug value.
#[derive(Debug, Clone, PartialEq)]
pub enum SlugIssue {
    Empty,
    LeadingSlash,
    TrailingSlash,
    ContainsPathSeparator,
    HasBlogPrefix,
    HasNumericPrefix,
    ContainsUnderscores,
    ContainsSpaces,
    UppercaseCharacters,
}

impl std::fmt::Display for SlugIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SlugIssue::Empty => write!(f, "slug is empty"),
            SlugIssue::LeadingSlash => write!(f, "slug has a leading slash"),
            SlugIssue::TrailingSlash => write!(f, "slug has a trailing slash"),
            SlugIssue::ContainsPathSeparator => {
                write!(f, "slug contains path separators (should be a single segment)")
            }
            SlugIssue::HasBlogPrefix => {
                write!(f, "slug has a 'blog/' prefix (should be stripped)")
            }
            SlugIssue::HasNumericPrefix => {
                write!(f, "slug has a leading numeric prefix like '001_'")
            }
            SlugIssue::ContainsUnderscores => {
                write!(f, "slug contains underscores (use dashes)")
            }
            SlugIssue::ContainsSpaces => {
                write!(f, "slug contains spaces (use dashes)")
            }
            SlugIssue::UppercaseCharacters => {
                write!(f, "slug contains uppercase characters (use lowercase)")
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Normalization
// ═══════════════════════════════════════════════════════════════════════════════

/// Canonical slug normalizer.
///
/// Applies the following rules in order:
/// 1. Trim whitespace
/// 2. Strip leading/trailing slashes
/// 3. Take the final path segment (`blog/my-post` → `my-post`)
/// 4. Strip leading numeric prefix (`001_` or `001-`)
/// 5. Replace underscores with dashes
/// 6. Lowercase
///
/// # Examples
///
/// ```
/// use pageseeds_lib::content::slug::normalize_url_slug;
///
/// assert_eq!(normalize_url_slug("my-post"), "my-post");
/// assert_eq!(normalize_url_slug("blog/my-post"), "my-post");
/// assert_eq!(normalize_url_slug("/blog/my-post"), "my-post");
/// assert_eq!(normalize_url_slug("001_my_post"), "my-post");
/// assert_eq!(normalize_url_slug("My_Post"), "my-post");
/// ```
pub fn normalize_url_slug(slug: &str) -> String {
    let slug = slug.trim();
    let slug = slug.trim_start_matches('/').trim_end_matches('/');
    let slug = slug.rsplit('/').next().unwrap_or(slug);
    // Strip ALL leading numeric prefixes (e.g. 2025-08-01- → "")
    let re = numeric_prefix_re();
    let mut slug = slug.to_string();
    while re.is_match(&slug) {
        let next = re.replace(&slug, "").to_string();
        if next == slug {
            break;
        }
        slug = next;
    }
    slug.replace('_', "-").replace(' ', "-").to_lowercase()
}

/// Strip a leading numeric prefix from a file stem.
///
/// Handles both `_` and `-` separators: `001_my-post` → `my-post`.
pub fn strip_numeric_prefix(stem: &str) -> String {
    numeric_prefix_re().replace(stem, "").to_string()
}

// ═══════════════════════════════════════════════════════════════════════════════
// URL extraction
// ═══════════════════════════════════════════════════════════════════════════════

/// Extract a clean slug from a full URL.
///
/// Strips scheme (`https://`, `http://`) and `www.`, then takes the path and
/// runs it through [`normalize_url_slug`].
///
/// # Examples
///
/// ```
/// use pageseeds_lib::content::slug::extract_slug_from_url;
///
/// assert_eq!(extract_slug_from_url("https://example.com/blog/my-post"), "my-post");
/// assert_eq!(extract_slug_from_url("https://example.com/my-post/"), "my-post");
/// assert_eq!(extract_slug_from_url("https://www.example.com/blog/001_hello_world"), "hello-world");
/// assert_eq!(extract_slug_from_url("https://example.com/"), "");
/// ```
pub fn extract_slug_from_url(url: &str) -> String {
    let without_scheme = url
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let without_www = without_scheme.trim_start_matches("www.");

    let path = if let Some(pos) = without_www.find('/') {
        &without_www[pos..]
    } else {
        ""
    };

    normalize_url_slug(path)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Validation
// ═══════════════════════════════════════════════════════════════════════════════

/// Check a slug for common drift issues.
///
/// Returns a list of [`SlugIssue`]s. An empty list means the slug is clean.
///
/// # Examples
///
/// ```
/// use pageseeds_lib::content::slug::{validate_url_slug, SlugIssue};
///
/// let issues = validate_url_slug("blog/my_post");
/// assert!(issues.contains(&SlugIssue::HasBlogPrefix));
/// assert!(issues.contains(&SlugIssue::ContainsUnderscores));
///
/// assert!(validate_url_slug("my-post").is_empty());
/// ```
pub fn validate_url_slug(slug: &str) -> Vec<SlugIssue> {
    let mut issues = Vec::new();

    let trimmed = slug.trim();
    if trimmed.is_empty() {
        issues.push(SlugIssue::Empty);
        return issues;
    }

    if slug.starts_with('/') {
        issues.push(SlugIssue::LeadingSlash);
    }
    if slug.ends_with('/') {
        issues.push(SlugIssue::TrailingSlash);
    }
    if slug.contains('/') {
        issues.push(SlugIssue::ContainsPathSeparator);
    }
    if slug.starts_with("blog/") || slug.starts_with("/blog/") {
        issues.push(SlugIssue::HasBlogPrefix);
    }
    if numeric_prefix_re().is_match(slug) {
        issues.push(SlugIssue::HasNumericPrefix);
    }
    if slug.contains('_') {
        issues.push(SlugIssue::ContainsUnderscores);
    }
    if slug.contains(' ') {
        issues.push(SlugIssue::ContainsSpaces);
    }
    if slug.chars().any(|c| c.is_uppercase()) {
        issues.push(SlugIssue::UppercaseCharacters);
    }

    issues
}

/// Return `true` if the slug has any validation issues.
pub fn is_slug_dirty(slug: &str) -> bool {
    !validate_url_slug(slug).is_empty()
}

// ═══════════════════════════════════════════════════════════════════════════════
// Link formatting
// ═══════════════════════════════════════════════════════════════════════════════

/// Format a slug as an internal blog link (`/blog/{slug}`).
///
/// The slug is normalized before formatting. In debug builds, panics if the
/// normalized slug still contains path separators (indicates a caller bug).
///
/// # Examples
///
/// ```
/// use pageseeds_lib::content::slug::format_blog_link;
///
/// assert_eq!(format_blog_link("my-post"), "/blog/my-post");
/// assert_eq!(format_blog_link("blog/my-post"), "/blog/my-post");
/// assert_eq!(format_blog_link("/blog/my-post/"), "/blog/my-post");
/// ```
pub fn format_blog_link(slug: &str) -> String {
    let normalized = normalize_url_slug(slug);
    debug_assert!(
        !normalized.contains('/'),
        "blog link slug must not contain path separators after normalization: {}",
        normalized
    );
    format!("/blog/{}", normalized)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_url_slug_basic() {
        assert_eq!(normalize_url_slug("my-post"), "my-post");
        assert_eq!(normalize_url_slug("blog/my-post"), "my-post");
        assert_eq!(normalize_url_slug("/blog/my-post"), "my-post");
        assert_eq!(normalize_url_slug("tools/blog/my-post"), "my-post");
    }

    #[test]
    fn normalize_url_slug_cleans_formatting() {
        assert_eq!(normalize_url_slug("001_my_post"), "my-post");
        assert_eq!(normalize_url_slug("001-my-post"), "my-post");
        assert_eq!(normalize_url_slug("My_Post"), "my-post");
        assert_eq!(normalize_url_slug("MY POST"), "my-post");
        assert_eq!(normalize_url_slug("  /blog/my-post/  "), "my-post");
    }

    #[test]
    fn extract_slug_from_url_variants() {
        assert_eq!(
            extract_slug_from_url("https://example.com/blog/my-post"),
            "my-post"
        );
        assert_eq!(
            extract_slug_from_url("http://www.example.com/my-post/"),
            "my-post"
        );
        assert_eq!(extract_slug_from_url("https://example.com/"), "");
        assert_eq!(
            extract_slug_from_url("https://example.com/blog/001_hello_world"),
            "hello-world"
        );
    }

    #[test]
    fn validate_url_slug_detects_issues() {
        assert_eq!(validate_url_slug("my-post"), vec![]);

        let issues = validate_url_slug("blog/my_post");
        assert!(issues.contains(&SlugIssue::HasBlogPrefix));
        assert!(issues.contains(&SlugIssue::ContainsPathSeparator));
        assert!(issues.contains(&SlugIssue::ContainsUnderscores));
    }

    #[test]
    fn format_blog_link_normalizes() {
        assert_eq!(format_blog_link("my-post"), "/blog/my-post");
        assert_eq!(format_blog_link("blog/my-post"), "/blog/my-post");
        assert_eq!(format_blog_link("/blog/my-post/"), "/blog/my-post");
    }

    #[test]
    fn strip_numeric_prefix_variants() {
        assert_eq!(strip_numeric_prefix("001_my_post"), "my_post");
        assert_eq!(strip_numeric_prefix("001-my-post"), "my-post");
        assert_eq!(strip_numeric_prefix("my-post"), "my-post");
    }
}
