/// Trait-based workflow handlers — one per task family.
///
/// Each handler knows:
///   - which task types it owns (`supports`)
///   - what steps the task needs (`plan`)
///
/// Step execution happens in `executor.rs`; handlers only describe the plan.

use super::{step_params, StepResult, WorkflowStep};
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
                WorkflowStep::new("collect_gsc_inspect", "collect_gsc_inspect"),
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
                // Step 1 (deterministic): group reason_codes, count occurrences, pick example URLs.
                // Writes gsc_summary.json. Grouping and counting cannot require judgment.
                WorkflowStep::new("investigate_gsc_summarise", "gsc_summarise"),
                // Step 2 (agentic): interpret the grouped summary, identify patterns, recommend
                // corrective actions. Cannot be deterministic: interpreting *why* a cluster of
                // pages is not indexed and what to do about it requires intent-level judgment.
                WorkflowStep::new("investigate_gsc_agent", "gsc_investigate_agentic"),
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
        match task_type(task) {
            "research_keywords" => {
                // Deterministic mode when themes are explicitly provided on the task.
                let has_explicit_themes = task
                    .description
                    .as_deref()
                    .map(crate::engine::exec::keywords::parse_desc_themes)
                    .map(|themes| !themes.is_empty())
                    .unwrap_or(false);

                if has_explicit_themes {
                    vec![WorkflowStep::new("research_keywords_cli", "keyword_research_cli")]
                } else {
                    // Agentic mode for theme discovery from project context/brief,
                    // followed by deterministic keyword API execution.
                    vec![
                        WorkflowStep::new("research_theme_selection_agent", "agentic"),
                        WorkflowStep::new("research_keywords_cli", "keyword_research_cli"),
                    ]
                }
            }
            _ => {
                let mut steps = vec![
                    WorkflowStep::new("research_agent_stage", "agentic")
                        .with_param(step_params::SKILL, "seo-keyword-research")
                ];
                if task_type(task) == "custom_keyword_research" {
                    steps.push(
                        WorkflowStep::new("research_normalize_stage", "normalizer")
                            .with_param(step_params::NORMALIZER_ID, "keyword_research")
                            .with_param(step_params::ARTIFACT_NAME, "keyword_research"),
                    );
                }
                steps
            }
        }
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
        match task_type(task) {
            "content_cleanup" => vec![
                WorkflowStep::new("content_cleanup_run", "deterministic")
                    .with_param(step_params::CMD, "pageseeds content clean --workspace-dir {automation_dir}"),
            ],
            "publish_content" => vec![
                WorkflowStep::new("publish_content_run", "deterministic")
                    .with_param(step_params::CMD, "pageseeds content validate --workspace-dir {automation_dir}"),
            ],
            "cluster_and_link" => vec![
                // Step 1 (deterministic): scan all MDX files, build the full link map,
                // identify gaps. The scan itself is pure file I/O + regex — no judgment.
                WorkflowStep::new("cluster_and_link_scan", "deterministic")
                    .with_param(step_params::CMD, "pageseeds content scan-internal-links --workspace-dir {automation_dir}"),
                // Step 2 (agentic): interpret the scan output, pick pillar/cluster
                // structure, recommend specific links to add.
                // Cannot be deterministic: choosing which articles are pillars vs supports,
                // and which gaps matter most, requires understanding article intent and
                // business priorities — not just graph connectivity.
                WorkflowStep::new("cluster_and_link_strategy", "agentic"),
            ],
            // fix_* and other implementation types: agentic for now.
            //
            // TODO: each fix_* type that gets implemented should follow the hybrid pattern:
            //   Step 1 (deterministic): classify URLs/issues by pattern, generate
            //     mechanical rules (e.g. redirect rules for recognisable URL formats)
            //   Step 2 (agentic): handle ambiguous cases that don't fit a pattern
            //
            // An agentic step alone for fix_404s will ask the LLM to generate redirect
            // rules for patterns that a regex would handle reliably.
            _ => vec![WorkflowStep::new("implementation_agent_stage", "agentic")],
        }
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
            vec![
                // Step 1 (deterministic): API search + engagement/accessibility scoring.
                // Filter by age, exclusions, and score threshold. No judgment required.
                WorkflowStep::new("reddit_search_stage", "reddit_search"),
                // Step 2 (agentic): relevance scoring, pain point extraction, reply drafting.
                // Cannot be deterministic: deciding whether a post is relevant to *this*
                // product, extracting intent, and writing a contextually appropriate reply
                // all require understanding of project context and language.
                WorkflowStep::new("reddit_enrich_stage", "reddit_enrich"),
            ]
        } else {
            // Other reddit tasks (e.g. reply drafting) still use agent + optional normalizer.
            let mut steps = vec![WorkflowStep::new("reddit_agent_stage", "agentic")];
            steps.push(
                WorkflowStep::new("reddit_normalize_stage", "normalizer")
                    .with_param(step_params::NORMALIZER_ID, "reddit_opportunities")
                    .with_param(step_params::ARTIFACT_NAME, "reddit_opportunities")
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
        // TODO: not yet implemented. The correct design is:
        //   Step 1 (deterministic): fetch GSC analytics/movers data for the site
        //   Step 2 (agentic): interpret trends, surface ranking opportunities
        //
        // An empty agentic step with no data context is fake intelligence — the
        // agent receives no artifact and produces generic filler. Use manual until
        // the deterministic data-fetch step exists.
        vec![WorkflowStep::new("performance_manual", "manual")]
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

    let raw_cmd = match step.params.get(step_params::CMD) {
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
    use crate::engine::project_paths::ProjectPaths;
    use std::path::Path;

    let repo_root = Path::new(project_path);
    let paths = ProjectPaths::from_path(project_path);

    let is_content_task = matches!(
        task.task_type.as_str(),
        "write_article" | "optimize_article" | "create_content" | "optimize_content"
    );

    let content_context = if is_content_task {
        let resolved = crate::content::locator::resolve(repo_root, None);
        resolved
            .selected
            .as_ref()
            .map(|dir| (dir.clone(), snapshot_markdown_mtime(dir), detect_numbered_mdx_style(dir)))
    } else {
        None
    };

    // 1. Optionally load skill
    let skill = step
        .params
        .get("skill")
        .and_then(|name| skills::load_skill(repo_root, name));

    // 2. Optionally load step artifact content into prompt context.
    let artifact_context = if let Some(artifact_name) = step.params.get(step_params::ARTIFACT) {
        let artifact_path = paths.automation_dir.join(artifact_name);
        match std::fs::read_to_string(&artifact_path) {
            Ok(content) => format!(
                "\n\n## Artifact: {}\n\n```json\n{}\n```",
                artifact_name, content
            ),
            Err(_) => {
                return StepResult {
                    success: false,
                    message: format!("{} not found — run collect_gsc first", artifact_name),
                    output: None,
                };
            }
        }
    } else {
        String::new()
    };

    // 3. Build prompt
    let mut prompt = if let Some(ref s) = skill {
        let mut p = prompts::build_prompt(task, s, project_path, Some(site_url)).prompt;
        p.push_str(&artifact_context);
        p
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
        ) + &artifact_context
    };

        // Agentic theme selection step for research_keywords: produce only focused themes.
        if step.name == "research_theme_selection_agent" {
            let automation_dir = paths.automation_dir.to_string_lossy();
            prompt.push_str(&format!(
                "\n\n## Theme Selection Contract (Required)\n\
                 You are selecting keyword seed themes only. Do NOT run keyword research yet.\n\
                 Read project context files in this repo, especially:\n\
                 - {automation_dir}/seo_content_brief.md\n\
                 - {automation_dir}/project_summary.md (if present)\n\
                 \n\
                 Return ONLY one fenced JSON block and no extra prose:\n\
                 ```json\n\
                 {{\n\
                   \"themes\": [\"theme 1\", \"theme 2\", \"theme 3\"]\n\
                 }}\n\
                 ```\n\
                 Requirements:\n\
                 - Return 3 to 6 themes.\n\
                 - Prefer specific gap topics / missing intents from the brief.\n\
                 - Avoid generic umbrella terms (example: \"risk management\", \"advanced topics\").\n\
                 - Avoid job-seeker / unrelated enterprise/cybersecurity drift unless explicitly core to this project.\n\
                 - Keep each theme concise (2-6 words).\n\
                 - Do NOT include date, year, or month suffixes in themes (wrong: \"vix strategies 2026\", right: \"vix trading strategies\").\n\
                 - Prefer established, searchable topics that are NOT time-specific; these are used as Ahrefs seed queries.\n\
                 - Avoid highly niche or newly-coined phrases unlikely to have measurable search volume."
            ));
        }

        // Research parity: force a machine-readable output contract so the next UI
        // step (keyword selection) behaves like the CLI flow.
        if matches!(task.task_type.as_str(), "research_keywords" | "custom_keyword_research")
            && step.name != "research_theme_selection_agent"
        {
                prompt.push_str(
                        "\n\n## Output Contract (Required)\n\
                         Return ONLY one fenced JSON block and no extra prose.\n\
                         The JSON must use this schema:\n\
                         ```json\n\
                         {\n\
                             \"new_keywords\": [\"keyword 1\", \"keyword 2\"],\n\
                             \"filtered_out\": 0,\n\
                             \"difficulty\": {\n\
                                 \"results\": [\n\
                                     {\"keyword\": \"keyword 1\", \"difficulty\": 24, \"volume\": 1200},\n\
                                     {\"keyword\": \"keyword 2\", \"difficulty\": 31, \"volume\": 700}\n\
                                 ]\n\
                             }\n\
                         }\n\
                         ```\n\
                         Requirements:\n\
                         - Provide 10 keyword candidates when possible.\n\
                         - Include numeric difficulty and numeric volume when available.\n\
                         - Do not include tool transcripts, logs, or markdown tables outside the JSON block."
                );
        }

            if is_content_task {
                prompt.push_str(
                    "\n\n## Content File Format (Required)\n\
                     - New articles must be written as `.mdx` files (never `.md`).\n\
                     - If you propose a filename, it must end in `.mdx`.\n\
                     - Preserve valid frontmatter and markdown/MDX syntax."
                );

                if let Some((_dir, _before, Some(style))) = &content_context {
                    prompt.push_str(&format!(
                        "\n\n## Content Filename Convention (Required)\n\
                         - Follow this naming format: `{{id}}_topic_slug.mdx`\n\
                         - Use lowercase with underscores in the slug.\n\
                         - Continue numbering from approximately {}.",
                        style.next_id
                    ));
                }
            }

    log::info!(
        "[executor] agentic step '{}' with provider '{}' (skill: {:?})",
        step.name,
        agent_provider,
        step.params.get(step_params::SKILL)
    );

    // 4. Call agent
    match agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(output) => {
            let mut message = format!("Agentic step '{}' complete ({} chars)", step.name, output.len());

            if let Some((content_dir, before, style)) = content_context {
                let renamed = rename_new_or_modified_md_to_mdx(&content_dir, &before);
                if !renamed.is_empty() {
                    message.push_str(&format!(" · enforced MDX on {} file(s)", renamed.len()));
                    for (old, new) in &renamed {
                        log::info!("[content_mdx] renamed {} -> {}", old.display(), new.display());
                    }
                }

                if let Some(style) = style {
                    let renamed_style = rename_new_files_to_numbered_mdx(&content_dir, &before, style.next_id);
                    if !renamed_style.is_empty() {
                        message.push_str(&format!(" · normalized naming on {} file(s)", renamed_style.len()));
                        for (old, new) in &renamed_style {
                            log::info!("[content_name] renamed {} -> {}", old.display(), new.display());
                        }
                    }
                }
            }

            StepResult {
                success: true,
                message,
                output: Some(output),
            }
        }
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

#[derive(Debug, Clone, Copy)]
struct NumberedMdxStyle {
    next_id: i64,
}

fn detect_numbered_mdx_style(dir: &std::path::Path) -> Option<NumberedMdxStyle> {
    let mut count = 0i64;
    let mut max_id = 0i64;

    for path in crate::content::locator::collect_markdown_files(dir) {
        let is_mdx = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("mdx"))
            .unwrap_or(false);
        if !is_mdx {
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if let Some(id) = parse_numeric_prefix(name) {
            count += 1;
            if id > max_id {
                max_id = id;
            }
        }
    }

    // Only enforce when this style is clearly established in the repo.
    if count >= 5 {
        Some(NumberedMdxStyle { next_id: max_id + 1 })
    } else {
        None
    }
}

fn parse_numeric_prefix(filename: &str) -> Option<i64> {
    let prefix = filename.split_once('_')?.0;
    if prefix.chars().all(|c| c.is_ascii_digit()) {
        prefix.parse::<i64>().ok()
    } else {
        None
    }
}

fn normalize_slug_underscored(stem: &str) -> String {
    let mut out = String::new();
    let mut prev_sep = false;

    for ch in stem.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_sep = false;
        } else if !prev_sep {
            out.push('_');
            prev_sep = true;
        }
    }

    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "article".to_string()
    } else {
        trimmed
    }
}

