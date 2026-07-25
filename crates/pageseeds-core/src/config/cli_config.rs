//! CLI project context resolution (issue #177).
//!
//! Resolution chain for project id/path after setup:
//! ```text
//! flags (-i / -p)
//!   → env (PAGESEEDS_PROJECT_ID / PAGESEEDS_PROJECT_PATH)
//!   → local .pageseeds.yaml (cwd)
//!   → global config.toml defaults
//!   → if id resolved but path missing: load path from SQLite project row
//!   → if path resolved but id missing: find registered project by canonical path
//!   → clear error mentioning `pageseeds-cli setup` (never "open sqlite")
//! ```
//!
//! Global config: `dirs::config_dir()/pageseeds/config.toml`  
//! Overrides: `PAGESEEDS_CONFIG_PATH` (file) or `PAGESEEDS_CONFIG_DIR` (directory).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Message when no project context can be resolved. Never mentions sqlite.
pub const MISSING_PROJECT_CONTEXT_HINT: &str =
    "No project context resolved. Run `pageseeds-cli setup` in your project directory \
(or pass -i/--project-id and -p/--project-path). See docs/CLI_GETTING_STARTED.md.";

/// Global defaults written by `pageseeds-cli setup`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GlobalCliConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_project_path: Option<String>,
}

/// Repo-local marker (cwd). v1: project_id only.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LocalCliConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

/// Fully resolved project context for a CLI data tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProject {
    pub project_id: String,
    pub project_path: String,
}

/// Resolve the config directory (`…/pageseeds`).
pub fn config_dir() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("PAGESEEDS_CONFIG_DIR") {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    let base = dirs::config_dir()
        .ok_or_else(|| "could not resolve config directory for PageSeeds CLI".to_string())?;
    Ok(base.join("pageseeds"))
}

/// Path to the global `config.toml`.
pub fn global_config_path() -> Result<PathBuf, String> {
    if let Ok(p) = std::env::var("PAGESEEDS_CONFIG_PATH") {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    Ok(config_dir()?.join("config.toml"))
}

/// Load global config; missing file → empty defaults (not an error).
pub fn load_global() -> Result<GlobalCliConfig, String> {
    let path = global_config_path()?;
    if !path.exists() {
        return Ok(GlobalCliConfig::default());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("failed to read CLI config {}: {e}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(GlobalCliConfig::default());
    }
    toml::from_str(&raw).map_err(|e| format!("invalid CLI config {}: {e}", path.display()))
}

/// Persist global defaults (creates parent dirs).
pub fn save_global(config: &GlobalCliConfig) -> Result<(), String> {
    let path = global_config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create config directory {}: {e}", parent.display()))?;
    }
    let raw = toml::to_string_pretty(config)
        .map_err(|e| format!("failed to serialize CLI config: {e}"))?;
    std::fs::write(&path, raw)
        .map_err(|e| format!("failed to write CLI config {}: {e}", path.display()))?;
    Ok(())
}

/// Load `.pageseeds.yaml` from `cwd` if present.
pub fn load_local(cwd: &Path) -> Result<Option<LocalCliConfig>, String> {
    let path = cwd.join(".pageseeds.yaml");
    if !path.exists() {
        // Also accept .yml for convenience
        let alt = cwd.join(".pageseeds.yml");
        if !alt.exists() {
            return Ok(None);
        }
        return load_local_file(&alt).map(Some);
    }
    load_local_file(&path).map(Some)
}

fn load_local_file(path: &Path) -> Result<LocalCliConfig, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    if raw.trim().is_empty() {
        return Ok(LocalCliConfig::default());
    }
    serde_yaml::from_str(&raw).map_err(|e| format!("invalid {}: {e}", path.display()))
}

/// Write repo-local `.pageseeds.yaml` under `cwd`.
pub fn save_local(cwd: &Path, config: &LocalCliConfig) -> Result<(), String> {
    let path = cwd.join(".pageseeds.yaml");
    let raw = serde_yaml::to_string(config)
        .map_err(|e| format!("failed to serialize local CLI config: {e}"))?;
    std::fs::write(&path, raw)
        .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    Ok(())
}

fn nonempty(s: Option<&str>) -> Option<String> {
    s.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(|v| {
        let t = v.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    })
}

