//! Shared harness for live LLM eval regression tests.
//!
//! Each pipeline eval (`ctr_fix_evals`, `content_fix_evals`, `content_review_evals`,
//! `content_review_investigate_evals`) runs production helpers / extract against a
//! live backend over committed fixtures in `fixtures/evals/`, then applies two layers
//! of checks:
//!
//! 1. **Deterministic contract checks** — plain Rust assertions encoding the output
//!    contract (length limits, keyword presence, no hallucinated link slugs). Free,
//!    reproducible, and the primary regression signal.
//! 2. **LLM judge** — `rig::evals::LlmScoreMetric` scoring output quality (0-1,
//!    threshold 0.7). Requires a judge provider key; skipped with a note otherwise.
//!
//! Config via env:
//! - `EVAL_PROVIDER` — generation backend: `kimi` (default, CLI connector), `claude`, `openai`, `ollama`
//! - `EVAL_JUDGE_PROVIDER` — judge provider: `claude`/`anthropic` or `openai`.
//!   Auto-detected from `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` when unset.
//!
//! Run: `cargo test evals -- --ignored --nocapture` (or `scripts/run-evals.sh`).

pub mod content_fix_evals;
pub mod content_review_evals;
pub mod content_review_investigate_evals;
pub mod ctr_fix_evals;

use std::path::{Path, PathBuf};

use rig::client::CompletionClient;
use rig::evals::{Eval, EvalOutcome, LlmScoreMetricBuilder, LlmScoreMetricScore};
use serde::de::DeserializeOwned;

use crate::models::task::{self, Task, TaskArtifact};
use crate::rig::provider::LlmBackend;

pub const FIXTURES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/evals");

/// Global lock serializing live eval suites: cargo runs test fns in parallel by
/// default, and concurrent LLM calls get rate-limited (bridge) or spawn competing
/// CLI subprocesses. Every live eval test must hold this lock for its whole run.
pub static EVAL_LOCK: once_cell::sync::Lazy<tokio::sync::Mutex<()>> =
    once_cell::sync::Lazy::new(|| tokio::sync::Mutex::new(()));

/// LLM judge pass threshold (0-1 scale).
pub const JUDGE_THRESHOLD: f64 = 0.7;
/// Fraction of judged cases that must pass for the suite to pass
/// (LLM judging is non-deterministic — rig docs recommend aggregate thresholds).
pub const JUDGE_PASS_RATE: f64 = 2.0 / 3.0;

// ─── Fixtures ────────────────────────────────────────────────────────────────

/// List fixture case files for a pipeline, sorted by name.
pub fn list_cases(pipeline: &str) -> Vec<PathBuf> {
    let dir = Path::new(FIXTURES_DIR).join(pipeline);
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read fixture dir {}: {}", dir.display(), e))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    cases.sort();
    assert!(
        !cases.is_empty(),
        "no fixture cases found in {}",
        dir.display()
    );
    cases
}

/// Load a fixture case file as JSON.
pub fn load_fixture<T: DeserializeOwned>(path: &Path) -> T {
    let raw = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read fixture {}: {}", path.display(), e));
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("cannot parse fixture {}: {}", path.display(), e))
}

/// Write the fixture's MDX article into a fresh temp project dir and return the dir.
/// The temp dir leaks (tests are short-lived); uniqueness avoids cross-test interference.
pub fn temp_project_with_mdx(mdx_path: &str, mdx: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pageseeds-eval-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let file = dir.join(mdx_path);
    std::fs::create_dir_all(file.parent().expect("mdx_path has a parent"))
        .expect("create temp content dir");
    std::fs::write(&file, mdx).expect("write fixture mdx");
    dir
}

// ─── Task scaffolding ────────────────────────────────────────────────────────

/// Minimal task fixture carrying a single artifact, mirroring the shape the
/// production exec steps read (`extract_recommendation` / `extract_context`).
pub fn task_with_artifact(task_type: &str, key: &str, content: String) -> Task {
    Task {
        id: format!("eval-{}", uuid::Uuid::new_v4()),
        project_id: "eval-project".to_string(),
        task_type: task_type.to_string(),
        phase: "implementation".to_string(),
        status: task::TaskStatus::InProgress,
        priority: task::Priority::Medium,
        run_policy: task::TaskRunPolicy::AutoEnqueue,
        review_surface: task::TaskReviewSurface::None,
        follow_up_policy: task::FollowUpPolicy::None,
        agent_policy: task::AgentPolicy::None,
        title: Some(format!("eval {}", task_type)),
        description: None,
        depends_on: vec![],
        artifacts: vec![TaskArtifact {
            key: key.to_string(),
            path: None,
            artifact_type: None,
            source: None,
            content: Some(content),
        }],
        run: task::TaskRun::default(),
        created_at: chrono::Utc::now().to_rfc3339(),
        not_before: None,
        updated_at: chrono::Utc::now().to_rfc3339(),
    }
}

// ─── Backends ────────────────────────────────────────────────────────────────

