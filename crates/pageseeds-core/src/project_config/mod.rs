//! Typed, versioned on-disk schema for `.github/automation/project.yaml`.
//!
//! Load/save, conversions, deterministic MD→YAML migrator ([`migrate`]), and
//! runtime ensure ([`ensure_project_config`]) which auto-migrates legacy MD on
//! first need. Strategy and Reddit pipelines use ensure for structured knobs
//! (YAML SOT; legacy MD is migration source only).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::reddit::config::MentionStance;
use crate::strategy::{ProjectStrategy, StrategyCluster};

pub mod ensure;
pub mod migrate;

pub use ensure::{ensure_project_config, EnsureAction};
pub use migrate::{
    migrate_project_config, project_config_status, LegacySourcesStatus, MigrateAction,
    MigrateOpts, MigrateReport, MigrateSources, ProjectConfigFieldCounts, ProjectConfigFormat,
    ProjectConfigStatus,
};

// ─── Constants ───────────────────────────────────────────────────────────────

/// Only schema_version accepted by this crate version.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

/// Filename under the automation directory.
pub const PROJECT_CONFIG_FILE: &str = "project.yaml";

/// `{automation_dir}/project.yaml`
pub fn project_config_path(automation_dir: &Path) -> PathBuf {
    automation_dir.join(PROJECT_CONFIG_FILE)
}

// ─── Types ───────────────────────────────────────────────────────────────────

/// Versioned project configuration (schema v1).
///
/// Unknown YAML keys are ignored (no `deny_unknown_fields`). Optional sections
/// use `#[serde(default)]`; `schema_version` is required on load.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectConfig {
    pub schema_version: u32,
    #[serde(default)]
    pub product_name: Option<String>,
    #[serde(default)]
    pub search_keywords: SearchKeywords,
    #[serde(default)]
    pub clusters: Vec<StrategyCluster>,
    #[serde(default)]
    pub reddit: ProjectRedditConfig,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            schema_version: SUPPORTED_SCHEMA_VERSION,
            product_name: None,
            search_keywords: SearchKeywords::default(),
            clusters: Vec::new(),
            reddit: ProjectRedditConfig::default(),
        }
    }
}

/// Search-keyword buckets (maps to [`ProjectStrategy`] keyword fields).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchKeywords {
    #[serde(default)]
    pub primary: Vec<String>,
    #[serde(default)]
    pub problem: Vec<String>,
    #[serde(default)]
    pub audience: Vec<String>,
    #[serde(default)]
    pub do_not_expand: Vec<String>,
}

/// Reddit block inside `project.yaml` (superset of MD-derived fields).
///
/// Includes `query_keywords` for future search assembly (#293). Does not
/// depend on `engine::exec`.
///
/// **Subreddit wire invariant:** `seed_subreddits` and `excluded_subreddits`
/// are bare lowercase names without an `r/` prefix (e.g. `"seo"`, not
/// `"r/SEO"`). Matches MD runtime normalization in `reddit/config.rs`
/// (`trim_start_matches("r/")` + `to_lowercase`). Load and save both
/// normalize so dual shapes never re-enter the SOT.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectRedditConfig {
    #[serde(default)]
    pub mention_stance: MentionStance,
    #[serde(default)]
    pub seed_subreddits: Vec<String>,
    #[serde(default)]
    pub excluded_subreddits: Vec<String>,
    #[serde(default)]
    pub trigger_topics: Vec<String>,
    /// Required field; empty list when absent.
    #[serde(default)]
    pub query_keywords: Vec<String>,
}

impl Default for ProjectRedditConfig {
    fn default() -> Self {
        Self {
            mention_stance: MentionStance::Optional,
            seed_subreddits: Vec::new(),
            excluded_subreddits: Vec::new(),
            trigger_topics: Vec::new(),
            query_keywords: Vec::new(),
        }
    }
}

// ─── Load / save ─────────────────────────────────────────────────────────────

/// Load and validate `project.yaml` from `path`.
///
/// - Missing file → [`Error::ConfigMissing`]
/// - Unreadable → [`Error::Other`] with path context
/// - Invalid YAML / type mismatch / missing `schema_version` → [`Error::Validation`]
/// - `schema_version` ≠ [`SUPPORTED_SCHEMA_VERSION`] → hard [`Error::Validation`]
///
/// Subreddit lists are normalized to bare lowercase (no `r/` prefix) after
/// deserialize — see [`ProjectRedditConfig`].
pub fn load_project_config(path: &Path) -> Result<ProjectConfig> {
    if !path.exists() {
        return Err(Error::ConfigMissing(path.display().to_string()));
    }

    let raw = std::fs::read_to_string(path).map_err(|e| {
        Error::Other(format!(
            "failed to read project config {}: {e}",
            path.display()
        ))
    })?;

    let mut config: ProjectConfig = serde_yaml::from_str(&raw).map_err(|e| {
        Error::Validation(format!(
            "invalid project config {}: {e}",
            path.display()
        ))
    })?;

    validate_schema_version(config.schema_version)?;
    config.normalize_subreddits();

    Ok(config)
}

