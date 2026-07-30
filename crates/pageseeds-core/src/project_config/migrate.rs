//! Deterministic project config migrator: legacy MD → `project.yaml`.
//!
//! Zero LLM. Assembles [`ProjectConfig`] from `project.md` + `reddit_config.md`
//! (when present) and writes schema v1 YAML. See issue #291.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::reddit::config::{extract_query_keywords, parse_reddit_config};
use crate::strategy::load_project_strategy;

use super::{
    load_project_config, project_config_path, save_project_config, ProjectConfig,
    ProjectRedditConfig, SUPPORTED_SCHEMA_VERSION,
};

// ─── Migrate ─────────────────────────────────────────────────────────────────

/// Options for [`migrate_project_config`].
#[derive(Debug, Clone, Default)]
pub struct MigrateOpts {
    /// When true, compute the planned config and report without writing files.
    pub dry_run: bool,
    /// When true, backup then rewrite even if valid `project.yaml` already exists.
    pub force: bool,
}

/// What the migrator did (or would do).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MigrateAction {
    /// Wrote a new or forced `project.yaml`.
    Written,
    /// Dry-run: no filesystem writes.
    DryRun,
    /// Valid YAML already present and `--force` was not set.
    SkippedExisting,
}

/// Which legacy / intermediate sources fed the assembled config.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MigrateSources {
    pub project_md: bool,
    pub reddit_config_md: bool,
}

/// Field counts for a config snapshot (status + migrate report).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectConfigFieldCounts {
    pub primary_keywords: usize,
    pub problem_keywords: usize,
    pub audience_keywords: usize,
    pub do_not_expand: usize,
    pub clusters: usize,
    pub trigger_topics: usize,
    pub query_keywords: usize,
    pub seed_subreddits: usize,
    pub excluded_subreddits: usize,
}

impl ProjectConfigFieldCounts {
    pub fn from_config(cfg: &ProjectConfig) -> Self {
        Self {
            primary_keywords: cfg.search_keywords.primary.len(),
            problem_keywords: cfg.search_keywords.problem.len(),
            audience_keywords: cfg.search_keywords.audience.len(),
            do_not_expand: cfg.search_keywords.do_not_expand.len(),
            clusters: cfg.clusters.len(),
            trigger_topics: cfg.reddit.trigger_topics.len(),
            query_keywords: cfg.reddit.query_keywords.len(),
            seed_subreddits: cfg.reddit.seed_subreddits.len(),
            excluded_subreddits: cfg.reddit.excluded_subreddits.len(),
        }
    }
}

/// Result of a migrate attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrateReport {
    pub action: MigrateAction,
    pub yaml_path: PathBuf,
    pub sources: MigrateSources,
    pub counts: ProjectConfigFieldCounts,
    pub warnings: Vec<String>,
    /// Present when an existing YAML was backed up before rewrite.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<PathBuf>,
    pub schema_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_name: Option<String>,
}

/// Assemble and optionally write `project.yaml` from legacy MD sources.
///
/// See module docs and issue #291 for the full algorithm.
pub fn migrate_project_config(automation_dir: &Path, opts: MigrateOpts) -> Result<MigrateReport> {
    let yaml_path = project_config_path(automation_dir);

    // Existing valid YAML → skip (unless force).
    if yaml_path.exists() {
        match load_project_config(&yaml_path) {
            Ok(existing) => {
                if !opts.force {
                    return Ok(MigrateReport {
                        action: MigrateAction::SkippedExisting,
                        yaml_path,
                        sources: MigrateSources::default(),
                        counts: ProjectConfigFieldCounts::from_config(&existing),
                        warnings: vec![],
                        backup_path: None,
                        schema_version: existing.schema_version,
                        product_name: existing.product_name.clone(),
                    });
                }
                // force: fall through after optional backup
            }
            Err(e) if !opts.force => {
                // Invalid / unsupported schema without force → do not clobber.
                return Err(Error::Validation(format!(
                    "existing project.yaml is invalid or unsupported (refusing to overwrite without --force): {e}"
                )));
            }
            Err(_) => {
                // force: fall through after optional backup
            }
        }
    }

    let (config, sources, warnings) = assemble_from_legacy(automation_dir);

    if opts.dry_run {
        return Ok(MigrateReport {
            action: MigrateAction::DryRun,
            yaml_path,
            sources,
            counts: ProjectConfigFieldCounts::from_config(&config),
            warnings,
            backup_path: None,
            schema_version: config.schema_version,
            product_name: config.product_name.clone(),
        });
    }

    // Backup existing YAML when rewriting (force or invalid with force).
    let backup_path = if yaml_path.exists() {
        Some(backup_existing_yaml(&yaml_path)?)
    } else {
        None
    };

    save_project_config(&yaml_path, &config)?;

    Ok(MigrateReport {
        action: MigrateAction::Written,
        yaml_path,
        sources,
        counts: ProjectConfigFieldCounts::from_config(&config),
        warnings,
        backup_path,
        schema_version: config.schema_version,
        product_name: config.product_name.clone(),
    })
}