/// Expand a leading `~` to `$HOME` (best-effort).
///
/// Canonical path API (issue #177 / review): use this + [`normalize_path_string`]
/// + [`paths_equal`] everywhere path identity matters. Do not reimplement tilde
/// expansion or path matching in the bin, task_store, or project_create.
pub fn expand_tilde(path: &str) -> String {
    if path.starts_with('~') {
        std::env::var("HOME")
            .map(|h| path.replacen('~', &h, 1))
            .unwrap_or_else(|_| path.to_string())
    } else {
        path.to_string()
    }
}

/// Best-effort absolute path for comparison / storage.
///
/// Trims whitespace, expands `~`, canonicalizes when the path exists on disk,
/// otherwise makes absolute via cwd when relative. Empty/whitespace → empty string.
pub fn normalize_path_string(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let expanded = expand_tilde(trimmed);
    let p = PathBuf::from(&expanded);
    if let Ok(c) = p.canonicalize() {
        return c.to_string_lossy().to_string();
    }
    if p.is_absolute() {
        return expanded;
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(&p).to_string_lossy().to_string())
        .unwrap_or(expanded)
}

/// Path identity after [`normalize_path_string`].
///
/// On macOS, comparison is ASCII case-insensitive (HFS+/APFS default).
pub fn paths_equal(a: &str, b: &str) -> bool {
    let na = normalize_path_string(a);
    let nb = normalize_path_string(b);
    if na == nb {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        na.eq_ignore_ascii_case(&nb)
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Resolve project id + path using the full chain.
///
/// `lookup_path_by_id` / `lookup_id_by_path` are optional SQLite bridges so this
/// module stays free of a hard DB dependency in pure config tests.
pub fn resolve_project_context_with_lookups(
    flags_id: Option<&str>,
    flags_path: Option<&str>,
    cwd: &Path,
    lookup_path_by_id: Option<&dyn Fn(&str) -> Option<String>>,
    lookup_id_by_path: Option<&dyn Fn(&str) -> Option<String>>,
) -> Result<ResolvedProject, String> {
    let mut id = nonempty(flags_id);
    let mut path = nonempty(flags_path).map(|p| expand_tilde(&p));

    if id.is_none() {
        id = env_nonempty("PAGESEEDS_PROJECT_ID");
    }
    if path.is_none() {
        path = env_nonempty("PAGESEEDS_PROJECT_PATH").map(|p| expand_tilde(&p));
    }

    if id.is_none() {
        if let Some(local) = load_local(cwd)? {
            id = local
                .project_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string());
        }
    }

    if id.is_none() || path.is_none() {
        let global = load_global()?;
        if id.is_none() {
            id = global
                .default_project_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| v.to_string());
        }
        if path.is_none() {
            path = global
                .default_project_path
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| expand_tilde(v));
        }
    }

    // Fill missing half from the project registry when possible.
    if id.is_some() && path.is_none() {
        if let (Some(pid), Some(lookup)) = (id.as_deref(), lookup_path_by_id) {
            path = lookup(pid);
        }
    }
    if path.is_some() && id.is_none() {
        if let (Some(ppath), Some(lookup)) = (path.as_deref(), lookup_id_by_path) {
            let normalized = normalize_path_string(ppath);
            id = lookup(&normalized).or_else(|| lookup(ppath));
        }
    }

    match (id, path) {
        (Some(project_id), Some(project_path)) => Ok(ResolvedProject {
            project_id,
            project_path,
        }),
        _ => Err(MISSING_PROJECT_CONTEXT_HINT.to_string()),
    }
}

