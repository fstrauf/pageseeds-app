/// Trait-based workflow handlers — one per task family.
///
/// Each handler knows:
///   - which task types it owns (`supports`)
///   - what steps the task needs (`plan`)
///
/// Step execution happens in `executor.rs`; handlers only describe the plan.

use super::{StepResult, WorkflowStep};
use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

// ─── Trait ────────────────────────────────────────────────────────────────────

pub trait WorkflowHandler: Send + Sync {
    fn supports(&self, task: &Task) -> bool;
    fn plan(&self, task: &Task) -> Vec<WorkflowStep>;
}

// ─── Helper ───────────────────────────────────────────────────────────────────

fn task_type(t: &Task) -> &str {
    &t.task_type
}

// ─── Collection ───────────────────────────────────────────────────────────────

pub struct CollectionHandler;

impl WorkflowHandler for CollectionHandler {
    fn supports(&self, task: &Task) -> bool {
        matches!(task_type(task), "collect_gsc" | "collect_posthog")
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task_type(task) {
            "collect_gsc" => vec![
                WorkflowStep::new("collect_gsc_run", "deterministic")
                    .with_param("cmd", "pageseeds automation seo gsc-sync-articles --workspace-dir {automation_dir} --days 90"),
            ],
            // collect_posthog has no CLI implementation yet — fall back to agent.
            _ => vec![WorkflowStep::new("collect_agent_stage", "agentic")],
        }
    }
}

// ─── Investigation ────────────────────────────────────────────────────────────

pub struct InvestigationHandler;

impl WorkflowHandler for InvestigationHandler {
    fn supports(&self, task: &Task) -> bool {
        matches!(task_type(task), "investigate_gsc" | "investigate_posthog")
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task_type(task) {
            "investigate_gsc" => vec![
                WorkflowStep::new("investigate_gsc_run", "deterministic")
                    .with_param("cmd", "pageseeds automation seo content-audit --workspace-dir {automation_dir}"),
            ],
            // investigate_posthog has no CLI implementation yet — fall back to agent.
            _ => vec![WorkflowStep::new("investigate_agent_stage", "agentic")],
        }
    }
}

// ─── Research ─────────────────────────────────────────────────────────────────

pub struct ResearchHandler;

impl WorkflowHandler for ResearchHandler {
    fn supports(&self, task: &Task) -> bool {
        matches!(
            task_type(task),
            "research_keywords" | "custom_keyword_research" | "research_landing_pages"
        )
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        let mut steps = vec![WorkflowStep::new("research_agent_stage", "agentic")];

        if task_type(task) == "custom_keyword_research" {
            steps.push(
                WorkflowStep::new("research_normalize_stage", "normalizer")
                    .with_param("normalizer_id", "keyword_research")
                    .with_param("artifact_name", "keyword_research"),
            );
        }

        steps
    }
}

// ─── Content ──────────────────────────────────────────────────────────────────

pub struct ContentHandler;

impl WorkflowHandler for ContentHandler {
    fn supports(&self, task: &Task) -> bool {
        matches!(
            task_type(task),
            "write_article" | "optimize_article" | "create_content" | "optimize_content"
                | "content_review_apply"
        )
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        if task_type(task) == "content_review_apply" {
            // Dedicated step runner that reads the recommendations artifact and
            // builds a structured apply prompt — not a generic skill/agentic call.
            return vec![WorkflowStep::new("content_review_apply_execute", "content_review_apply_execute")];
        }
        // Agentic: the agent reads the article spec and writes the MDX file.
        vec![WorkflowStep::new("content_write_stage", "agentic")]
    }
}

// ─── Content Review ───────────────────────────────────────────────────────────

pub struct ContentReviewHandler;

impl WorkflowHandler for ContentReviewHandler {
    fn supports(&self, task: &Task) -> bool {
        matches!(task_type(task), "content_review" | "content_audit")
    }

