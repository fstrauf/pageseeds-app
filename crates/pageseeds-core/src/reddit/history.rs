/// Manages the `_posted_history.json` file used by the PageSeeds CLI to track
/// which Reddit posts have already been handled (posted/skipped).
///
/// This file lives at: `{repo}/.github/automation/reddit/_posted_history.json`
///
/// The Python CLI stores entries as objects: `{"post_id": "...", "title": "...", "posted_at": "..."}`.
/// We read the `post_id` field from objects, or the string directly if the entry is a bare string.
/// When writing new entries from the Tauri app we also use the object format to stay compatible.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// An entry in the history file — may be a bare string (old format) or an object (CLI format).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum HistoryEntry {
    /// Bare string — older app-written format.
    Id(String),
    /// Object — written by the Python CLI.
    Object {
        post_id: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        posted_at: Option<String>,
        #[serde(default)]
        reason: Option<String>,
        #[serde(default)]
        skipped_at: Option<String>,
    },
}

impl HistoryEntry {
    fn post_id(&self) -> &str {
        match self {
            HistoryEntry::Id(s) => s,
            HistoryEntry::Object { post_id, .. } => post_id,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct HistoryFile {
    #[serde(default)]
    posted: Vec<HistoryEntry>,
    #[serde(default)]
    skipped: Vec<HistoryEntry>,
}

pub struct RedditHistoryManager {
    path: PathBuf,
}

impl RedditHistoryManager {
    /// Create a manager for the given project repo root.
    ///
    /// File path: `{repo_root}/.github/automation/reddit/_posted_history.json`
    pub fn new(repo_root: &Path) -> Self {
        RedditHistoryManager {
            path: repo_root
                .join(".github")
                .join("automation")
                .join("reddit")
                .join("_posted_history.json"),
        }
    }

    /// Return all post_ids that have been handled (posted or skipped).
    pub fn get_all_handled_ids(&self) -> HashSet<String> {
        let history = self.load();
        let mut ids: HashSet<String> = HashSet::new();
        for e in &history.posted {
            ids.insert(e.post_id().to_string());
        }
        for e in &history.skipped {
            ids.insert(e.post_id().to_string());
        }
        ids
    }

    /// Check whether a single post_id has already been handled.
    #[allow(dead_code)]
    pub fn is_handled(&self, post_id: &str) -> bool {
        let history = self.load();
        history.posted.iter().any(|e| e.post_id() == post_id)
            || history.skipped.iter().any(|e| e.post_id() == post_id)
    }

    /// Mark a post as posted. Idempotent — safe to call multiple times.
    /// Writes an object entry matching the CLI format.
    pub fn mark_posted(&self, post_id: &str) -> Result<(), String> {
        let mut history = self.load();
        if !history.posted.iter().any(|e| e.post_id() == post_id) {
            // Remove from skipped if previously skipped.
            history.skipped.retain(|e| e.post_id() != post_id);
            history.posted.push(HistoryEntry::Object {
                post_id: post_id.to_string(),
                title: None,
                posted_at: Some(chrono::Utc::now().to_rfc3339()),
                reason: None,
                skipped_at: None,
            });
            self.save(&history)?;
        }
        Ok(())
    }

    /// Mark a post as skipped. Idempotent — safe to call multiple times.
    /// Writes an object entry matching the CLI format.
    pub fn mark_skipped(&self, post_id: &str) -> Result<(), String> {
        let mut history = self.load();
        if !history.skipped.iter().any(|e| e.post_id() == post_id)
            && !history.posted.iter().any(|e| e.post_id() == post_id)
        {
            history.skipped.push(HistoryEntry::Object {
                post_id: post_id.to_string(),
                title: None,
                posted_at: None,
                reason: None,
                skipped_at: Some(chrono::Utc::now().to_rfc3339()),
            });
            self.save(&history)?;
        }
        Ok(())
    }

    // ─── Private ──────────────────────────────────────────────────────────────

    fn load(&self) -> HistoryFile {
        let content = match std::fs::read_to_string(&self.path) {
            Ok(s) => s,
            Err(_) => return HistoryFile::default(),
        };
        serde_json::from_str(&content).unwrap_or_else(|e| {
            log::warn!(
                "[history] failed to parse history file: {} — treating as empty",
                e
            );
            HistoryFile::default()
        })
    }

    fn save(&self, history: &HistoryFile) -> Result<(), String> {
        // Ensure the parent directory exists.
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create history dir: {}", e))?;
        }
        let json = serde_json::to_string_pretty(history)
            .map_err(|e| format!("Failed to serialize history: {}", e))?;
        std::fs::write(&self.path, json).map_err(|e| format!("Failed to write history file: {}", e))
    }
}
