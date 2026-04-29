/// Secrets resolution with the same precedence chain as the Python CLI:
///
/// 1. ~/.config/automation/secrets.env  (highest priority)
/// 2. {repo_root}/.env.local
/// 3. {repo_root}/.env
/// 4. Shell environment variables       (fallback)
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Known secrets required by various modules.
pub const REQUIRED_SECRETS: &[(&str, &str)] = &[
    ("CAPSOLVER_API_KEY", "Ahrefs keyword research (CapSolver)"),
    (
        "DATAFORSEO_LOGIN",
        "DataForSEO API login (optional - use instead of Ahrefs)",
    ),
    (
        "DATAFORSEO_PASSWORD",
        "DataForSEO API password (optional - use instead of Ahrefs)",
    ),
    (
        "GSC_SERVICE_ACCOUNT_PATH",
        "Google Search Console (service account)",
    ),
    (
        "GSC_REPORT_OAUTH_CLIENT_SECRETS",
        "Google Search Console (OAuth alternative)",
    ),
    ("REDDIT_CLIENT_ID", "Reddit API"),
    ("REDDIT_CLIENT_SECRET", "Reddit API"),
    ("REDDIT_REFRESH_TOKEN", "Reddit API (posting)"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretStatus {
    pub key: String,
    pub description: String,
    pub configured: bool,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsStatus {
    pub secrets: Vec<SecretStatus>,
    pub secrets_file_exists: bool,
    pub secrets_file_path: String,
}

pub struct EnvResolver {
    repo_root: PathBuf,
}

impl EnvResolver {
    pub fn new(repo_root: impl Into<PathBuf>) -> Self {
        Self {
            repo_root: repo_root.into(),
        }
    }

    /// Return all env files in priority order (first = highest priority).
    pub fn env_files(&self) -> Vec<PathBuf> {
        let mut files = vec![];

        // 1. Machine-local secrets (highest priority) — ~/.config/automation/secrets.env
        if let Some(home) = dirs::home_dir() {
            let secrets = home.join(".config").join("automation").join("secrets.env");
            if secrets.exists() {
                files.push(secrets);
            }
        }

        // 2. Repo-local overrides
        let env_local = self.repo_root.join(".env.local");
        if env_local.exists() {
            files.push(env_local);
        }

        // 3. Repo defaults
        let env = self.repo_root.join(".env");
        if env.exists() {
            files.push(env);
        }

        files
    }

    /// Resolve a single key. Returns (value, source) or None.
    pub fn resolve(&self, key: &str) -> Option<(String, String)> {
        // Check env files in priority order (first file wins)
        for file in self.env_files() {
            if let Ok(content) = std::fs::read_to_string(&file) {
                let file_label = file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("env file")
                    .to_string();
                for line in content.lines() {
                    let line = line.trim();
                    if line.starts_with('#') || !line.contains('=') {
                        continue;
                    }
                    if let Some((k, v)) = line.split_once('=') {
                        if k.trim() == key {
                            let value = v.trim().trim_matches('"').trim_matches('\'').to_string();
                            if !value.is_empty() {
                                return Some((value, file_label));
                            }
                        }
                    }
                }
            }
        }

        // Fall back to shell environment
        if let Ok(val) = std::env::var(key) {
            if !val.is_empty() {
                return Some((val, "shell env".to_string()));
            }
        }

        None
    }

    /// Build a full environment map for subprocess calls.
    /// File-sourced values override the current process environment.
    pub fn build_env(&self, overrides: HashMap<String, String>) -> HashMap<String, String> {
        // Start with current process environment
        let mut env: HashMap<String, String> = std::env::vars().collect();

        // Apply file-sourced values (they WIN over shell env per spec)
        for file in self.env_files().into_iter().rev() {
            // reverse: lowest priority first so higher priority overwrites
            if let Ok(content) = std::fs::read_to_string(&file) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.starts_with('#') || !line.contains('=') {
                        continue;
                    }
                    if let Some((k, v)) = line.split_once('=') {
                        let key = k.trim().to_string();
                        let value = v.trim().trim_matches('"').trim_matches('\'').to_string();
                        if !key.is_empty() {
                            env.insert(key, value);
                        }
                    }
                }
            }
        }

        // Caller overrides win over everything
        env.extend(overrides);
        env
    }

    /// Report the status of all known secrets.
    pub fn secrets_status(&self) -> SecretsStatus {
        let secrets_file = dirs::config_dir()
            .map(|d| d.join("automation").join("secrets.env"))
            .unwrap_or_default();

        let secrets_file_exists = secrets_file.exists();
        let secrets_file_path = secrets_file.to_string_lossy().to_string();

        let secrets = REQUIRED_SECRETS
            .iter()
            .map(|(key, description)| {
                let resolved = self.resolve(key);
                SecretStatus {
                    key: key.to_string(),
                    description: description.to_string(),
                    configured: resolved.is_some(),
                    source: resolved.map(|(_, src)| src),
                }
            })
            .collect();

        SecretsStatus {
            secrets,
            secrets_file_exists,
            secrets_file_path,
        }
    }
}

/// Returns the path to secrets.env for display purposes.
pub fn secrets_env_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".config")
        .join("automation")
        .join("secrets.env")
}

/// Read `source_path` as a `.env` file and merge the discovered key=value pairs
/// into `~/.config/automation/secrets.env`, creating the file if needed.
///
/// Only keys that have non-empty values are written. Existing keys in
/// `secrets.env` are updated in-place; new keys are appended.
///
/// Returns the list of key names that were written (inserted or updated).
pub fn import_from_env_file(source_path: &Path) -> Result<Vec<String>, String> {
    let content = std::fs::read_to_string(source_path)
        .map_err(|e| format!("cannot read {}: {}", source_path.display(), e))?;

    // Parse source file into (key, raw_line) pairs — we keep the raw values as-is.
    let mut incoming: Vec<(String, String)> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || !trimmed.contains('=') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            let key = k.trim().to_string();
            let val = v.trim().to_string();
            if !key.is_empty() && !val.is_empty() {
                incoming.push((key, val));
            }
        }
    }

    if incoming.is_empty() {
        return Ok(vec![]);
    }

    let dest = secrets_env_path();
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create secrets dir: {}", e))?;
    }

    // Read existing secrets.env lines (or start empty).
    let existing_content = if dest.exists() {
        std::fs::read_to_string(&dest).map_err(|e| format!("cannot read secrets.env: {}", e))?
    } else {
        String::new()
    };

    let mut output_lines: Vec<String> = existing_content.lines().map(|l| l.to_string()).collect();

    let mut written: Vec<String> = Vec::new();

    for (key, val) in &incoming {
        let new_line = format!("{}={}", key, val);
        // Find and update in-place if key already present.
        let mut found = false;
        for line in output_lines.iter_mut() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') || !trimmed.contains('=') {
                continue;
            }
            if let Some((k, _)) = trimmed.split_once('=') {
                if k.trim() == key.as_str() {
                    *line = new_line.clone();
                    found = true;
                    break;
                }
            }
        }
        if !found {
            output_lines.push(new_line);
        }
        written.push(key.clone());
    }

    let mut final_content = output_lines.join("\n");
    if !final_content.ends_with('\n') {
        final_content.push('\n');
    }

    std::fs::write(&dest, &final_content)
        .map_err(|e| format!("cannot write secrets.env: {}", e))?;

    Ok(written)
}