/// Build a v1 [`ProjectConfig`] from legacy MD sources (no filesystem write).
fn assemble_from_legacy(automation_dir: &Path) -> (ProjectConfig, MigrateSources, Vec<String>) {
    let mut warnings = Vec::new();
    let mut sources = MigrateSources::default();

    let project_md_path = automation_dir.join("project.md");
    sources.project_md = project_md_path.exists();

    let strategy = load_project_strategy(automation_dir);
    if strategy.is_empty() {
        if sources.project_md {
            warnings.push(
                "project.md present but produced empty strategy (missing Search Keywords / Content Clusters?)"
                    .into(),
            );
        } else {
            warnings.push("project.md missing — strategy fields empty".into());
        }
    }

    let mut config = ProjectConfig::from_strategy(&strategy);

    let reddit_path = automation_dir.join("reddit_config.md");
    sources.reddit_config_md = reddit_path.exists();

    if sources.reddit_config_md {
        match std::fs::read_to_string(&reddit_path) {
            Ok(content) => {
                let reddit = parse_reddit_config(&content);
                let query_keywords = extract_query_keywords(&content);
                config.product_name = reddit.product_name;
                config.reddit = ProjectRedditConfig {
                    mention_stance: reddit.mention_stance,
                    seed_subreddits: reddit.seed_subreddits,
                    excluded_subreddits: reddit.excluded_subreddits,
                    trigger_topics: reddit.trigger_topics,
                    query_keywords,
                };
            }
            Err(e) => {
                warnings.push(format!(
                    "reddit_config.md present but unreadable ({e}) — using reddit defaults"
                ));
            }
        }
    } else {
        warnings.push("reddit_config.md missing — using reddit defaults".into());
    }

    config.schema_version = SUPPORTED_SCHEMA_VERSION;
    (config, sources, warnings)
}

fn backup_existing_yaml(yaml_path: &Path) -> Result<PathBuf> {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup = yaml_path.with_file_name(format!("project.yaml.bak.{ts}"));
    std::fs::copy(yaml_path, &backup).map_err(|e| {
        Error::Other(format!(
            "failed to backup {} → {}: {e}",
            yaml_path.display(),
            backup.display()
        ))
    })?;
    Ok(backup)
}

// ─── Status ──────────────────────────────────────────────────────────────────

/// On-disk format discriminator for project config.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectConfigFormat {
    /// Valid schema-v1 `project.yaml` present.
    Yaml,
    /// No valid YAML, but at least one legacy MD source exists.
    LegacyMd,
    /// Nothing useful on disk.
    Missing,
}

/// Presence of legacy MD sources under the automation dir.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LegacySourcesStatus {
    pub project_md: bool,
    pub reddit_config_md: bool,
}

/// Read-only snapshot of project config readiness (CLI status).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfigStatus {
    pub format: ProjectConfigFormat,
    pub yaml_path: PathBuf,
    pub yaml_present: bool,
    /// `Some(true)` when present and valid; `Some(false)` when present but invalid;
    /// `None` when absent.
    pub yaml_valid: Option<bool>,
    /// True when YAML is absent or invalid AND at least one legacy MD source exists.
    pub needs_migration: bool,
    pub legacy: LegacySourcesStatus,
    pub counts: ProjectConfigFieldCounts,
    pub hint: String,
}

const MIGRATE_HINT: &str = "pageseeds-cli migrate-project-config -p . --dry-run";

