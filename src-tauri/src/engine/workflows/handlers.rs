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
            "fix_content_article" => vec![
                // Per-article content fix: reads the recommendations artifact embedded in the task
                // and applies SEO improvements (title, meta, intro, internal links, FAQ, EEAT, CTA)
                // to a single MDX file. One focused agent call per article.
                WorkflowStep::new("fix_content_article_apply", "agentic"),
            ],
            "cluster_and_link" => vec![
                // Step 1 (deterministic, native Rust): scan all MDX files, build the full link
                // map, identify orphans and coverage gaps.  Pure file I/O + regex — no judgment.
                // Writes link_scan.json to the automation dir for the next step to consume.
                WorkflowStep::new("cluster_and_link_scan", "cluster_link_scan"),
                // Step 2 (agentic): interpret the scan output, determine pillar/cluster
                // structure, and recommend specific missing links to add.
                // Cannot be deterministic: deciding which articles are pillars vs supports,
                // and which gaps matter most, requires understanding article intent and
                // business priorities — not just graph connectivity.
                // Writes links_to_add.json (output contract: {links_to_add:[{source_article_id,
                // source_file, target_article_id, target_title, target_slug, reason}]}).
                WorkflowStep::new("cluster_and_link_strategy", "cluster_link_strategy"),
                // Step 3 (deterministic): read links_to_add.json and append "Related Articles"
                // sections to the MDX files that are missing them.
                // Skips files that already have a Related Articles section or already link
                // to the target slug.
                WorkflowStep::new("cluster_and_link_apply", "cluster_link_apply"),
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
        match task_type(task) {
            "reddit_opportunity_search" => vec![
                // Step 1 (agentic): Parse reddit_config.md and extract structured search parameters.
                WorkflowStep::new("reddit_config_parse_stage", "reddit_config_parse"),
                // Step 2 (deterministic): API search using the structured parameters.
                WorkflowStep::new("reddit_search_stage", "reddit_search"),
                // Step 3 (agentic): relevance scoring, pain point extraction, reply drafting.
                WorkflowStep::new("reddit_enrich_stage", "reddit_enrich"),
                // Step 4 (deterministic): Fetch enriched opportunities from DB.
                WorkflowStep::new("reddit_results_stage", "reddit_fetch_results"),
            ],
            "reddit_reply" => vec![
                // Step 1 (deterministic): Post the reply to Reddit via API.
                // Extracts post_id and reply_text from task description.
                WorkflowStep::new("reddit_post_reply", "reddit_post_reply"),
            ],
            _ => {
                // Other reddit tasks use agent + optional normalizer.
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
}

// ─── Keyword Coverage ─────────────────────────────────────────────────────────

pub struct CoverageHandler;

impl WorkflowHandler for CoverageHandler {
    fn supports(&self, task: &Task) -> bool {
        task_type(task) == "analyze_keyword_coverage"
    }

    fn plan(&self, _task: &Task) -> Vec<WorkflowStep> {
        vec![
            // Step 1 (deterministic): Load articles from articles.json
            WorkflowStep::new("coverage_load_articles", "coverage_load_articles"),
            // Step 2 (agentic): Cluster articles by semantic similarity
            // Cannot be deterministic: understanding topic relationships and naming
            // clusters requires semantic judgment about content themes.
            WorkflowStep::new("coverage_cluster_analysis", "coverage_cluster_analysis"),
            // Step 3 (deterministic): Save results to keyword_coverage.json
            WorkflowStep::new("coverage_save", "coverage_save"),
        ]
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

// ─── Social Media Marketing ───────────────────────────────────────────────────

pub struct SocialHandler;

impl WorkflowHandler for SocialHandler {
    fn supports(&self, task: &Task) -> bool {
        task_type(task).starts_with("social_")
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task_type(task) {
            "social_generate_campaign" => vec![
                WorkflowStep::new("social_collect_sources", "social_collect_sources"),
                WorkflowStep::new("social_load_templates", "social_load_templates"),
                WorkflowStep::new("social_generate_posts", "social_generate_posts"),
                WorkflowStep::new("social_build_visuals", "social_build_visuals"),
                WorkflowStep::new("social_save_campaign", "social_save_campaign"),
            ],
            "social_generate_from_article" => vec![
                WorkflowStep::new("social_extract_article", "social_extract_article"),
                WorkflowStep::new("social_generate_posts", "social_generate_posts"),
                WorkflowStep::new("social_build_visuals", "social_build_visuals"),
                WorkflowStep::new("social_save_campaign", "social_save_campaign"),
            ],
            "social_regenerate_post" => vec![
                WorkflowStep::new("social_regenerate_single", "social_regenerate_single"),
                WorkflowStep::new("social_rebuild_visual", "social_rebuild_visual"),
                WorkflowStep::new("social_update_post", "social_update_post"),
            ],
            "social_create_template" => vec![
                WorkflowStep::new("social_design_template", "social_design_template"),
                WorkflowStep::new("social_save_template", "social_save_template"),
            ],
            _ => vec![WorkflowStep::new(&format!("{}_manual", task_type(task)), "manual")],
        }
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
        Box::new(SocialHandler),
        Box::new(PerformanceHandler),
        Box::new(CoverageHandler),
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
pub async fn exec_deterministic(step: &WorkflowStep, _task: &Task, project_path: &str) -> StepResult {
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

    // Run blocking subprocess in spawn_blocking
    let step_name = step.name.clone();
    let project_path = project_path.to_string();
    match tokio::task::spawn_blocking(move || {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(&project_path)
            .output()
    }).await {
        Ok(Ok(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = if stderr.is_empty() {
                stdout.clone()
            } else {
                format!("{}\n[stderr]\n{}", stdout, stderr)
            };
            if out.status.success() {
                StepResult { success: true, message: format!("Step '{}' OK", step_name), output: Some(combined) }
            } else {
                StepResult {
                    success: false,
                    message: format!("Step '{}' failed (exit {}): {}", step_name, out.status, stderr.trim()),
                    output: Some(combined),
                }
            }
        }
        Ok(Err(e)) => StepResult {
            success: false,
            message: format!("Step '{}' could not launch: {}", step_name, e),
            output: None,
        },
        Err(e) => StepResult {
            success: false,
            message: format!("Step '{}' task failed: {}", step.name, e),
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
pub async fn exec_agentic(
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
            
            // Load keyword coverage if it exists to inform gap-filling
            let coverage_context = load_coverage_context(&paths.automation_dir);
            
            prompt.push_str(&format!(
                "\n\n## Theme Selection Contract (Required)\n\
                 You are selecting keyword SEED QUERIES for the Ahrefs free keyword ideas API.\n\
                 Read project context files in this repo, especially:\n\
                 - {automation_dir}/seo_content_brief.md\n\
                 - {automation_dir}/project_summary.md (if present)\n\
                 \n\
                 {coverage_context}\n\
                 \n\
                 Return ONLY one fenced JSON block and no extra prose:\n\
                 ```json\n\
                 {{\n\
                   \"themes\": [\"theme 1\", \"theme 2\", \"theme 3\"]\n\
                 }}\n\
                 ```\n\
                 \n\
                 ## CRITICAL: Themes are SEED QUERIES, not target keywords\n\
                 The Ahrefs ideas API expands short seed terms into dozens of related keywords.\n\
                 Long, specific phrases (4+ words) return ZERO ideas — they are too narrow to expand.\n\
                 \n\
                 GOOD seeds (1-3 words, broad enough for Ahrefs to expand):\n\
                 - \"budget planner\", \"expense tracker\", \"savings tracker\"\n\
                 - \"options trading\", \"wheel strategy\", \"covered calls\"\n\
                 - \"content marketing\", \"keyword research\", \"internal linking\"\n\
                 \n\
                 BAD seeds (too long/specific, Ahrefs returns zero ideas):\n\
                 - \"monthly cash flow planner spreadsheet\" → use \"cash flow planner\" instead\n\
                 - \"variable income budget planner template\" → use \"budget planner\" instead\n\
                 - \"small business expense categories tax\" → use \"business expenses\" instead\n\
                 - \"savings goals tracker Google Sheets\" → use \"savings tracker\" instead\n\
                 \n\
                 Requirements:\n\
                 - Return 4 to 6 themes.\n\
                 - Each theme MUST be 1-3 words maximum.\n\
                 - Derive topics from the content brief gaps and pillars.\n\
                 - Pick topics that have real search volume (established, not newly-coined).\n\
                 - Do NOT include brand names, tool names (Google Sheets, Excel), or year/date suffixes.\n\
                 - Do NOT include job-seeker / enterprise/cybersecurity terms unless core to this project."
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
                // Pre-compute the next safe publish date and inject it into the prompt.
                // Without this, the agent defaults to today's date which conflicts with
                // articles already in articles.json and breaks the date distribution.
                // Cannot be deterministic-only: the date depends on the current state of
                // articles.json and must be computed from the existing occupied slots.
                if let Some(date) = compute_next_publish_date(project_path) {
                    prompt.push_str(&format!(
                        "\n\n## Publish Date (Required)\n\
                         - The frontmatter `date:` field MUST be exactly: `{date}`\n\
                         - Do not use today's date or any other value — use the date above."
                    ));
                }

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

    log::info!(
        "[executor] agentic step '{}' with provider '{}' (skill: {:?})",
        step.name,
        agent_provider,
        step.params.get(step_params::SKILL)
    );

    // 4. Call agent (blocking subprocess, run in spawn_blocking)
    let agent_provider = agent_provider.to_string();
    let prompt = prompt.clone();
    let repo_root = repo_root.to_path_buf();
    let step_name = step.name.clone();
    
    match tokio::task::spawn_blocking(move || {
        agent::run_agent(&agent_provider, &prompt, &repo_root)
    }).await {
        Ok(Ok(output)) => {
            let mut message = format!("Agentic step '{}' complete ({} chars)", step_name, output.len());

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
        Ok(Err(err)) => {
            log::warn!("[executor] agentic step '{}' failed: {}", step_name, err);
            StepResult {
                success: false,
                message: format!("Agentic step '{}' failed: {}", step_name, err),
                output: None,
            }
        }
        Err(e) => {
            log::warn!("[executor] agentic step '{}' task failed: {}", step_name, e);
            StepResult {
                success: false,
                message: format!("Agentic step '{}' task failed: {}", step_name, e),
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

/// Load keyword coverage from the automation directory and format it for the agent prompt.
///
/// Returns a formatted string describing current topic clusters, or an empty string
/// if no coverage analysis exists.
fn load_coverage_context(automation_dir: &std::path::Path) -> String {
    let coverage_path = automation_dir.join("keyword_coverage.json");
    
    let content = match std::fs::read_to_string(&coverage_path) {
        Ok(c) => c,
        Err(_) => return String::new(), // No coverage file yet
    };
    
    let coverage: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    
    let clusters = coverage.get("clusters").and_then(|c| c.as_array());
    let article_count = coverage.get("article_count").and_then(|a| a.as_i64()).unwrap_or(0);
    
    if clusters.is_none() || clusters.unwrap().is_empty() {
        return String::new();
    }
    
    let clusters = clusters.unwrap();
    let cluster_summaries: Vec<String> = clusters
        .iter()
        .filter_map(|c| {
            let name = c.get("cluster_name").and_then(|n| n.as_str())?;
            let keywords = c.get("primary_keywords").and_then(|k| k.as_array())?;
            let count = c.get("article_count").and_then(|n| n.as_i64()).unwrap_or(0);
            
            let keyword_list: Vec<&str> = keywords
                .iter()
                .filter_map(|k| k.as_str())
                .collect();
            
            Some(format!(
                "- {} ({} articles): focus on {}",
                name,
                count,
                keyword_list.join(", ")
            ))
        })
        .collect();
    
    if cluster_summaries.is_empty() {
        return String::new();
    }
    
    format!(
        "## Current Keyword Coverage (from previous analysis)\n\
         This project has {} articles organized into {} topic clusters:\n\
         {}\n\
         \n\
         ## Gap-Filling Guidance\n\
         When selecting themes, consider:\n\
         1. Which clusters are under-represented and need more content?\n\
         2. What related topics are NOT yet covered by existing clusters?\n\
         3. What sub-topics within large clusters could use dedicated articles?\n\
         Prioritize themes that fill gaps or expand thinly-covered areas.",
        article_count,
        clusters.len(),
        cluster_summaries.join("\n")
    )
}

/// Read articles.json from `{project_path}/.github/automation/articles.json` and return
/// the next unoccupied past date for a new article.
///
/// Implements the same logic as `content::date_policy::suggest_next_safe_date` but reads
/// dates directly from the on-disk JSON file instead of requiring a DB connection, so it
/// can be called from inside `exec_agentic` which has no access to SQLite.
pub(crate) fn compute_next_publish_date(project_path: &str) -> Option<String> {
    use chrono::{Duration, NaiveDate, Utc};
    use std::collections::HashSet;

    let articles_path = std::path::Path::new(project_path)
        .join(".github")
        .join("automation")
        .join("articles.json");

    let json = std::fs::read_to_string(&articles_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&json).ok()?;
    let articles = value.get("articles")?.as_array()?;

    let occupied: HashSet<NaiveDate> = articles
        .iter()
        .filter_map(|a| a["published_date"].as_str())
        .filter(|d| !d.is_empty())
        .filter_map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect();

    let today = Utc::now().date_naive();
    let mut cursor = today - Duration::days(1);
    while occupied.contains(&cursor) {
        cursor -= Duration::days(1);
    }
    Some(cursor.format("%Y-%m-%d").to_string())
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
