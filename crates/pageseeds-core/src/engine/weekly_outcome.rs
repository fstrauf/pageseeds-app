//! Read-only loader for the weekly SEO outcome sidecar JSON.
//!
//! Prefer `{automation}/weekly_outcome_latest.json`, then newest
//! `weekly_outcome_YYYYMMDD_HHMMSS.json` by filename. Never invent data.
//!
//! Schema (epic #326 freeze): `docs/examples/weekly_outcome_example.json`.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::engine::project_paths::ProjectPaths;
use crate::error::{Error, Result};

/// Expected schema version for soft-validation.
pub const WEEKLY_OUTCOME_SCHEMA_VERSION: u32 = 1;
/// Expected kind string for soft-validation.
pub const WEEKLY_OUTCOME_KIND: &str = "weekly_seo_outcome";

const LATEST_FILENAME: &str = "weekly_outcome_latest.json";
const TIMESTAMPED_PREFIX: &str = "weekly_outcome_";
const TIMESTAMPED_SUFFIX: &str = ".json";

/// One measure row from the weekly outcome.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeeklyOutcomeMeasure {
    #[serde(default)]
    pub measure: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub task: String,
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub task_ids: Vec<String>,
}

/// One decision row from the weekly outcome.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeeklyOutcomeDecision {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub pending: String,
    #[serde(default)]
    pub guidance: String,
    #[serde(default)]
    pub commands: Vec<String>,
    #[serde(default)]
    pub related_task_ids: Vec<String>,
    #[serde(default)]
    pub related_slugs: Vec<String>,
    #[serde(default)]
    pub not_before: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub source_report: String,
}

/// Full weekly SEO outcome document (schema_version 1).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeeklyOutcome {
    pub schema_version: u32,
    pub kind: String,
    pub project_id: String,
    #[serde(default)]
    pub project_name: String,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub report_path: String,
    #[serde(default)]
    pub report_date: Option<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub measure_only: bool,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub headline: String,
    #[serde(default)]
    pub measures: Vec<WeeklyOutcomeMeasure>,
    #[serde(default)]
    pub decisions: Vec<WeeklyOutcomeDecision>,
    #[serde(default)]
    pub recommended_next: Vec<String>,
    #[serde(default)]
    pub posthog_warn: bool,
    #[serde(default)]
    pub followup_prompt: String,
}

/// Compact open-decision row for `--summary`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OpenDecisionSummary {
    pub id: String,
    pub title: String,
    pub kind: String,
}

/// Compact operator view (`pageseeds-cli weekly-outcome --summary`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WeeklyOutcomeSummary {
    pub project_id: String,
    pub status: String,
    pub report_path: String,
    pub generated_at: String,
    pub open_operator_count: usize,
    pub open_decisions: Vec<OpenDecisionSummary>,
    pub posthog_warn: bool,
}

/// Loaded outcome plus resolution metadata and soft-validation warnings.
#[derive(Debug, Clone)]
pub struct WeeklyOutcomeLoad {
    pub outcome: WeeklyOutcome,
    pub source_path: PathBuf,
    pub warnings: Vec<String>,
}

impl WeeklyOutcomeLoad {
    /// Serialize for full CLI stdout: outcome fields, plus optional `warnings`.
    pub fn to_cli_value(&self) -> Result<serde_json::Value> {
        let mut value = serde_json::to_value(&self.outcome)?;
        if !self.warnings.is_empty() {
            if let Some(obj) = value.as_object_mut() {
                obj.insert(
                    "warnings".to_string(),
                    serde_json::to_value(&self.warnings)?,
                );
            }
        }
        Ok(value)
    }
}