    fn plan(&self, _task: &Task) -> Vec<WorkflowStep> {
        vec![
            // Step 1: fetch GSC page metrics and write into articles.json.
            // Optional — a missing service account skips gracefully rather than aborting.
            WorkflowStep::new("content_review_gsc_sync", "gsc_sync_articles")
                .optional(),
            // Step 2: deterministic multi-check audit → writes content_audit.json.
            // Optional — still valuable even without GSC data.
            WorkflowStep::new("content_review_audit", "content_audit")
                .optional(),
            // Step 3: native sync — validates articles.json ↔ content files, dates.
            WorkflowStep::new("content_review_sync", "content_sync")
                .optional(),
            // Step 4: select priority articles, build structured context, get agent recommendations.
            // One focused agent call (not N calls). Writes recommendations.json.
            WorkflowStep::new("content_review_recommend", "content_review_recommend"),
        ]
    }
}

// ─── Implementation ───────────────────────────────────────────────────────────

pub struct ImplementationHandler;

impl WorkflowHandler for ImplementationHandler {
    fn supports(&self, task: &Task) -> bool {
        let t = task_type(task);
        // Only claim types that are explicitly listed or named like fix_*.
        // Do NOT use a phase catch-all — that was the root cause of content_review being
        // silently captured and generating a bogus `pageseeds content review run` command.
        // Unknown task types fall through to ManualFallbackHandler instead.
        matches!(
            t,
            "cluster_and_link"
                | "content_cleanup"
                | "publish_content"
                | "indexing_diagnostics"
                | "content_strategy"
                | "technical_fix"
                | "landing_page_spec"
        ) || t.starts_with("fix_")
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        let cmd = match task_type(task) {
            "content_cleanup" => "pageseeds content clean --workspace-dir {automation_dir}",
            "publish_content" => "pageseeds content validate --workspace-dir {automation_dir}",
            "cluster_and_link" => "pageseeds content scan-internal-links --workspace-dir {automation_dir}",
            // For other implementation types (fix_*, indexing_diagnostics, etc.),
            // fall back to agentic execution — no CLI command reliably maps to these.
            _ => return vec![WorkflowStep::new("implementation_agent_stage", "agentic")],
        };
        vec![WorkflowStep::new(&format!("{}_run", task_type(task)), "deterministic")
            .with_param("cmd", cmd)]
    }
}

// ─── Reddit ───────────────────────────────────────────────────────────────────

pub struct RedditHandler;

impl WorkflowHandler for RedditHandler {
    fn supports(&self, task: &Task) -> bool {
        task_type(task).starts_with("reddit_")
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        if task_type(task) == "reddit_opportunity_search" {
            // Deterministic search — reads reddit_config.md, calls CLI, no agent required.
            vec![WorkflowStep::new("reddit_search_stage", "reddit_search")]
        } else {
            // Other reddit tasks (e.g. reply drafting) still use agent + optional normalizer.
            let mut steps = vec![WorkflowStep::new("reddit_agent_stage", "agentic")];
            steps.push(
                WorkflowStep::new("reddit_normalize_stage", "normalizer")
                    .with_param("normalizer_id", "reddit_opportunities")
                    .with_param("artifact_name", "reddit_opportunities")
                    .optional(),
            );
            steps
        }
    }
}

// ─── Performance ─────────────────────────────────────────────────────────────

pub struct PerformanceHandler;

impl WorkflowHandler for PerformanceHandler {
    fn supports(&self, task: &Task) -> bool {
        task_type(task) == "analyze_gsc_performance"
    }

    fn plan(&self, _task: &Task) -> Vec<WorkflowStep> {
        // GSC performance analysis — agent-backed until a native Rust implementation exists.
        vec![WorkflowStep::new("performance_agent_stage", "agentic")]
    }
}

// ─── Manual Fallback ─────────────────────────────────────────────────────────

pub struct ManualFallbackHandler;

impl WorkflowHandler for ManualFallbackHandler {
    fn supports(&self, _task: &Task) -> bool {
        true
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        vec![WorkflowStep::new(&format!("{}_manual", task_type(task)), "manual")]
    }
}

// ─── Registry ─────────────────────────────────────────────────────────────────

/// Default ordered handler list (most specific first, fallback last).
pub fn default_handlers() -> Vec<Box<dyn WorkflowHandler>> {
    vec![
        Box::new(CollectionHandler),
        Box::new(InvestigationHandler),
        Box::new(ResearchHandler),
        Box::new(ContentHandler),
        Box::new(ContentReviewHandler),
        Box::new(RedditHandler),
        Box::new(PerformanceHandler),
        Box::new(ImplementationHandler),
        Box::new(ManualFallbackHandler),
    ]
}

