use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProjectMode {
    #[default]
    Workspace,
    LiveSite,
}

impl ProjectMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProjectMode::Workspace => "workspace",
            ProjectMode::LiveSite => "live_site",
        }
    }
}

impl std::fmt::Display for ProjectMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl rusqlite::types::ToSql for ProjectMode {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(rusqlite::types::ToSqlOutput::Owned(
            rusqlite::types::Value::Text(self.as_str().to_string()),
        ))
    }
}

impl rusqlite::types::FromSql for ProjectMode {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        let s = String::column_result(value)?;
        match s.as_str() {
            "workspace" => Ok(ProjectMode::Workspace),
            "live_site" => Ok(ProjectMode::LiveSite),
            other => Err(rusqlite::types::FromSqlError::Other(
                format!("unknown project mode: {other}").into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sitemap_url: Option<String>,
    #[serde(default)]
    pub project_mode: ProjectMode,
    pub active: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seo_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clarity_project_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub struct ProjectCreate {
    pub name: String,
    pub path: Option<String>,
    pub content_dir: Option<String>,
    pub site_url: Option<String>,
    pub site_id: Option<String>,
    pub sitemap_url: Option<String>,
    #[serde(default)]
    pub project_mode: ProjectMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clarity_project_id: Option<String>,
}

impl Project {
    /// Fetchable base URL for this project (see [`site_base_url`]), if a
    /// `site_url` is configured.
    pub fn site_base_url(&self) -> Option<String> {
        self.site_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(site_base_url)
    }
}

/// Convert a stored `site_url` into a fetchable base URL.
///
/// `projects.site_url` holds the Google Search Console property identifier,
/// which is not always a URL: domain properties are stored as
/// `sc-domain:example.com`. **Any code that fetches from the live site
/// (sitemaps, page fetches, canonical checks) must go through this function
/// instead of using `site_url` directly.** GSC API calls are the exception —
/// they require the raw property ID.
///
/// - `sc-domain:example.com`     → `https://example.com`
/// - `example.com`               → `https://example.com`
/// - `https://example.com/`      → `https://example.com`
/// - `https://example.com/blog/` → `https://example.com/blog`
///
/// The result never has a trailing slash. `www.` is preserved on purpose:
/// stripping it is a URL-comparison concern (see
/// `engine::exec::gsc::normalize_site_for_url_match`), not a fetch concern.
pub fn site_base_url(site_url: &str) -> String {
    let trimmed = site_url.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let without_gsc = trimmed.strip_prefix("sc-domain:").unwrap_or(trimmed);
    let with_scheme = if without_gsc.starts_with("http://") || without_gsc.starts_with("https://") {
        without_gsc.to_string()
    } else {
        format!("https://{}", without_gsc)
    };
    with_scheme.trim_end_matches('/').to_string()
}

/// Validate a `site_url` value at the write boundary.
///
/// Accepts an empty value, a GSC domain property (`sc-domain:<host>`), or an
/// `http(s)://` URL with a host. Rejects everything else so values that no
/// consumer could ever fetch never enter the database.
pub fn validate_site_url(site_url: &str) -> Result<(), String> {
    let trimmed = site_url.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    if let Some(host) = trimmed.strip_prefix("sc-domain:") {
        if host.is_empty() || host.contains(['/', ' ']) {
            return Err(format!(
                "invalid site_url '{trimmed}': expected sc-domain:<host> (e.g. sc-domain:example.com)"
            ));
        }
        return Ok(());
    }
    match url::Url::parse(trimmed) {
        Ok(u) if matches!(u.scheme(), "http" | "https") && u.host_str().is_some() => Ok(()),
        _ => Err(format!(
            "invalid site_url '{trimmed}': expected sc-domain:<host> or an http(s):// URL"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_base_url_strips_gsc_domain_prefix() {
        assert_eq!(site_base_url("sc-domain:example.com"), "https://example.com");
        assert_eq!(
            site_base_url("sc-domain:www.example.com"),
            "https://www.example.com",
            "www must be preserved for fetching"
        );
    }

    #[test]
    fn site_base_url_adds_scheme_and_strips_trailing_slash() {
        assert_eq!(site_base_url("example.com"), "https://example.com");
        assert_eq!(site_base_url("https://example.com/"), "https://example.com");
        assert_eq!(site_base_url("http://example.com"), "http://example.com");
        assert_eq!(
            site_base_url("https://example.com/blog/"),
            "https://example.com/blog"
        );
    }

    #[test]
    fn site_base_url_empty_stays_empty() {
        assert_eq!(site_base_url(""), "");
        assert_eq!(site_base_url("   "), "");
    }

    #[test]
    fn validate_site_url_accepts_gsc_and_urls() {
        assert!(validate_site_url("").is_ok());
        assert!(validate_site_url("sc-domain:example.com").is_ok());
        assert!(validate_site_url("https://example.com").is_ok());
        assert!(validate_site_url("http://example.com/blog").is_ok());
    }

    #[test]
    fn validate_site_url_rejects_unfetchable_values() {
        assert!(validate_site_url("sc-domain:").is_err());
        assert!(validate_site_url("sc-domain:example.com/blog").is_err());
        assert!(validate_site_url("not a url").is_err());
        assert!(validate_site_url("ftp://example.com").is_err());
    }
}