/// Resolve the generation backend for evals. Explicit `Some("cli")` mode keeps
/// this hermetic (no app DB read in `resolve_backend`) and routes Kimi through
/// the native CLI connector (the bridge is no longer used). Panics with guidance
/// when no provider is reachable — eval tests are `#[ignore]`d and expect credentials.
pub async fn generation_backend() -> LlmBackend {
    let provider = std::env::var("EVAL_PROVIDER").unwrap_or_else(|_| "kimi".to_string());
    crate::rig::provider::resolve_backend(&provider, None, None, Some("cli"))
        .await
        .unwrap_or_else(|e| {
            panic!(
                "could not resolve EVAL_PROVIDER={} backend: {}. \
                 Install the kimi CLI (or set ANTHROPIC_API_KEY/OPENAI_API_KEY and EVAL_PROVIDER).",
                provider, e
            )
        })
}

/// Score `input` with the LLM judge against `criteria`. Returns `None` when no
/// judge provider is configured (deterministic checks still gate the suite).
pub async fn judge_score(
    criteria: &[&str],
    input: String,
) -> Option<EvalOutcome<LlmScoreMetricScore>> {
    let configured = std::env::var("EVAL_JUDGE_PROVIDER").ok();
    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").ok();
    let openai_key = std::env::var("OPENAI_API_KEY").ok();

    let choice = configured.as_deref().map(str::to_lowercase).or_else(|| {
        if anthropic_key.is_some() {
            Some("claude".to_string())
        } else if openai_key.is_some() {
            Some("openai".to_string())
        } else {
            None
        }
    })?;

    match choice.as_str() {
        "claude" | "anthropic" => {
            let key = anthropic_key.expect("ANTHROPIC_API_KEY required for claude judge");
            let client = rig::providers::anthropic::Client::new(&key)
                .expect("failed to build anthropic judge client");
            let model = crate::rig::provider::default_model_for_provider("claude");
            let metric = LlmScoreMetricBuilder::new(client.extractor::<LlmScoreMetricScore>(&model));
            let metric = criteria
                .iter()
                .fold(metric, |m, c| m.criteria(c))
                .threshold(JUDGE_THRESHOLD)
                .build()
                .expect("judge build failed");
            Some(metric.eval(input).await)
        }
        "openai" => {
            let key = openai_key.expect("OPENAI_API_KEY required for openai judge");
            let client = rig::providers::openai::Client::new(&key)
                .expect("failed to build openai judge client");
            let model = crate::rig::provider::default_model_for_provider("openai");
            let metric = LlmScoreMetricBuilder::new(client.extractor::<LlmScoreMetricScore>(&model));
            let metric = criteria
                .iter()
                .fold(metric, |m, c| m.criteria(c))
                .threshold(JUDGE_THRESHOLD)
                .build()
                .expect("judge build failed");
            Some(metric.eval(input).await)
        }
        other => panic!(
            "unknown EVAL_JUDGE_PROVIDER={} (expected claude|anthropic|openai)",
            other
        ),
    }
}

// ─── Reporting ───────────────────────────────────────────────────────────────

pub struct CaseReport {
    pub name: String,
    pub violations: Vec<String>,
    pub judge: Option<EvalOutcome<LlmScoreMetricScore>>,
}

/// Print the suite summary and assert the gates:
/// - zero deterministic violations across all cases
/// - judge pass-rate >= JUDGE_PASS_RATE (when a judge ran at all)
pub fn finish_suite(pipeline: &str, reports: Vec<CaseReport>) {
    println!("\n=== eval suite: {} ({} cases) ===", pipeline, reports.len());

    let mut violation_total = 0usize;
    let mut judged = 0usize;
    let mut judge_passed = 0usize;

    for report in &reports {
        let judge_str = match &report.judge {
            Some(EvalOutcome::Pass(s)) => {
                judged += 1;
                judge_passed += 1;
                format!("judge PASS ({:.2})", s.score)
            }
            Some(EvalOutcome::Fail(s)) => {
                judged += 1;
                format!("judge FAIL ({:.2}): {}", s.score, s.feedback)
            }
            Some(EvalOutcome::Invalid(reason)) => {
                judged += 1;
                format!("judge INVALID: {}", reason)
            }
            None => "judge skipped (no provider key)".to_string(),
        };

        if report.violations.is_empty() {
            println!("  [ok]   {} — {}", report.name, judge_str);
        } else {
            violation_total += report.violations.len();
            println!("  [FAIL] {} — {}", report.name, judge_str);
            for v in &report.violations {
                println!("         violation: {}", v);
            }
        }
    }

    assert_eq!(
        violation_total, 0,
        "eval suite {} produced {} deterministic contract violation(s)",
        pipeline, violation_total
    );

    if judged > 0 {
        let rate = judge_passed as f64 / judged as f64;
        println!(
            "  judge pass-rate: {}/{} ({:.0}%, required >= {:.0}%)",
            judge_passed,
            judged,
            rate * 100.0,
            JUDGE_PASS_RATE * 100.0
        );
        assert!(
            rate >= JUDGE_PASS_RATE,
            "eval suite {} judge pass-rate {:.2} below required {:.2}",
            pipeline,
            rate,
            JUDGE_PASS_RATE
        );
    } else {
        println!("  judge: no cases judged (set ANTHROPIC_API_KEY or OPENAI_API_KEY to enable)");
    }
}
