//! Minimal reader for `.github/automation/seo_program.yaml`.
//!
//! Skills remain the writers/SOT for the program file. Rust only **reads**
//! `current_mode` (and enough schema to load safely) so `create-task` can emit
//! a warn-only off-mode signal. Never hard-refuses create.

use std::path::Path;

use serde::Deserialize;

/// Filename under `{project}/.github/automation/`.
pub const SEO_PROGRAM_FILE: &str = "seo_program.yaml";

/// Minimal program view — only fields needed for mode-aware warnings.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct SeoProgram {
    #[serde(default)]
    pub schema_version: Option<u32>,
    #[serde(default)]
    pub current_mode: Option<String>,
}

/// Mode family used for off-mode classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeFamily {
    Attract,
    Harvest,
    Tools,
    Measure,
}

impl ModeFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            ModeFamily::Attract => "attract",
            ModeFamily::Harvest => "harvest",
            ModeFamily::Tools => "tools",
            ModeFamily::Measure => "measure",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "attract" => Some(ModeFamily::Attract),
            "harvest" => Some(ModeFamily::Harvest),
            "tools" => Some(ModeFamily::Tools),
            "measure" => Some(ModeFamily::Measure),
            _ => None,
        }
    }
}

/// Preferred modes for a task type per `docs/SEO_PROGRAM.md` mode table.
/// Empty → unclassified → no warning. A type may belong to multiple modes
/// (e.g. `create_landing_page` is fine in attract and tools).
fn preferred_modes_for_task_type(task_type: &str) -> &'static [ModeFamily] {
    match task_type {
        // attract — write / research / publish / cluster
        "write_article"
        | "create_content"
        | "research_keywords"
        | "custom_keyword_research"
        | "research_landing_pages"
        | "territory_research"
        | "update_research_shortlist"
        | "publish_content"
        | "cluster_and_link"
        | "interlinking"
        | "create_hub_page" => &[ModeFamily::Attract],

        // attract + tools
        "create_landing_page" => &[ModeFamily::Attract, ModeFamily::Tools],

        // harvest — fix / bridge / consolidate
        "fix_content_article"
        | "fix_ctr_article"
        | "fix_content"
        | "optimize_article"
        | "optimize_content"
        | "consolidate_cluster"
        | "fix_indexing_internal_links"
        | "content_cleanup" => &[ModeFamily::Harvest],

        // tools — commercial / calculator pages
        "calculator_rollout" => &[ModeFamily::Tools],

        // measure — audits / outcomes / GSC / reviews (side-pass: never warn)
        "content_outcome_review"
        | "ctr_outcome_review"
        | "gsc_indexing_outcome_review"
        | "content_audit"
        | "ctr_audit"
        | "cannibalization_audit"
        | "collect_gsc"
        | "analyze_gsc_performance"
        | "seo_health_scan"
        | "review_article_quality"
        | "content_review" => &[ModeFamily::Measure],

        _ => &[],
    }
}

/// Primary family for messaging when off-mode (first preferred mode).
fn primary_family_for_task_type(task_type: &str) -> Option<ModeFamily> {
    preferred_modes_for_task_type(task_type).first().copied()
}

/// Load `seo_program.yaml` from `{project_path}/.github/automation/`.
///
/// Returns `None` when the file is absent or unreadable/invalid (soft — never
/// fails create-task).
pub fn load_seo_program(project_path: &Path) -> Option<SeoProgram> {
    let path = project_path
        .join(".github")
        .join("automation")
        .join(SEO_PROGRAM_FILE);
    if !path.is_file() {
        return None;
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("seo_program: failed to read {}: {e}", path.display());
            return None;
        }
    };
    match serde_yaml::from_str::<SeoProgram>(&text) {
        Ok(prog) => Some(prog),
        Err(e) => {
            log::warn!("seo_program: invalid YAML at {}: {e}", path.display());
            None
        }
    }
}