/// Resolve using an open SQLite connection for registry lookups.
pub fn resolve_project_context(
    flags_id: Option<&str>,
    flags_path: Option<&str>,
    cwd: &Path,
    conn: Option<&rusqlite::Connection>,
) -> Result<ResolvedProject, String> {
    let path_by_id = |pid: &str| -> Option<String> {
        let conn = conn?;
        crate::engine::task_store::get_project(conn, pid)
            .ok()
            .map(|p| p.path)
    };
    let id_by_path = |path: &str| -> Option<String> {
        let conn = conn?;
        crate::engine::task_store::find_project_by_path(conn, path)
            .ok()
            .flatten()
            .map(|p| p.id)
    };
    // Bridge to trait objects: use closures via resolve_project_context_with_lookups
    // with optional dyn Fn only when conn is present.
    if conn.is_some() {
        resolve_project_context_with_lookups(
            flags_id,
            flags_path,
            cwd,
            Some(&path_by_id as &dyn Fn(&str) -> Option<String>),
            Some(&id_by_path as &dyn Fn(&str) -> Option<String>),
        )
    } else {
        resolve_project_context_with_lookups(flags_id, flags_path, cwd, None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::ENV_LOCK;
    use std::sync::MutexGuard;

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        keys: Vec<&'static str>,
        saved: Vec<Option<String>>,
    }

    impl EnvGuard {
        fn acquire(keys: &[&'static str]) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let saved: Vec<Option<String>> = keys.iter().map(|k| std::env::var(k).ok()).collect();
            for k in keys {
                std::env::remove_var(k);
            }
            Self {
                _lock: lock,
                keys: keys.to_vec(),
                saved,
            }
        }

        fn set(&self, key: &str, value: &str) {
            std::env::set_var(key, value);
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (k, prev) in self.keys.iter().zip(self.saved.iter()) {
                match prev {
                    Some(v) => std::env::set_var(k, v),
                    None => std::env::remove_var(k),
                }
            }
        }
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("{prefix}_{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolve_precedence_flag_over_env_local_global() {
        let _env = EnvGuard::acquire(&[
            "PAGESEEDS_CONFIG_DIR",
            "PAGESEEDS_CONFIG_PATH",
            "PAGESEEDS_PROJECT_ID",
            "PAGESEEDS_PROJECT_PATH",
        ]);

        let tmp = unique_temp_dir("ps_cli_cfg_prec");
        let config_dir = tmp.join("cfg");
        std::fs::create_dir_all(&config_dir).unwrap();
        _env.set("PAGESEEDS_CONFIG_DIR", &config_dir.to_string_lossy());

        save_global(&GlobalCliConfig {
            default_project_id: Some("global-id".into()),
            default_project_path: Some("/global/path".into()),
        })
        .unwrap();

        let cwd = tmp.join("repo");
        std::fs::create_dir_all(&cwd).unwrap();
        save_local(
            &cwd,
            &LocalCliConfig {
                project_id: Some("local-id".into()),
            },
        )
        .unwrap();

        _env.set("PAGESEEDS_PROJECT_ID", "env-id");
        _env.set("PAGESEEDS_PROJECT_PATH", "/env/path");

        // Flags win
        let r = resolve_project_context_with_lookups(
            Some("flag-id"),
            Some("/flag/path"),
            &cwd,
            None,
            None,
        )
        .unwrap();
        assert_eq!(r.project_id, "flag-id");
        assert_eq!(r.project_path, "/flag/path");

        // Env wins over local/global when flags empty
        let r = resolve_project_context_with_lookups(None, None, &cwd, None, None).unwrap();
        assert_eq!(r.project_id, "env-id");
        assert_eq!(r.project_path, "/env/path");

        // Local id + global path when env cleared
        std::env::remove_var("PAGESEEDS_PROJECT_ID");
        std::env::remove_var("PAGESEEDS_PROJECT_PATH");
        let r = resolve_project_context_with_lookups(None, None, &cwd, None, None).unwrap();
        assert_eq!(r.project_id, "local-id");
        assert_eq!(r.project_path, "/global/path");

        // Global only when no local
        let empty_cwd = tmp.join("empty");
        std::fs::create_dir_all(&empty_cwd).unwrap();
        let r = resolve_project_context_with_lookups(None, None, &empty_cwd, None, None).unwrap();
        assert_eq!(r.project_id, "global-id");
        assert_eq!(r.project_path, "/global/path");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_fills_path_from_id_lookup() {
        let _env = EnvGuard::acquire(&[
            "PAGESEEDS_CONFIG_DIR",
            "PAGESEEDS_CONFIG_PATH",
            "PAGESEEDS_PROJECT_ID",
            "PAGESEEDS_PROJECT_PATH",
        ]);
        let tmp = unique_temp_dir("ps_cli_cfg_id");
        _env.set("PAGESEEDS_CONFIG_DIR", &tmp.join("cfg").to_string_lossy());
        std::fs::create_dir_all(tmp.join("cfg")).unwrap();

        let lookup = |id: &str| {
            if id == "known" {
                Some("/from/db".into())
            } else {
                None
            }
        };
        let r = resolve_project_context_with_lookups(
            Some("known"),
            None,
            &tmp,
            Some(&lookup as &dyn Fn(&str) -> Option<String>),
            None,
        )
        .unwrap();
        assert_eq!(r.project_id, "known");
        assert_eq!(r.project_path, "/from/db");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_fills_id_from_path_lookup() {
        let _env = EnvGuard::acquire(&[
            "PAGESEEDS_CONFIG_DIR",
            "PAGESEEDS_CONFIG_PATH",
            "PAGESEEDS_PROJECT_ID",
            "PAGESEEDS_PROJECT_PATH",
        ]);
        let tmp = unique_temp_dir("ps_cli_cfg_path");
        _env.set("PAGESEEDS_CONFIG_DIR", &tmp.join("cfg").to_string_lossy());
        std::fs::create_dir_all(tmp.join("cfg")).unwrap();

        let lookup = |path: &str| {
            if path.contains("repo") {
                Some("by-path".into())
            } else {
                None
            }
        };
        let r = resolve_project_context_with_lookups(
            None,
            Some("/tmp/repo"),
            &tmp,
            None,
            Some(&lookup as &dyn Fn(&str) -> Option<String>),
        )
        .unwrap();
        assert_eq!(r.project_id, "by-path");
        assert_eq!(r.project_path, "/tmp/repo");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn missing_context_mentions_setup_not_sqlite() {
        let _env = EnvGuard::acquire(&[
            "PAGESEEDS_CONFIG_DIR",
            "PAGESEEDS_CONFIG_PATH",
            "PAGESEEDS_PROJECT_ID",
            "PAGESEEDS_PROJECT_PATH",
        ]);
        let tmp = unique_temp_dir("ps_cli_cfg_miss");
        _env.set("PAGESEEDS_CONFIG_DIR", &tmp.join("cfg").to_string_lossy());
        std::fs::create_dir_all(tmp.join("cfg")).unwrap();

        let err =
            resolve_project_context_with_lookups(None, None, &tmp, None, None).unwrap_err();
        assert!(
            err.contains("pageseeds-cli setup"),
            "error should mention setup: {err}"
        );
        assert!(
            !err.to_lowercase().contains("sqlite"),
            "error must not mention sqlite: {err}"
        );
        assert_eq!(err, MISSING_PROJECT_CONTEXT_HINT);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn save_and_load_global_roundtrip() {
        let _env = EnvGuard::acquire(&["PAGESEEDS_CONFIG_DIR", "PAGESEEDS_CONFIG_PATH"]);
        let tmp = unique_temp_dir("ps_cli_cfg_rt");
        let cfg = tmp.join("pageseeds");
        std::fs::create_dir_all(&cfg).unwrap();
        _env.set("PAGESEEDS_CONFIG_DIR", &cfg.to_string_lossy());

        let original = GlobalCliConfig {
            default_project_id: Some("mysite".into()),
            default_project_path: Some("/abs/path".into()),
        };
        save_global(&original).unwrap();
        let loaded = load_global().unwrap();
        assert_eq!(loaded, original);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn normalize_path_string_absolute_and_tilde() {
        let _env = EnvGuard::acquire(&["HOME"]);
        let tmp = unique_temp_dir("ps_cli_path_norm");
        let repo = tmp.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let abs = repo.canonicalize().unwrap().to_string_lossy().to_string();

        assert_eq!(normalize_path_string(&abs), abs);
        assert_eq!(normalize_path_string(&format!("  {abs}  ")), abs);
        assert_eq!(normalize_path_string(""), "");
        assert_eq!(normalize_path_string("   "), "");

        // Relative path becomes absolute (best-effort via cwd).
        let rel = normalize_path_string(".");
        assert!(PathBuf::from(&rel).is_absolute(), "got {rel}");

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn paths_equal_normalizes_both_sides() {
        let tmp = unique_temp_dir("ps_cli_paths_eq");
        let repo = tmp.join("site");
        std::fs::create_dir_all(&repo).unwrap();
        let abs = repo.canonicalize().unwrap().to_string_lossy().to_string();
        // Same path with trailing spaces / as-given absolute should match.
        assert!(paths_equal(&abs, &format!(" {abs} ")));
        assert!(paths_equal(&abs, &abs));
        assert!(!paths_equal(&abs, &tmp.join("other").to_string_lossy()));
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