/// Serialize and write `project.yaml`, creating parent directories if needed.
///
/// Subreddit lists are normalized to bare lowercase before write so the
/// on-disk form matches the load invariant.
pub fn save_project_config(path: &Path, config: &ProjectConfig) -> Result<()> {
    validate_schema_version(config.schema_version)?;

    let mut config = config.clone();
    config.normalize_subreddits();

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                Error::Other(format!(
                    "failed to create project config directory {}: {e}",
                    parent.display()
                ))
            })?;
        }
    }

    let raw = serde_yaml::to_string(&config).map_err(|e| {
        Error::Validation(format!("failed to serialize project config: {e}"))
    })?;

    std::fs::write(path, raw).map_err(|e| {
        Error::Other(format!(
            "failed to write project config {}: {e}",
            path.display()
        ))
    })?;

    Ok(())
}

fn validate_schema_version(version: u32) -> Result<()> {
    if version != SUPPORTED_SCHEMA_VERSION {
        return Err(Error::Validation(format!(
            "unsupported project.yaml schema_version {version} (supported: {SUPPORTED_SCHEMA_VERSION})"
        )));
    }
    Ok(())
}

/// Canonical subreddit form: bare lowercase, no `r/` prefix.
/// Matches MD runtime in `reddit/config.rs` (`extract_subreddits`); lowercases
/// first so `R/` is handled the same as `r/`.
fn normalize_subreddit_name(name: &str) -> String {
    name.trim().to_lowercase().trim_start_matches("r/").to_string()
}

// ─── Conversions ─────────────────────────────────────────────────────────────

impl ProjectConfig {
    /// Normalize seed/excluded subreddit lists in place.
    fn normalize_subreddits(&mut self) {
        self.reddit.seed_subreddits = self
            .reddit
            .seed_subreddits
            .iter()
            .map(|s| normalize_subreddit_name(s))
            .filter(|s| !s.is_empty())
            .collect();
        self.reddit.excluded_subreddits = self
            .reddit
            .excluded_subreddits
            .iter()
            .map(|s| normalize_subreddit_name(s))
            .filter(|s| !s.is_empty())
            .collect();
    }

    /// Lossless map of keyword lists + clusters into [`ProjectStrategy`].
    pub fn to_strategy(&self) -> ProjectStrategy {
        ProjectStrategy::from(self)
    }

    /// Build a v1 config from strategy lists/clusters (empty reddit, no product name).
    ///
    /// Intended for migrator assembly from existing MD strategy.
    pub fn from_strategy(strategy: &ProjectStrategy) -> Self {
        Self {
            schema_version: SUPPORTED_SCHEMA_VERSION,
            product_name: None,
            search_keywords: SearchKeywords::from(strategy),
            clusters: strategy.clusters.clone(),
            reddit: ProjectRedditConfig::default(),
        }
    }
}

impl From<&ProjectConfig> for ProjectStrategy {
    fn from(cfg: &ProjectConfig) -> Self {
        ProjectStrategy {
            primary_keywords: cfg.search_keywords.primary.clone(),
            problem_keywords: cfg.search_keywords.problem.clone(),
            audience_keywords: cfg.search_keywords.audience.clone(),
            do_not_expand: cfg.search_keywords.do_not_expand.clone(),
            clusters: cfg.clusters.clone(),
        }
    }
}

impl From<ProjectConfig> for ProjectStrategy {
    fn from(cfg: ProjectConfig) -> Self {
        ProjectStrategy::from(&cfg)
    }
}