// ─── Step execution helpers (used by executor) ────────────────────────────────

/// Execute a deterministic step by invoking an installed CLI tool via shell.
/// Returns the captured stdout/stderr and success flag.
///
/// The `cmd` param MUST be set explicitly on the WorkflowStep via `.with_param("cmd", "...")`.
/// Auto-generation of CLI commands from step names was removed because it silently produced
/// broken commands (e.g. step `content_review_run` → `pageseeds content review run`).
///
/// Supported tokens in `cmd`:
///   {project_path}   → repo root
///   {automation_dir} → repo/.github/automation
pub fn exec_deterministic(step: &WorkflowStep, _task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let automation_dir = paths.automation_dir.to_string_lossy();

    let raw_cmd = match step.params.get("cmd") {
        Some(c) => c.clone(),
        None => {
            return StepResult {
                success: false,
                message: format!(
                    "Step '{}' is 'deterministic' but has no 'cmd' param. \
                     Set it via .with_param(\"cmd\", \"pageseeds ...\") in the handler's plan().",
                    step.name
                ),
                output: None,
            };
        }
    };

    let cmd = raw_cmd
        .replace("{project_path}", project_path)
        .replace("{automation_dir}", &automation_dir);

    log::info!("[executor] deterministic step '{}' cmd: {}", step.name, cmd);

    match std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .current_dir(project_path)
        .output()
    {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = if stderr.is_empty() {
                stdout.clone()
            } else {
                format!("{}\n[stderr]\n{}", stdout, stderr)
            };
            if out.status.success() {
                StepResult { success: true, message: format!("Step '{}' OK", step.name), output: Some(combined) }
            } else {
                StepResult {
                    success: false,
                    message: format!("Step '{}' failed (exit {}): {}", step.name, out.status, stderr.trim()),
                    output: Some(combined),
                }
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Step '{}' could not launch: {}", step.name, e),
            output: None,
        },
    }
}

/// Execute an agentic step — invokes the configured agent CLI with a built prompt.
///
/// Build order:
///   1. Load skill from step params ("skill" key) → SKILL.md text
///   2. Build a prompt via `prompts::build_prompt`
///   3. Call `agent::run_agent(provider, prompt, project_path)`
///   4. Return the raw output as the step result
pub fn exec_agentic(
    step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    site_url: &str,
    agent_provider: &str,
) -> StepResult {
    use crate::engine::{agent, prompts, skills};
    use std::path::Path;

    let repo_root = Path::new(project_path);

    // 1. Optionally load skill
    let skill = step
        .params
        .get("skill")
        .and_then(|name| skills::load_skill(repo_root, name));

    // 2. Build prompt
    let prompt = if let Some(ref s) = skill {
        prompts::build_prompt(task, s, project_path, Some(site_url)).prompt
    } else {
        // Fallback prompt when no skill is configured.
        // Include description so the agent knows exactly which file to edit and
        // what checks to fix — avoiding any need for shell-based file discovery.
        let desc_section = task.description
            .as_deref()
            .filter(|d| !d.is_empty())
            .map(|d| format!("\n\n## Task Details\n\n{}", d))
            .unwrap_or_default();
        format!(
            "## Task\n\n- ID: {}\n- Type: {}\n- Title: {}\n- Step: {}\n- Site: {}\n- Repo: {}{}\n\nExecute this task step and return the results.",
            task.id,
            task.task_type,
            task.title.as_deref().unwrap_or("(untitled)"),
            step.name,
            site_url,
            project_path,
            desc_section,
        )
    };

    log::info!(
        "[executor] agentic step '{}' with provider '{}' (skill: {:?})",
        step.name,
        agent_provider,
        step.params.get("skill")
    );

    // 3. Call agent
    match agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(output) => StepResult {
            success: true,
            message: format!("Agentic step '{}' complete ({} chars)", step.name, output.len()),
            output: Some(output),
        },
        Err(err) => {
            log::warn!("[executor] agentic step '{}' failed: {}", step.name, err);
            StepResult {
                success: false,
                message: format!("Agentic step '{}' failed: {}", step.name, err),
                output: None,
            }
        }
    }
}
