use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One page × event × day conversion count (engine tape row).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PosthogPageDailyRow {
    pub page: String,
    pub event: String,
    pub date: String,
    pub count: f64,
}

/// Metadata for a conversion-tape collection run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PosthogCollectionMeta {
    pub project_id: String,
    /// PostHog project id from `project.yaml` (numeric string).
    pub posthog_project_id: String,
    pub exported_at: String,
    pub days: u8,
    pub events: Vec<String>,
    pub rows: usize,
}

/// Full collection artifact written to the repo automation dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PosthogCollection {
    pub meta: PosthogCollectionMeta,
    pub rows: Vec<PosthogPageDailyRow>,
}

/// Aggregated event totals for a page over a date window.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PosthogPageWindow {
    pub page: String,
    pub days_with_data: i64,
    pub events: HashMap<String, f64>,
    pub total: f64,
}

/// Default conversion events when config is missing or empty.
pub fn default_posthog_conversion_events() -> Vec<String> {
    vec![
        "signup_started".into(),
        "signup_completed".into(),
        "cta_clicked".into(),
    ]
}

/// Resolve events for collect: explicit non-empty list wins; empty → defaults.
pub fn resolve_conversion_events(configured: &[String]) -> Vec<String> {
    let filtered: Vec<String> = configured
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if filtered.is_empty() {
        default_posthog_conversion_events()
    } else {
        filtered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_empty_falls_back_to_defaults() {
        let resolved = resolve_conversion_events(&[]);
        assert_eq!(resolved, default_posthog_conversion_events());
    }

    #[test]
    fn resolve_whitespace_only_falls_back() {
        let resolved = resolve_conversion_events(&["".into(), "  ".into()]);
        assert_eq!(resolved, default_posthog_conversion_events());
    }

    #[test]
    fn resolve_keeps_explicit_list() {
        let resolved = resolve_conversion_events(&["purchase".into(), "trial".into()]);
        assert_eq!(resolved, vec!["purchase".to_string(), "trial".to_string()]);
    }
}