/// Resolve, read, and soft-validate the latest weekly outcome for a project.
pub fn load_weekly_outcome(project_path: &Path) -> Result<WeeklyOutcomeLoad> {
    let paths = ProjectPaths::from_path(&project_path.to_string_lossy());
    let source_path = resolve_weekly_outcome_path(&paths)?;
    let raw = fs::read_to_string(&source_path).map_err(|e| {
        Error::Other(format!(
            "failed to read weekly outcome {}: {e}",
            source_path.display()
        ))
    })?;
    let outcome: WeeklyOutcome = serde_json::from_str(&raw).map_err(|e| {
        Error::InvalidJson(format!(
            "invalid weekly outcome JSON at {}: {e}",
            source_path.display()
        ))
    })?;
    let warnings = soft_validate(&outcome);
    Ok(WeeklyOutcomeLoad {
        outcome,
        source_path,
        warnings,
    })
}

/// Compact summary for operators / cron status scripts.
pub fn weekly_outcome_summary(outcome: &WeeklyOutcome) -> WeeklyOutcomeSummary {
    let open_decisions: Vec<OpenDecisionSummary> = outcome
        .decisions
        .iter()
        .filter(|d| d.status == "open")
        .map(|d| OpenDecisionSummary {
            id: d.id.clone(),
            title: d.title.clone(),
            kind: d.kind.clone(),
        })
        .collect();

    let open_operator_count = outcome
        .decisions
        .iter()
        .filter(|d| {
            d.status == "open" && (d.kind == "operator_act" || d.kind == "operator_confirm")
        })
        .count();

    WeeklyOutcomeSummary {
        project_id: outcome.project_id.clone(),
        status: outcome.status.clone(),
        report_path: outcome.report_path.clone(),
        generated_at: outcome.generated_at.clone(),
        open_operator_count,
        open_decisions,
        posthog_warn: outcome.posthog_warn,
    }
}

/// Prefer `weekly_outcome_latest.json`, else newest timestamped `weekly_outcome_*.json`.
pub fn resolve_weekly_outcome_path(paths: &ProjectPaths) -> Result<PathBuf> {
    let automation = paths.automation_dir();
    let latest = automation.join(LATEST_FILENAME);
    if latest.is_file() {
        return Ok(latest);
    }

    if let Some(newest) = find_newest_timestamped_outcome(automation) {
        return Ok(newest);
    }

    Err(Error::ConfigMissing(format!(
        "weekly outcome not found; expected {} (or weekly_outcome_YYYYMMDD_HHMMSS.json)",
        latest.display()
    )))
}

fn find_newest_timestamped_outcome(automation: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(automation).ok()?;
    let mut candidates: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            name.starts_with(TIMESTAMPED_PREFIX)
                && name.ends_with(TIMESTAMPED_SUFFIX)
                && name != LATEST_FILENAME
        })
        .collect();

    // Filename descending: `weekly_outcome_YYYYMMDD_HHMMSS.json` sorts newest first.
    candidates.sort_by(|a, b| {
        let an = a.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let bn = b.file_name().and_then(|n| n.to_str()).unwrap_or("");
        bn.cmp(an)
    });
    candidates.into_iter().next()
}