fn rename_new_files_to_numbered_mdx(
    dir: &std::path::Path,
    before: &std::collections::HashMap<std::path::PathBuf, std::time::SystemTime>,
    start_id: i64,
) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    let mut renamed = Vec::new();
    let mut next_id = start_id;

    for path in crate::content::locator::collect_markdown_files(dir) {
        // Rename only newly created files from this run, not existing repo files.
        if before.contains_key(&path) {
            continue;
        }

        let is_mdx = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("mdx"))
            .unwrap_or(false);
        if !is_mdx {
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if parse_numeric_prefix(name).is_some() {
            continue;
        }

        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("article");
        let slug = normalize_slug_underscored(stem);

        let target = loop {
            let candidate = dir.join(format!("{}_{}.mdx", next_id, slug));
            if !candidate.exists() {
                break candidate;
            }
            next_id += 1;
        };

        if std::fs::rename(&path, &target).is_ok() {
            renamed.push((path, target));
            next_id += 1;
        }
    }

    renamed
}

fn snapshot_markdown_mtime(
    dir: &std::path::Path,
) -> std::collections::HashMap<std::path::PathBuf, std::time::SystemTime> {
    let mut out = std::collections::HashMap::new();
    for path in crate::content::locator::collect_markdown_files(dir) {
        if let Ok(meta) = std::fs::metadata(&path) {
            if let Ok(mtime) = meta.modified() {
                out.insert(path, mtime);
            }
        }
    }
    out
}

fn rename_new_or_modified_md_to_mdx(
    dir: &std::path::Path,
    before: &std::collections::HashMap<std::path::PathBuf, std::time::SystemTime>,
) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    let mut renamed = Vec::new();

    for path in crate::content::locator::collect_markdown_files(dir) {
        let is_md = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("md"))
            .unwrap_or(false);
        if !is_md {
            continue;
        }

        let modified = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok());

        let changed_since_before = match (before.get(&path), modified) {
            (None, Some(_)) => true,
            (Some(prev), Some(now)) => now > *prev,
            _ => false,
        };

        if !changed_since_before {
            continue;
        }

        let target = path.with_extension("mdx");
        if target.exists() {
            log::warn!(
                "[content_mdx] skipping rename {} -> {} because target exists",
                path.display(),
                target.display()
            );
            continue;
        }

        if std::fs::rename(&path, &target).is_ok() {
            renamed.push((path, target));
        }
    }

    renamed
}