/// Warn-only signal when `task_type` is classified into mode families that do
/// not include `current_mode`. Unclassified types, measure types, and missing
/// files → `None` (never blocks create).
pub fn off_mode_create_warning(project_path: &Path, task_type: &str) -> Option<String> {
    let program = load_seo_program(project_path)?;
    let mode_str = program.current_mode.as_deref()?.trim();
    if mode_str.is_empty() {
        return None;
    }
    let current = ModeFamily::parse(mode_str)?;
    let preferred = preferred_modes_for_task_type(task_type);
    if preferred.is_empty() {
        return None; // unclassified → no warning
    }
    // Measure types are a mandatory weekly side-pass in every mode — no warn.
    if preferred.contains(&ModeFamily::Measure) {
        return None;
    }
    if preferred.contains(&current) {
        return None;
    }
    let prefer = primary_family_for_task_type(task_type)?.as_str();
    Some(format!(
        "Task type '{task_type}' is outside current_mode '{}' (prefer {prefer} work in {prefer} mode). Create still succeeded; check seo_program.yaml.",
        current.as_str(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn write_program(dir: &Path, yaml: &str) {
        let automation = dir.join(".github").join("automation");
        std::fs::create_dir_all(&automation).unwrap();
        std::fs::write(automation.join(SEO_PROGRAM_FILE), yaml).unwrap();
    }

    fn unique_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("seo_program_test_{nanos}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".github").join("automation")).unwrap();
        dir
    }

    #[test]
    fn parse_sample_yaml_with_current_mode() {
        let dir = unique_dir();
        write_program(
            &dir,
            "schema_version: 1\ncurrent_mode: harvest\ngoal: ship CTAs\nunknown_field: ignored\n",
        );
        let prog = load_seo_program(&dir).expect("should load");
        assert_eq!(prog.schema_version, Some(1));
        assert_eq!(prog.current_mode.as_deref(), Some("harvest"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn off_mode_warning_on_mismatch() {
        let dir = unique_dir();
        write_program(&dir, "schema_version: 1\ncurrent_mode: attract\n");
        let w = off_mode_create_warning(&dir, "fix_content_article");
        assert!(w.is_some(), "expected warning");
        let msg = w.unwrap();
        assert!(msg.contains("fix_content_article"), "{msg}");
        assert!(msg.contains("attract"), "{msg}");
        assert!(msg.contains("harvest"), "{msg}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_warning_when_file_missing() {
        let dir = unique_dir();
        assert!(off_mode_create_warning(&dir, "fix_content_article").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_warning_when_type_matches_mode() {
        let dir = unique_dir();
        write_program(&dir, "schema_version: 1\ncurrent_mode: attract\n");
        assert!(off_mode_create_warning(&dir, "write_article").is_none());
        assert!(off_mode_create_warning(&dir, "research_keywords").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_warning_for_unclassified_types() {
        let dir = unique_dir();
        write_program(&dir, "schema_version: 1\ncurrent_mode: attract\n");
        assert!(off_mode_create_warning(&dir, "reddit_opportunity_search").is_none());
        assert!(off_mode_create_warning(&dir, "some_future_type").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn measure_types_never_warn_off_mode() {
        let dir = unique_dir();
        write_program(&dir, "schema_version: 1\ncurrent_mode: attract\n");
        assert!(off_mode_create_warning(&dir, "content_outcome_review").is_none());
        assert!(off_mode_create_warning(&dir, "collect_gsc").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_yaml_returns_none() {
        let dir = unique_dir();
        write_program(&dir, "schema_version: [\nnot: valid\n");
        assert!(load_seo_program(&dir).is_none());
        assert!(off_mode_create_warning(&dir, "write_article").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every task type classified by `preferred_modes_for_task_type` must still
    /// exist in `task_definitions::DEFINITIONS` (drift guard for #319).
    #[test]
    fn preferred_modes_types_exist_in_task_definitions() {
        let classified = [
            // attract
            "write_article",
            "create_content",
            "research_keywords",
            "custom_keyword_research",
            "research_landing_pages",
            "territory_research",
            "update_research_shortlist",
            "publish_content",
            "cluster_and_link",
            "interlinking",
            "create_hub_page",
            // attract + tools
            "create_landing_page",
            // harvest
            "fix_content_article",
            "fix_ctr_article",
            "fix_content",
            "optimize_article",
            "optimize_content",
            "consolidate_cluster",
            "fix_indexing_internal_links",
            "content_cleanup",
            // tools
            "calculator_rollout",
            // measure
            "content_outcome_review",
            "ctr_outcome_review",
            "gsc_indexing_outcome_review",
            "content_audit",
            "ctr_audit",
            "cannibalization_audit",
            "collect_gsc",
            "analyze_gsc_performance",
            "seo_health_scan",
            "review_article_quality",
            "content_review",
        ];
        for tt in classified {
            assert!(
                !preferred_modes_for_task_type(tt).is_empty(),
                "expected non-empty preferred modes for classified type '{tt}'"
            );
            assert!(
                crate::config::task_definitions::find(tt).is_some(),
                "preferred_modes classifies '{tt}' but task_definitions has no entry — map drift"
            );
        }
        // Unclassified product types stay empty (no false-positive mode warnings).
        for def in crate::config::task_definitions::all() {
            let modes = preferred_modes_for_task_type(def.task_type);
            // If non-empty, primary family must be well-formed.
            if !modes.is_empty() {
                assert!(
                    primary_family_for_task_type(def.task_type).is_some(),
                    "non-empty preferred modes without primary for {}",
                    def.task_type
                );
            }
        }
    }
}