fn soft_validate(outcome: &WeeklyOutcome) -> Vec<String> {
    let mut warnings = Vec::new();
    if outcome.schema_version != WEEKLY_OUTCOME_SCHEMA_VERSION {
        warnings.push(format!(
            "unknown schema_version {} (expected {})",
            outcome.schema_version, WEEKLY_OUTCOME_SCHEMA_VERSION
        ));
    }
    if outcome.kind != WEEKLY_OUTCOME_KIND {
        warnings.push(format!(
            "unexpected kind {:?} (expected {:?})",
            outcome.kind, WEEKLY_OUTCOME_KIND
        ));
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const EXAMPLE_JSON: &str = r#"{
  "schema_version": 1,
  "kind": "weekly_seo_outcome",
  "project_id": "coffee",
  "project_name": "Brewedlate",
  "generated_at": "2026-07-28T18:31:04Z",
  "report_path": "/Users/example/coffee/.github/automation/weekly_seo_20260728_183104.md",
  "report_date": "2026-07-28",
  "status": "needs_attention",
  "mode": "harvest",
  "measure_only": false,
  "summary": "Harvest week summary",
  "headline": "2 fixes shipped · 1 open operator act",
  "measures": [
    {
      "measure": "Path B CTR fix",
      "evidence": "GSC",
      "task": "fix-submit -k ctr",
      "outcome": "submitted",
      "task_ids": ["task-ctr-001"]
    }
  ],
  "decisions": [
    {
      "id": "d-op-act",
      "title": "Diagnostics fan-out",
      "kind": "operator_act",
      "status": "open",
      "pending": "Drain",
      "guidance": "Prefer list-tasks",
      "commands": [],
      "related_task_ids": [],
      "related_slugs": [],
      "not_before": null,
      "expires_at": null,
      "source_report": "weekly_seo.md"
    },
    {
      "id": "d-op-confirm",
      "title": "Noindex thin post",
      "kind": "operator_confirm",
      "status": "open",
      "pending": "Human noindex",
      "guidance": "Confirm required",
      "commands": [],
      "related_task_ids": [],
      "related_slugs": [],
      "not_before": null,
      "expires_at": null,
      "source_report": "weekly_seo.md"
    },
    {
      "id": "d-product-gap",
      "title": "Bulk noindex missing",
      "kind": "product_gap",
      "status": "open",
      "pending": "Escalate",
      "guidance": "Report only",
      "commands": [],
      "related_task_ids": [],
      "related_slugs": [],
      "not_before": null,
      "expires_at": null,
      "source_report": "weekly_seo.md"
    },
    {
      "id": "d-waiting",
      "title": "GSC lag",
      "kind": "waiting",
      "status": "watching",
      "pending": "Wait",
      "guidance": "Re-check later",
      "commands": [],
      "related_task_ids": [],
      "related_slugs": [],
      "not_before": null,
      "expires_at": null,
      "source_report": "weekly_seo.md"
    },
    {
      "id": "d-optional",
      "title": "Optional push",
      "kind": "optional_backlog",
      "status": "deferred",
      "pending": "Skipped",
      "guidance": "Next harvest",
      "commands": [],
      "related_task_ids": [],
      "related_slugs": [],
      "not_before": null,
      "expires_at": null,
      "source_report": "weekly_seo.md"
    }
  ],
  "recommended_next": ["Drain diagnostics"],
  "posthog_warn": false,
  "followup_prompt": "Continue from last weekly SEO run."
}"#;

    fn tempfile_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-weekly-outcome-{}-{}-{}",
            tag,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn project_with_automation(tag: &str) -> (PathBuf, PathBuf) {
        let root = tempfile_dir(tag);
        let automation = root.join(".github").join("automation");
        fs::create_dir_all(&automation).unwrap();
        (root, automation)
    }

    #[test]
    fn load_from_latest_json() {
        let (root, automation) = project_with_automation("latest");
        fs::write(automation.join(LATEST_FILENAME), EXAMPLE_JSON).unwrap();

        let loaded = load_weekly_outcome(&root).expect("load latest");
        assert_eq!(loaded.outcome.project_id, "coffee");
        assert_eq!(loaded.outcome.kind, WEEKLY_OUTCOME_KIND);
        assert_eq!(loaded.outcome.schema_version, 1);
        assert!(loaded.warnings.is_empty());
        assert!(loaded
            .source_path
            .ends_with("weekly_outcome_latest.json"));
    }

    #[test]
    fn missing_path_returns_config_missing() {
        let (root, _automation) = project_with_automation("missing");
        let err = load_weekly_outcome(&root).expect_err("must not invent data");
        match err {
            Error::ConfigMissing(msg) => {
                assert!(
                    msg.contains("weekly outcome not found"),
                    "message should describe missing outcome: {msg}"
                );
                assert!(
                    msg.contains("weekly_outcome_latest.json"),
                    "message should include expected path: {msg}"
                );
            }
            other => panic!("expected ConfigMissing, got {other:?}"),
        }
    }

    #[test]
    fn fallback_picks_newest_timestamped_filename() {
        let (root, automation) = project_with_automation("fallback");
        // Older then newer by filename timestamp.
        fs::write(
            automation.join("weekly_outcome_20260101_120000.json"),
            EXAMPLE_JSON.replace("\"coffee\"", "\"old\""),
        )
        .unwrap();
        fs::write(
            automation.join("weekly_outcome_20260728_183104.json"),
            EXAMPLE_JSON,
        )
        .unwrap();

        let loaded = load_weekly_outcome(&root).expect("load timestamped");
        assert_eq!(loaded.outcome.project_id, "coffee");
        assert!(
            loaded
                .source_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap()
                .contains("20260728_183104"),
            "should pick newest filename, got {:?}",
            loaded.source_path
        );
    }

    #[test]
    fn latest_preferred_over_timestamped() {
        let (root, automation) = project_with_automation("prefer-latest");
        fs::write(
            automation.join("weekly_outcome_20260728_183104.json"),
            EXAMPLE_JSON.replace("\"coffee\"", "\"timestamped\""),
        )
        .unwrap();
        fs::write(
            automation.join(LATEST_FILENAME),
            EXAMPLE_JSON.replace("\"coffee\"", "\"latest\""),
        )
        .unwrap();

        let loaded = load_weekly_outcome(&root).expect("prefer latest");
        assert_eq!(loaded.outcome.project_id, "latest");
    }

    #[test]
    fn summary_counts_open_operator_decisions() {
        let outcome: WeeklyOutcome = serde_json::from_str(EXAMPLE_JSON).unwrap();
        let summary = weekly_outcome_summary(&outcome);

        assert_eq!(summary.project_id, "coffee");
        assert_eq!(summary.status, "needs_attention");
        assert!(!summary.posthog_warn);
        // operator_act + operator_confirm open; product_gap open is not operator.
        assert_eq!(summary.open_operator_count, 2);
        // All status=open: operator_act, operator_confirm, product_gap
        assert_eq!(summary.open_decisions.len(), 3);
        let kinds: Vec<&str> = summary
            .open_decisions
            .iter()
            .map(|d| d.kind.as_str())
            .collect();
        assert!(kinds.contains(&"operator_act"));
        assert!(kinds.contains(&"operator_confirm"));
        assert!(kinds.contains(&"product_gap"));
        assert!(!kinds.contains(&"waiting")); // watching, not open
        assert!(!kinds.contains(&"optional_backlog")); // deferred
    }

    #[test]
    fn soft_warn_on_wrong_schema_still_returns_data() {
        let (root, automation) = project_with_automation("soft-warn");
        let mut bad: serde_json::Value = serde_json::from_str(EXAMPLE_JSON).unwrap();
        bad["schema_version"] = serde_json::json!(2);
        bad["kind"] = serde_json::json!("other_kind");
        fs::write(automation.join(LATEST_FILENAME), bad.to_string()).unwrap();

        let loaded = load_weekly_outcome(&root).expect("soft-fail only");
        assert_eq!(loaded.outcome.project_id, "coffee");
        assert_eq!(loaded.outcome.schema_version, 2);
        assert_eq!(loaded.warnings.len(), 2);
        assert!(loaded.warnings.iter().any(|w| w.contains("schema_version")));
        assert!(loaded.warnings.iter().any(|w| w.contains("kind")));

        let cli = loaded.to_cli_value().unwrap();
        assert_eq!(cli["project_id"], "coffee");
        assert!(cli.get("warnings").is_some());
        assert_eq!(cli["warnings"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn invalid_json_hard_fails() {
        let (root, automation) = project_with_automation("bad-json");
        fs::write(automation.join(LATEST_FILENAME), "{not json").unwrap();
        let err = load_weekly_outcome(&root).expect_err("invalid json");
        match err {
            Error::InvalidJson(msg) => {
                assert!(msg.contains("invalid weekly outcome JSON"), "{msg}");
            }
            other => panic!("expected InvalidJson, got {other:?}"),
        }
    }
}