/// Inspect automation dir for `project.yaml` vs legacy MD readiness.
pub fn project_config_status(automation_dir: &Path) -> ProjectConfigStatus {
    let yaml_path = project_config_path(automation_dir);
    let yaml_present = yaml_path.exists();

    let legacy = LegacySourcesStatus {
        project_md: automation_dir.join("project.md").exists(),
        reddit_config_md: automation_dir.join("reddit_config.md").exists(),
    };
    let any_legacy = legacy.project_md || legacy.reddit_config_md;

    let (yaml_valid, yaml_counts) = if yaml_present {
        match load_project_config(&yaml_path) {
            Ok(cfg) => (Some(true), Some(ProjectConfigFieldCounts::from_config(&cfg))),
            Err(_) => (Some(false), None),
        }
    } else {
        (None, None)
    };

    let yaml_ok = yaml_valid == Some(true);
    let needs_migration = !yaml_ok && any_legacy;

    let format = if yaml_ok {
        ProjectConfigFormat::Yaml
    } else if any_legacy {
        ProjectConfigFormat::LegacyMd
    } else {
        ProjectConfigFormat::Missing
    };

    let counts = if let Some(c) = yaml_counts {
        c
    } else if any_legacy {
        let (cfg, _, _) = assemble_from_legacy(automation_dir);
        ProjectConfigFieldCounts::from_config(&cfg)
    } else {
        ProjectConfigFieldCounts::default()
    };

    ProjectConfigStatus {
        format,
        yaml_path,
        yaml_present,
        yaml_valid,
        needs_migration,
        legacy,
        counts,
        hint: MIGRATE_HINT.to_string(),
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reddit::config::MentionStance;
    use crate::strategy::{ClusterStatus, parse_project_strategy};

    // Mirrors strategy::tests::FIXTURE (private there).
    const STRATEGY_FIXTURE: &str = r#"# Example Project

## Search Keywords

### Primary Keywords
- seo tools
- keyword research

### Problem Keywords
- thin content

### Audience Keywords
- content marketers

### Legacy Service Keywords (do not expand)
- custom web design
- wordpress agency

## Content Clusters And Priorities

### Cluster 1: SEO Fundamentals (ACTIVE)
- on-page seo
- technical seo

### Cluster 2: Alternatives (MAINTAIN)
- competitor alternatives

### Cluster 3: Services (LEGACY)
- web design packages

### Cluster 4: New Pillar (PLANNED)
- ai content ops
"#;

    // Mirrors reddit::config::tests::SAMPLE plus Query Keywords section.
    const REDDIT_SAMPLE: &str = r#"
## Product Name
- Days to Expiry

## Mention Stance
- REQUIRED

## Trigger Topics
- options trading strategies
- DTE tracking for options
- managing expiry risk

## Seed Subreddits
- r/options
- r/thetagang

## Excluded Subreddits
- r/wallstreetbets

## Query Keywords
- "options DTE tracker"
- "days to expiry tool"
"#;

    fn temp_automation(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("ps_migrate_{tag}_{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_legacy(dir: &Path, project_md: Option<&str>, reddit_md: Option<&str>) {
        if let Some(body) = project_md {
            std::fs::write(dir.join("project.md"), body).unwrap();
        }
        if let Some(body) = reddit_md {
            std::fs::write(dir.join("reddit_config.md"), body).unwrap();
        }
    }

    #[test]
    fn migrate_fixture_field_equality() {
        let dir = temp_automation("fixture");
        write_legacy(&dir, Some(STRATEGY_FIXTURE), Some(REDDIT_SAMPLE));

        let report = migrate_project_config(
            &dir,
            MigrateOpts {
                dry_run: false,
                force: false,
            },
        )
        .unwrap();

        assert_eq!(report.action, MigrateAction::Written);
        assert_eq!(report.schema_version, 1);
        assert!(report.sources.project_md);
        assert!(report.sources.reddit_config_md);
        assert_eq!(report.product_name.as_deref(), Some("Days to Expiry"));

        let loaded = load_project_config(&report.yaml_path).unwrap();
        let strategy = parse_project_strategy(STRATEGY_FIXTURE);
        assert_eq!(loaded.search_keywords.primary, strategy.primary_keywords);
        assert_eq!(loaded.search_keywords.problem, strategy.problem_keywords);
        assert_eq!(loaded.search_keywords.audience, strategy.audience_keywords);
        assert_eq!(loaded.search_keywords.do_not_expand, strategy.do_not_expand);
        assert_eq!(loaded.clusters, strategy.clusters);
        assert_eq!(loaded.clusters[0].status, ClusterStatus::Active);

        assert_eq!(loaded.product_name.as_deref(), Some("Days to Expiry"));
        assert_eq!(loaded.reddit.mention_stance, MentionStance::Required);
        assert_eq!(loaded.reddit.seed_subreddits, vec!["options", "thetagang"]);
        assert_eq!(loaded.reddit.excluded_subreddits, vec!["wallstreetbets"]);
        assert_eq!(loaded.reddit.trigger_topics.len(), 3);
        assert_eq!(
            loaded.reddit.query_keywords,
            vec!["options DTE tracker", "days to expiry tool"]
        );

        assert_eq!(report.counts.primary_keywords, 2);
        assert_eq!(report.counts.clusters, 4);
        assert_eq!(report.counts.trigger_topics, 3);
        assert_eq!(report.counts.query_keywords, 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dry_run_no_write_no_bak() {
        let dir = temp_automation("dry");
        write_legacy(&dir, Some(STRATEGY_FIXTURE), Some(REDDIT_SAMPLE));

        let report = migrate_project_config(
            &dir,
            MigrateOpts {
                dry_run: true,
                force: false,
            },
        )
        .unwrap();

        assert_eq!(report.action, MigrateAction::DryRun);
        assert_eq!(report.counts.primary_keywords, 2);
        assert_eq!(report.counts.query_keywords, 2);
        assert!(!project_config_path(&dir).exists());
        // No backup files either
        let entries: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!entries.iter().any(|n| n.contains(".bak.")));
        assert!(!entries.iter().any(|n| n == "project.yaml"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn real_migrate_writes_schema_version_1() {
        let dir = temp_automation("v1");
        write_legacy(&dir, Some(STRATEGY_FIXTURE), Some(REDDIT_SAMPLE));

        migrate_project_config(&dir, MigrateOpts::default()).unwrap();
        let raw = std::fs::read_to_string(project_config_path(&dir)).unwrap();
        assert!(raw.contains("schema_version: 1") || raw.contains("schema_version:1"));

        let cfg = load_project_config(&project_config_path(&dir)).unwrap();
        assert_eq!(cfg.schema_version, SUPPORTED_SCHEMA_VERSION);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_reddit_uses_defaults() {
        let dir = temp_automation("no_reddit");
        write_legacy(&dir, Some(STRATEGY_FIXTURE), None);

        let report = migrate_project_config(&dir, MigrateOpts::default()).unwrap();
        assert_eq!(report.action, MigrateAction::Written);
        assert!(report.sources.project_md);
        assert!(!report.sources.reddit_config_md);
        assert!(report
            .warnings
            .iter()
            .any(|w| w.contains("reddit_config.md missing")));

        let cfg = load_project_config(&report.yaml_path).unwrap();
        assert_eq!(cfg.product_name, None);
        assert_eq!(cfg.reddit, ProjectRedditConfig::default());
        assert_eq!(cfg.search_keywords.primary, vec!["seo tools", "keyword research"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_strategy_and_missing_reddit_valid_empty_yaml() {
        let dir = temp_automation("empty");
        // No MD files at all
        let report = migrate_project_config(&dir, MigrateOpts::default()).unwrap();
        assert_eq!(report.action, MigrateAction::Written);

        let cfg = load_project_config(&report.yaml_path).unwrap();
        assert_eq!(cfg.schema_version, 1);
        assert!(cfg.search_keywords.primary.is_empty());
        assert!(cfg.clusters.is_empty());
        assert_eq!(cfg.reddit, ProjectRedditConfig::default());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn existing_valid_yaml_skipped_without_force() {
        let dir = temp_automation("skip");
        write_legacy(&dir, Some(STRATEGY_FIXTURE), Some(REDDIT_SAMPLE));
        migrate_project_config(&dir, MigrateOpts::default()).unwrap();

        let before = std::fs::read_to_string(project_config_path(&dir)).unwrap();

        // Change legacy sources — should not rewrite without force
        write_legacy(
            &dir,
            Some("## Search Keywords\n### Primary Keywords\n- changed\n"),
            None,
        );

        let report = migrate_project_config(&dir, MigrateOpts::default()).unwrap();
        assert_eq!(report.action, MigrateAction::SkippedExisting);
        assert_eq!(report.counts.primary_keywords, 2); // from existing YAML

        let after = std::fs::read_to_string(project_config_path(&dir)).unwrap();
        assert_eq!(before, after);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn force_backups_then_rewrites() {
        let dir = temp_automation("force");
        write_legacy(&dir, Some(STRATEGY_FIXTURE), Some(REDDIT_SAMPLE));
        migrate_project_config(&dir, MigrateOpts::default()).unwrap();

        // Rewrite legacy with different primary keyword
        write_legacy(
            &dir,
            Some(
                r#"## Search Keywords
### Primary Keywords
- forced keyword
"#,
            ),
            Some(REDDIT_SAMPLE),
        );

        let report = migrate_project_config(
            &dir,
            MigrateOpts {
                dry_run: false,
                force: true,
            },
        )
        .unwrap();

        assert_eq!(report.action, MigrateAction::Written);
        assert!(report.backup_path.is_some());
        let bak = report.backup_path.unwrap();
        assert!(bak.exists());
        assert!(bak
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("project.yaml.bak."));

        let cfg = load_project_config(&project_config_path(&dir)).unwrap();
        assert_eq!(cfg.search_keywords.primary, vec!["forced keyword"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_yaml_without_force_errors() {
        let dir = temp_automation("bad");
        let path = project_config_path(&dir);
        std::fs::write(&path, "schema_version: 2\n").unwrap();
        write_legacy(&dir, Some(STRATEGY_FIXTURE), None);

        let err = migrate_project_config(&dir, MigrateOpts::default()).unwrap_err();
        match err {
            Error::Validation(msg) => {
                assert!(msg.contains("--force") || msg.contains("refusing"));
            }
            other => panic!("expected Validation, got {other:?}"),
        }
        // Original file untouched
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(raw.contains("schema_version: 2"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn invalid_yaml_with_force_rewrites() {
        let dir = temp_automation("bad_force");
        let path = project_config_path(&dir);
        std::fs::write(&path, "not: valid: yaml: [[[\n").unwrap();
        write_legacy(&dir, Some(STRATEGY_FIXTURE), Some(REDDIT_SAMPLE));

        let report = migrate_project_config(
            &dir,
            MigrateOpts {
                dry_run: false,
                force: true,
            },
        )
        .unwrap();
        assert_eq!(report.action, MigrateAction::Written);
        assert!(report.backup_path.is_some());

        let cfg = load_project_config(&path).unwrap();
        assert_eq!(cfg.schema_version, 1);
        assert_eq!(cfg.search_keywords.primary.len(), 2);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dry_run_with_force_still_no_write() {
        let dir = temp_automation("dry_force");
        write_legacy(&dir, Some(STRATEGY_FIXTURE), Some(REDDIT_SAMPLE));
        migrate_project_config(&dir, MigrateOpts::default()).unwrap();
        let before = std::fs::read_to_string(project_config_path(&dir)).unwrap();

        write_legacy(
            &dir,
            Some("## Search Keywords\n### Primary Keywords\n- dry only\n"),
            None,
        );

        let report = migrate_project_config(
            &dir,
            MigrateOpts {
                dry_run: true,
                force: true,
            },
        )
        .unwrap();
        assert_eq!(report.action, MigrateAction::DryRun);
        assert_eq!(report.counts.primary_keywords, 1);
        assert!(report.backup_path.is_none());

        let after = std::fs::read_to_string(project_config_path(&dir)).unwrap();
        assert_eq!(before, after);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_needs_migration_and_format_discriminators() {
        let dir = temp_automation("status");

        // Pure missing
        let st = project_config_status(&dir);
        assert_eq!(st.format, ProjectConfigFormat::Missing);
        assert!(!st.needs_migration);
        assert!(!st.yaml_present);
        assert_eq!(st.yaml_valid, None);
        assert!(st.hint.contains("migrate-project-config"));

        // Legacy present, no YAML
        write_legacy(&dir, Some(STRATEGY_FIXTURE), Some(REDDIT_SAMPLE));
        let st = project_config_status(&dir);
        assert_eq!(st.format, ProjectConfigFormat::LegacyMd);
        assert!(st.needs_migration);
        assert!(st.legacy.project_md);
        assert!(st.legacy.reddit_config_md);
        assert_eq!(st.counts.primary_keywords, 2);
        assert_eq!(st.counts.query_keywords, 2);

        // After migrate
        migrate_project_config(&dir, MigrateOpts::default()).unwrap();
        let st = project_config_status(&dir);
        assert_eq!(st.format, ProjectConfigFormat::Yaml);
        assert!(!st.needs_migration);
        assert_eq!(st.yaml_valid, Some(true));
        assert!(st.yaml_present);

        // Invalid YAML + legacy → needs migration
        std::fs::write(project_config_path(&dir), "schema_version: 99\n").unwrap();
        let st = project_config_status(&dir);
        assert_eq!(st.format, ProjectConfigFormat::LegacyMd);
        assert!(st.needs_migration);
        assert_eq!(st.yaml_valid, Some(false));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
