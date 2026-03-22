/// Execution ledger — JSONL event log and run summaries.
///
/// Mirrors Python `dashboard_ptk/dashboard/engine/ledger.py`.
/// Events are appended to `.github/automation/orchestrator_runs/{run_id}/events.jsonl`.
/// Summaries are written to `summary.json` and `summary.md`.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEvent {
    pub timestamp: String,
    pub event_type: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub project_id: String,
    pub started_at: String,
    pub finished_at: String,
    pub tasks_processed: usize,
    pub tasks_succeeded: usize,
    pub tasks_failed: usize,
    pub errors: Vec<String>,
}

// ─── Ledger ───────────────────────────────────────────────────────────────────

pub struct Ledger {
    runs_dir: PathBuf,
}

impl Ledger {
    /// Create a ledger rooted at `{repo_root}/.github/automation/orchestrator_runs/`.
    pub fn new(repo_root: &Path) -> Self {
        let runs_dir = repo_root
            .join(".github")
            .join("automation")
            .join("orchestrator_runs");
        Self { runs_dir }
    }

    /// Start a new run. Returns the `run_id` and the run directory path.
    pub fn start_run(&self, project_id: &str) -> Result<(String, PathBuf), String> {
        let run_id = Utc::now().format("%Y%m%d_%H%M%S").to_string();
        let run_dir = self.runs_dir.join(&run_id);
        std::fs::create_dir_all(&run_dir).map_err(|e| e.to_string())?;

        let metadata = serde_json::json!({
            "run_id": run_id,
            "project_id": project_id,
            "started_at": Utc::now().to_rfc3339(),
        });
        std::fs::write(
            run_dir.join("metadata.json"),
            serde_json::to_string_pretty(&metadata).unwrap_or_default(),
        )
        .map_err(|e| e.to_string())?;

        Ok((run_id, run_dir))
    }

    /// Append a single JSONL event to `{run_dir}/events.jsonl`.
    pub fn append_event(
        &self,
        run_dir: &Path,
        event_type: &str,
        payload: Value,
    ) -> Result<LedgerEvent, String> {
        let event = LedgerEvent {
            timestamp: Utc::now().to_rfc3339(),
            event_type: event_type.to_string(),
            payload,
        };
        let line = serde_json::to_string(&event).map_err(|e| e.to_string())?;

        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(run_dir.join("events.jsonl"))
            .map_err(|e| e.to_string())?;
        writeln!(file, "{}", line).map_err(|e| e.to_string())?;

        Ok(event)
    }

    /// Write `summary.json` and `summary.md` to the run directory.
    pub fn write_summary(&self, run_dir: &Path, summary: &RunSummary) -> Result<(), String> {
        std::fs::write(
            run_dir.join("summary.json"),
            serde_json::to_string_pretty(summary).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;

        let md = format!(
            "# Run {}\n\n\
             - **Project:** {}\n\
             - **Started:** {}\n\
             - **Finished:** {}\n\
             - **Processed:** {} tasks\n\
             - **Succeeded:** {}\n\
             - **Failed:** {}\n\
             {}\n",
            summary.run_id,
            summary.project_id,
            summary.started_at,
            summary.finished_at,
            summary.tasks_processed,
            summary.tasks_succeeded,
            summary.tasks_failed,
            if summary.errors.is_empty() {
                String::new()
            } else {
                format!("\n## Errors\n\n{}", summary.errors.join("\n"))
            },
        );

        std::fs::write(run_dir.join("summary.md"), md).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// List all run IDs in descending order (most recent first).
    pub fn list_runs(&self) -> Result<Vec<String>, String> {
        if !self.runs_dir.exists() {
            return Ok(vec![]);
        }
        let mut runs: Vec<String> = std::fs::read_dir(&self.runs_dir)
            .map_err(|e| e.to_string())?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                if entry.path().is_dir() {
                    entry.file_name().into_string().ok()
                } else {
                    None
                }
            })
            .collect();
        runs.sort_by(|a, b| b.cmp(a)); // newest first
        Ok(runs)
    }

    /// Load the summary for a given run ID.
    pub fn get_summary(&self, run_id: &str) -> Result<RunSummary, String> {
        let path = self.runs_dir.join(run_id).join("summary.json");
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| e.to_string())
    }

    /// Load all events for a given run ID.
    pub fn get_events(&self, run_id: &str) -> Result<Vec<LedgerEvent>, String> {
        let path = self.runs_dir.join(run_id).join("events.jsonl");
        if !path.exists() {
            return Ok(vec![]);
        }
        let content = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let events: Vec<LedgerEvent> = content
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        Ok(events)
    }
}
