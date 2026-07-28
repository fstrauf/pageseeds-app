//! Artifact parsing helpers for the keyword research pipeline.

use super::*;
use crate::models::task::Task;

pub(crate) fn parse_seed_extraction_artifact(task: &Task) -> SeedArtifact {
    let content = task
        .artifacts
        .iter()
        .rev()
        .find(|a| a.key == "research_seed_extraction")
        .and_then(|a| a.content.as_deref());

    let Some(raw) = content else {
        return SeedArtifact::default();
    };

    // Try to parse as JSON first
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(raw) {
        let themes = themes_from_json(&json);
        let competitors = competitors_from_json(&json);
        if !themes.is_empty() || !competitors.is_empty() {
            return SeedArtifact {
                themes,
                competitors,
            };
        }
    }

    // Fallback: extract JSON from fenced blocks or bare JSON
    if let Some(json) = crate::engine::text::extract_json(raw) {
        let themes = themes_from_json(&json);
        let competitors = competitors_from_json(&json);
        if !themes.is_empty() || !competitors.is_empty() {
            return SeedArtifact {
                themes,
                competitors,
            };
        }
    }

    SeedArtifact::default()
}

pub(crate) fn themes_from_json(v: &serde_json::Value) -> Vec<String> {
    let from_array = |arr: &[serde_json::Value]| {
        arr.iter()
            .filter_map(|x| x.as_str())
            .filter_map(super::clean_theme_str)
            .collect::<Vec<String>>()
    };

    // Accept either object-based or array-based contracts.
    if let Some(arr) = v.as_array() {
        return from_array(arr);
    }

    for key in ["themes", "selected_themes", "keyword_themes"] {
        if let Some(arr) = v.get(key).and_then(|x| x.as_array()) {
            return from_array(arr);
        }
    }

    vec![]
}

pub(crate) fn competitors_from_json(v: &serde_json::Value) -> Vec<String> {
    let extract = |arr: &[serde_json::Value]| {
        arr.iter()
            .filter_map(|x| x.as_str())
            .map(|s| {
                s.trim()
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .split('/')
                    .next()
                    .unwrap_or(s)
                    .to_string()
            })
            .filter(|s| !s.is_empty() && s.contains('.'))
            .collect::<Vec<String>>()
    };

    if let Some(arr) = v.get("competitors").and_then(|x| x.as_array()) {
        return extract(arr);
    }

    vec![]
}

/// Tri-state result of parsing the `research_seed_validation` artifact.
///
/// The distinction matters: `Missing` means validation never produced usable
/// output, so falling back to raw themes is safe. `RejectedAll` means the
/// validation step ran successfully and condemned every theme — falling back
/// to the raw themes would bill the user for seeds the gate just rejected.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ValidatedSeeds {
    /// Artifact absent, unparseable as JSON, or no `validated_seeds` array.
    Missing,
    /// Artifact parsed fine with a `validated_seeds` array, but yields zero
    /// (theme, seed) pairs — the validator rejected every extracted theme.
    RejectedAll,
    /// Flat list of `(theme, seed)` pairs ready for DataForSEO calls.
    Seeds(Vec<(String, String)>),
}

/// Parse the `research_seed_validation` artifact.
///
/// Expected artifact format:
/// `{validated_seeds: [{theme: string, seeds: [string]}]}`
pub(crate) fn parse_validated_seeds_artifact(task: &Task) -> ValidatedSeeds {
    let content = task
        .artifacts
        .iter()
        .rev()
        .find(|a| a.key == "research_seed_validation")
        .and_then(|a| a.content.as_deref());

    let Some(raw) = content else {
        return ValidatedSeeds::Missing;
    };

    // Try direct JSON parse first, then extract_json helper
    let json = serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .or_else(|| crate::engine::text::extract_json(raw));

    let Some(json) = json else {
        return ValidatedSeeds::Missing;
    };

    let validated = json.get("validated_seeds").and_then(|v| v.as_array());

    let Some(validated) = validated else {
        return ValidatedSeeds::Missing;
    };

    let mut pairs: Vec<(String, String)> = vec![];
    for entry in validated {
        let theme = entry
            .get("theme")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        if theme.is_empty() {
            continue;
        }
        let seeds = entry.get("seeds").and_then(|s| s.as_array());
        if let Some(seeds) = seeds {
            for seed in seeds {
                if let Some(s) = seed.as_str() {
                    let s = s.trim();
                    if !s.is_empty() {
                        pairs.push((theme.clone(), s.to_string()));
                    }
                }
            }
        }
    }
    if pairs.is_empty() {
        ValidatedSeeds::RejectedAll
    } else {
        ValidatedSeeds::Seeds(pairs)
    }
}

