//! Runtime chokepoint: ensure structured `project.yaml` is available.
//!
//! Single entry for load-or-auto-migrate. See issue #292.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

use super::migrate::{migrate_project_config, MigrateOpts};
use super::{load_project_config, project_config_path, ProjectConfig};

/// Outcome of [`ensure_project_config`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnsureAction {
    /// Valid `project.yaml` was already present and loaded (no rewrite).
    Loaded,
    /// Legacy MD was migrated to `project.yaml` then loaded.
    AutoMigrated,
}

/// Ensure structured project config is available as valid `project.yaml`.
///
/// Algorithm:
/// 1. If `project.yaml` exists and is valid → load, return [`EnsureAction::Loaded`].
///    Does **not** rewrite. Does **not** re-read MD.
/// 2. If `project.yaml` exists but is invalid / unsupported schema →
///    [`Error::Validation`] (or other load error). **No** MD fallback.
/// 3. Else if legacy migratable sources exist (`project.md` and/or
///    `reddit_config.md`) → call [`migrate_project_config`] (not dry-run, not
///    force), load the new YAML → [`EnsureAction::AutoMigrated`]. Logs the
///    auto-migrate notice.
/// 4. Else → [`Error::ConfigMissing`]. Does **not** scaffold empty defaults.
///
/// Idempotency: a second call after a successful migrate returns
/// [`EnsureAction::Loaded`] without rewriting the YAML.
pub fn ensure_project_config(automation_dir: &Path) -> Result<(ProjectConfig, EnsureAction)> {
    let yaml_path = project_config_path(automation_dir);

    if yaml_path.exists() {
        // Valid → Loaded; invalid/unsupported → Err. Never fall back to MD.
        let config = load_project_config(&yaml_path)?;
        return Ok((config, EnsureAction::Loaded));
    }

    let has_legacy = automation_dir.join("project.md").exists()
        || automation_dir.join("reddit_config.md").exists();

    if !has_legacy {
        return Err(Error::ConfigMissing(yaml_path.display().to_string()));
    }

    migrate_project_config(
        automation_dir,
        MigrateOpts {
            dry_run: false,
            force: false,
        },
    )?;

    let config = load_project_config(&yaml_path)?;
    log::info!(
        "[project_config] auto-migrated legacy MD → project.yaml at {}",
        yaml_path.display()
    );
    Ok((config, EnsureAction::AutoMigrated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project_config::{save_project_config, ProjectConfig};
    use crate::strategy::parse_project_strategy;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn temp_automation(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("ps_ensure_{tag}_{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn ensure_md_only_auto_migrates_then_loaded_idempotent() {
        let dir = temp_automation("md_only");
        std::fs::write(dir.join("project.md"), STRATEGY_FIXTURE).unwrap();

        let yaml_path = project_config_path(&dir);
        assert!(!yaml_path.exists());

        let (cfg1, action1) = ensure_project_config(&dir).unwrap();
        assert_eq!(action1, EnsureAction::AutoMigrated);
        assert!(yaml_path.exists());
        assert_eq!(cfg1.search_keywords.primary, vec!["seo tools", "keyword research"]);

        let before = std::fs::read(yaml_path.as_path()).unwrap();

        let (cfg2, action2) = ensure_project_config(&dir).unwrap();
        assert_eq!(action2, EnsureAction::Loaded);
        assert_eq!(cfg2, cfg1);

        let after = std::fs::read(yaml_path.as_path()).unwrap();
        assert_eq!(before, after, "second ensure must not rewrite YAML");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_valid_yaml_loaded_no_rewrite() {
        let dir = temp_automation("valid_yaml");
        let path = project_config_path(&dir);
        let original = ProjectConfig::default();
        save_project_config(&path, &original).unwrap();
        let before = std::fs::read(&path).unwrap();

        // MD with different content must not be used when YAML is valid.
        std::fs::write(dir.join("project.md"), STRATEGY_FIXTURE).unwrap();

        let (cfg, action) = ensure_project_config(&dir).unwrap();
        assert_eq!(action, EnsureAction::Loaded);
        assert!(cfg.search_keywords.primary.is_empty());

        let after = std::fs::read(&path).unwrap();
        assert_eq!(before, after);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_invalid_yaml_errors_no_md_fallback() {
        let dir = temp_automation("invalid_yaml");
        let path = project_config_path(&dir);
        std::fs::write(&path, "schema_version: [\nnot: valid\n").unwrap();
        std::fs::write(dir.join("project.md"), STRATEGY_FIXTURE).unwrap();

        let err = ensure_project_config(&dir).unwrap_err();
        match err {
            Error::Validation(_) => {}
            other => panic!("expected Validation, got {other:?}"),
        }
        // Corrupt YAML must not have been replaced by a migrate.
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("not: valid"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_empty_dir_config_missing() {
        let dir = temp_automation("empty");
        let err = ensure_project_config(&dir).unwrap_err();
        match err {
            Error::ConfigMissing(p) => assert!(p.contains("project.yaml")),
            other => panic!("expected ConfigMissing, got {other:?}"),
        }
        assert!(!project_config_path(&dir).exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ensure_action_serde_snake_case() {
        let loaded = serde_json::to_string(&EnsureAction::Loaded).unwrap();
        let migrated = serde_json::to_string(&EnsureAction::AutoMigrated).unwrap();
        assert_eq!(loaded, "\"loaded\"");
        assert_eq!(migrated, "\"auto_migrated\"");

        let back: EnsureAction = serde_json::from_str("\"auto_migrated\"").unwrap();
        assert_eq!(back, EnsureAction::AutoMigrated);
    }

    #[test]
    fn ensure_auto_migrate_strategy_matches_md_parse() {
        let dir = temp_automation("match_md");
        std::fs::write(dir.join("project.md"), STRATEGY_FIXTURE).unwrap();

        let (cfg, action) = ensure_project_config(&dir).unwrap();
        assert_eq!(action, EnsureAction::AutoMigrated);

        let expected = parse_project_strategy(STRATEGY_FIXTURE);
        assert_eq!(cfg.to_strategy(), expected);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