impl From<&ProjectStrategy> for SearchKeywords {
    fn from(s: &ProjectStrategy) -> Self {
        SearchKeywords {
            primary: s.primary_keywords.clone(),
            problem: s.problem_keywords.clone(),
            audience: s.audience_keywords.clone(),
            do_not_expand: s.do_not_expand.clone(),
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::ClusterStatus;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("ps_project_config_{tag}_{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn full_fixture() -> ProjectConfig {
        ProjectConfig {
            schema_version: 1,
            product_name: Some("PageSeeds".into()),
            search_keywords: SearchKeywords {
                primary: vec!["seo automation".into()],
                problem: vec!["manual seo workflows".into()],
                audience: vec!["seo operators".into()],
                do_not_expand: vec!["web design packages".into()],
            },
            clusters: vec![
                StrategyCluster {
                    name: "SEO Fundamentals".into(),
                    status: ClusterStatus::Active,
                    keywords: vec!["on-page seo".into()],
                },
                StrategyCluster {
                    name: "Alternatives".into(),
                    status: ClusterStatus::Maintain,
                    keywords: vec![],
                },
                StrategyCluster {
                    name: "Services".into(),
                    status: ClusterStatus::Legacy,
                    keywords: vec!["custom services".into()],
                },
                StrategyCluster {
                    name: "New Pillar".into(),
                    status: ClusterStatus::Planned,
                    keywords: vec![],
                },
            ],
            reddit: ProjectRedditConfig {
                mention_stance: MentionStance::Required,
                // Canonical wire form: bare lowercase, no r/ prefix
                seed_subreddits: vec!["seo".into()],
                excluded_subreddits: vec!["spam".into()],
                trigger_topics: vec!["seo tools".into()],
                query_keywords: vec!["pageseeds".into()],
            },
        }
    }

    #[test]
    fn round_trip_full_fixture() {
        let dir = temp_dir("roundtrip");
        let path = project_config_path(&dir);
        let original = full_fixture();

        save_project_config(&path, &original).unwrap();
        let loaded = load_project_config(&path).unwrap();

        assert_eq!(loaded, original);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_empty_save_load() {
        let dir = temp_dir("default");
        let path = project_config_path(&dir);
        let original = ProjectConfig::default();

        assert_eq!(original.schema_version, SUPPORTED_SCHEMA_VERSION);
        assert_eq!(original.product_name, None);
        assert!(original.search_keywords.primary.is_empty());
        assert!(original.clusters.is_empty());
        assert_eq!(original.reddit.mention_stance, MentionStance::Optional);
        assert!(original.reddit.query_keywords.is_empty());

        save_project_config(&path, &original).unwrap();
        let loaded = load_project_config(&path).unwrap();
        assert_eq!(loaded, original);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_file_errors() {
        let dir = temp_dir("missing");
        let path = project_config_path(&dir);
        let err = load_project_config(&path).unwrap_err();
        match err {
            Error::ConfigMissing(p) => assert!(p.contains("project.yaml")),
            other => panic!("expected ConfigMissing, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bad_yaml_errors() {
        let dir = temp_dir("bad_yaml");
        let path = project_config_path(&dir);
        std::fs::write(&path, "schema_version: [\nnot: valid\n").unwrap();

        let err = load_project_config(&path).unwrap_err();
        match err {
            Error::Validation(msg) => assert!(msg.contains("invalid project config")),
            other => panic!("expected Validation, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_schema_version_errors() {
        let dir = temp_dir("no_version");
        let path = project_config_path(&dir);
        std::fs::write(
            &path,
            "product_name: Test\nsearch_keywords:\n  primary: []\n",
        )
        .unwrap();

        let err = load_project_config(&path).unwrap_err();
        match err {
            Error::Validation(_) => {}
            other => panic!("expected Validation for missing schema_version, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn schema_version_2_rejects() {
        let dir = temp_dir("v2");
        let path = project_config_path(&dir);
        std::fs::write(&path, "schema_version: 2\n").unwrap();

        let err = load_project_config(&path).unwrap_err();
        match err {
            Error::Validation(msg) => {
                assert!(msg.contains("schema_version 2"));
                assert!(msg.contains("supported: 1"));
            }
            other => panic!("expected Validation, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn schema_version_999_rejects() {
        let dir = temp_dir("v999");
        let path = project_config_path(&dir);
        std::fs::write(&path, "schema_version: 999\nproduct_name: x\n").unwrap();

        let err = load_project_config(&path).unwrap_err();
        match err {
            Error::Validation(msg) => assert!(msg.contains("999")),
            other => panic!("expected Validation, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn map_config_to_project_strategy_equality() {
        let cfg = full_fixture();
        let strategy = cfg.to_strategy();

        assert_eq!(strategy.primary_keywords, cfg.search_keywords.primary);
        assert_eq!(strategy.problem_keywords, cfg.search_keywords.problem);
        assert_eq!(strategy.audience_keywords, cfg.search_keywords.audience);
        assert_eq!(strategy.do_not_expand, cfg.search_keywords.do_not_expand);
        assert_eq!(strategy.clusters, cfg.clusters);

        // Reverse: strategy → config parts (empty reddit for migrator assembly)
        let rebuilt = ProjectConfig::from_strategy(&strategy);
        assert_eq!(rebuilt.search_keywords, cfg.search_keywords);
        assert_eq!(rebuilt.clusters, cfg.clusters);
        assert_eq!(rebuilt.reddit, ProjectRedditConfig::default());
        assert_eq!(rebuilt.product_name, None);
        assert_eq!(rebuilt.schema_version, 1);
    }

    #[test]
    fn stance_and_status_enum_wire_forms() {
        let yaml = r#"
schema_version: 1
clusters:
  - name: "Example"
    status: active
    keywords: []
  - name: "Old"
    status: legacy
    keywords: []
  - name: "Soon"
    status: planned
    keywords: []
  - name: "Keep"
    status: maintain
    keywords: []
  - name: "???"
    status: unknown
    keywords: []
reddit:
  mention_stance: recommended
  seed_subreddits: []
  excluded_subreddits: []
  trigger_topics: []
  query_keywords: []
"#;
        let cfg: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
        validate_schema_version(cfg.schema_version).unwrap();

        assert_eq!(cfg.clusters[0].status, ClusterStatus::Active);
        assert_eq!(cfg.clusters[1].status, ClusterStatus::Legacy);
        assert_eq!(cfg.clusters[2].status, ClusterStatus::Planned);
        assert_eq!(cfg.clusters[3].status, ClusterStatus::Maintain);
        assert_eq!(cfg.clusters[4].status, ClusterStatus::Unknown);
        assert_eq!(cfg.reddit.mention_stance, MentionStance::Recommended);

        // Serialize back uses snake_case
        let out = serde_yaml::to_string(&cfg).unwrap();
        assert!(out.contains("mention_stance: recommended"));
        assert!(out.contains("status: active"));
        assert!(out.contains("status: legacy"));
    }

    #[test]
    fn query_keywords_defaults_to_empty() {
        let yaml = r#"
schema_version: 1
reddit:
  mention_stance: optional
"#;
        let cfg: ProjectConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(cfg.reddit.query_keywords.is_empty());
        assert_eq!(cfg.reddit.mention_stance, MentionStance::Optional);
    }

    #[test]
    fn load_normalizes_subreddit_wire_form() {
        let dir = temp_dir("sub_norm");
        let path = project_config_path(&dir);
        std::fs::write(
            &path,
            r#"
schema_version: 1
reddit:
  seed_subreddits:
    - r/SEO
    - R/Marketing
    - " options "
  excluded_subreddits:
    - r/spam
    - WALLSTREETBETS
"#,
        )
        .unwrap();

        let loaded = load_project_config(&path).unwrap();
        assert_eq!(
            loaded.reddit.seed_subreddits,
            vec!["seo", "marketing", "options"]
        );
        assert_eq!(
            loaded.reddit.excluded_subreddits,
            vec!["spam", "wallstreetbets"]
        );

        // Save writes normalized form
        save_project_config(&path, &loaded).unwrap();
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(!on_disk.contains("r/SEO"));
        assert!(!on_disk.contains("R/Marketing"));
        assert!(on_disk.contains("- seo"));
        assert!(on_disk.contains("- marketing"));
        assert!(on_disk.contains("- spam"));
        assert!(on_disk.contains("- wallstreetbets"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_normalizes_prefixed_subreddits() {
        let dir = temp_dir("save_norm");
        let path = project_config_path(&dir);
        let mut cfg = ProjectConfig::default();
        cfg.reddit.seed_subreddits = vec!["r/SEO".into()];
        cfg.reddit.excluded_subreddits = vec!["r/Spam".into()];

        save_project_config(&path, &cfg).unwrap();
        let loaded = load_project_config(&path).unwrap();
        assert_eq!(loaded.reddit.seed_subreddits, vec!["seo"]);
        assert_eq!(loaded.reddit.excluded_subreddits, vec!["spam"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn project_config_path_joins_filename() {
        let dir = PathBuf::from("/tmp/automation");
        assert_eq!(
            project_config_path(&dir),
            PathBuf::from("/tmp/automation/project.yaml")
        );
    }

    #[test]
    fn mention_stance_md_uppercase_unchanged() {
        // MD callers keep UPPERCASE API
        assert_eq!(MentionStance::from_str("REQUIRED"), MentionStance::Required);
        assert_eq!(MentionStance::Required.as_str(), "REQUIRED");
        assert_eq!(MentionStance::Optional.as_str(), "OPTIONAL");
    }
}