pub(crate) fn read_pending_shortlist(task: &Task) -> Vec<crate::db::research_shortlist::ResearchShortlistEntry> {
    let db_path = crate::db::default_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[keyword_research_native] Failed to open DB for shortlist: {}", e);
            return Vec::new();
        }
    };
    match crate::db::research_shortlist::list_pending_excluding_depleted(&conn, &task.project_id) {
        Ok(entries) => {
            let strategy = crate::strategy::load_for_project(&conn, &task.project_id);
            let filtered = filter_pending_shortlist_by_strategy(entries, &strategy);
            log::info!(
                "[keyword_research_native] loaded {} pending shortlist entries (depleted + strategy-blocked filtered)",
                filtered.len()
            );
            filtered
        }
        Err(e) => {
            log::warn!("[keyword_research_native] Failed to read shortlist: {}", e);
            Vec::new()
        }
    }
}

/// Drop themes/seeds hard-blocked by live content strategy (issue #258).
///
/// Uses [`crate::strategy::strategy_blocks_expansion`] so produce, consume, and
/// final selection share one policy. Empty strategy → no-op (full shortlist).
pub(crate) fn filter_pending_shortlist_by_strategy(
    entries: Vec<crate::db::research_shortlist::ResearchShortlistEntry>,
    strategy: &crate::strategy::ProjectStrategy,
) -> Vec<crate::db::research_shortlist::ResearchShortlistEntry> {
    if strategy.is_empty() {
        return entries;
    }

    let before = entries.len();
    let mut kept = Vec::with_capacity(entries.len());
    let mut themes_skipped = 0usize;
    let mut seeds_skipped = 0usize;

    for mut entry in entries {
        if crate::strategy::strategy_blocks_expansion(&entry.theme, strategy) {
            themes_skipped += 1;
            continue;
        }
        let seed_before = entry.seeds.len();
        entry
            .seeds
            .retain(|s| !crate::strategy::strategy_blocks_expansion(s, strategy));
        seeds_skipped += seed_before.saturating_sub(entry.seeds.len());
        kept.push(entry);
    }

    if themes_skipped > 0 || seeds_skipped > 0 {
        log::info!(
            "[keyword_research_native] shortlist_strategy_skipped themes={} seeds={} (kept {} of {} pending entries)",
            themes_skipped,
            seeds_skipped,
            kept.len(),
            before
        );
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::research_shortlist::ResearchShortlistEntry;
    use crate::strategy::{ClusterStatus, ProjectStrategy, StrategyCluster};

    fn entry(theme: &str, seeds: &[&str]) -> ResearchShortlistEntry {
        ResearchShortlistEntry::new(
            "proj1",
            theme,
            seeds.iter().map(|s| s.to_string()).collect(),
            "test",
            "medium",
            None,
            None,
        )
    }

    fn fixture_strategy() -> ProjectStrategy {
        ProjectStrategy {
            do_not_expand: vec!["custom web design".to_string()],
            clusters: vec![
                StrategyCluster {
                    name: "SEO Fundamentals".to_string(),
                    status: ClusterStatus::Active,
                    keywords: vec!["technical seo".to_string()],
                },
                StrategyCluster {
                    name: "Old Services".to_string(),
                    status: ClusterStatus::Legacy,
                    keywords: vec!["web design packages".to_string()],
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn filter_drops_legacy_and_do_not_expand_themes() {
        let strategy = fixture_strategy();
        let entries = vec![
            entry("technical seo", &["technical seo checklist"]),
            entry("web design packages", &["web design packages pricing"]),
            entry("custom web design", &["custom web design agency"]),
            entry("unrelated theme", &["random seed"]),
        ];
        let kept = filter_pending_shortlist_by_strategy(entries, &strategy);
        let themes: Vec<&str> = kept.iter().map(|e| e.theme.as_str()).collect();
        assert_eq!(themes, vec!["technical seo", "unrelated theme"]);
    }

    #[test]
    fn filter_strips_blocked_seeds_within_allowed_theme() {
        let strategy = fixture_strategy();
        let entries = vec![entry(
            "mixed theme",
            &[
                "technical seo tips",
                "web design packages guide",
                "custom web design near me",
                "ok seed",
            ],
        )];
        let kept = filter_pending_shortlist_by_strategy(entries, &strategy);
        assert_eq!(kept.len(), 1);
        assert_eq!(
            kept[0].seeds,
            vec!["technical seo tips".to_string(), "ok seed".to_string()]
        );
    }

    #[test]
    fn filter_empty_strategy_keeps_full_shortlist() {
        let strategy = ProjectStrategy::default();
        let entries = vec![
            entry("web design packages", &["web design packages"]),
            entry("custom web design", &["custom web design"]),
        ];
        let kept = filter_pending_shortlist_by_strategy(entries, &strategy);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].seeds.len(), 1);
        assert_eq!(kept[1].seeds.len(), 1);
    }
}
