/// Trait-based workflow handlers — one per task family.
///
/// Each handler knows:
///   - which task types it owns (`supports`)
///   - what steps the task needs (`plan`)
///
/// Step execution happens in `executor.rs`; handlers only describe the plan.

use super::{step_params, StepKind, StepResult, WorkflowStep};
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
                WorkflowStep::new("collect_gsc_inspect", StepKind::CollectGscInspect.as_ref()),
            ],
            // collect_posthog has no CLI implementation yet — fall back to agent.
            _ => vec![WorkflowStep::new("collect_agent_stage", StepKind::Agentic.as_ref())],
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
                WorkflowStep::new("investigate_gsc_summarise", StepKind::GscSummarise.as_ref()),
                // Step 2 (agentic): interpret the grouped summary, identify patterns, recommend
                // corrective actions. Cannot be deterministic: interpreting *why* a cluster of
                // pages is not indexed and what to do about it requires intent-level judgment.
                WorkflowStep::new("investigate_gsc_agent", StepKind::GscInvestigateAgentic.as_ref()),
            ],
            // investigate_posthog has no CLI implementation yet — fall back to agent.
            _ => vec![WorkflowStep::new("investigate_agent_stage", StepKind::Agentic.as_ref())],
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
            "research_keywords" | "research_landing_pages" => {
                // 6-step workflow:
                // agentic → deterministic → agentic → deterministic → deterministic → normalizer
                vec![
                    // Step 1 (agentic): LLM extracts 3-4 themes from project brief.
                    // Cannot be deterministic: requires reading intent from free-form text.
                    WorkflowStep::new("research_seed_extraction", StepKind::Agentic.as_ref()),

                    // Step 2 (deterministic): fetch Google Autocomplete for all themes.
                    // Free API, always returns results. Outputs structured JSON: [{theme, suggestions}].
                    WorkflowStep::new("research_autocomplete", StepKind::ResearchAutocomplete.as_ref()),

                    // Step 3 (agentic): LLM filters autocomplete suggestions for domain relevance.
                    // Cannot be deterministic: requires understanding what is on-topic for this
                    // specific product/site. Hard-coding a relevance rule would produce silent errors
                    // on any input it was not tested against.
                    // Input contract: [{theme, suggestions: [string]}]
                    // Output contract: {validated_seeds: [{theme: string, seeds: [string]}]}
                    WorkflowStep::new("research_seed_validation", StepKind::Agentic.as_ref()),

                    // Step 4 (deterministic): DataForSEO related_keywords per validated seed.
                    // Deterministic: given validated seeds, fetches keyword ideas + KD + volume.
                    WorkflowStep::new("research_ahrefs_pipeline", StepKind::KeywordResearchNative.as_ref()),

                    // Step 5 (deterministic): Select best candidates from structured data.
                    WorkflowStep::new("research_final_selection", StepKind::ResearchFinalSelection.as_ref()),

                    // Step 6 (normalizer): Enforces output contract before UI parses it.
                    WorkflowStep::new("research_normalize", StepKind::Normalizer.as_ref())
                        .with_param(step_params::NORMALIZER_ID, "keyword_research")
                        .with_param(step_params::ARTIFACT_NAME, "keyword_research"),
                ]
            }
            _ => {
                // TODO: migrate custom_keyword_research to 3-step agentic workflow
                // This legacy path uses the old agentic+normalizer flow via seo-keyword-research skill.
                // Kept for backward compatibility - migrate to the 3-step hybrid flow when ready.
                let mut steps = vec![
                    WorkflowStep::new("research_agent_stage", StepKind::Agentic.as_ref())
                        .with_param(step_params::SKILL, "seo-keyword-research")
                ];
                if task_type(task) == "custom_keyword_research" {
                    steps.push(
                        WorkflowStep::new("research_normalize_stage", StepKind::Normalizer.as_ref())
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
            return vec![WorkflowStep::new("content_review_apply_execute", StepKind::ContentReviewApplyExecute.as_ref())];
        }
        // Agentic: the agent reads the article spec and writes the MDX file.
        vec![WorkflowStep::new("content_write_stage", StepKind::Agentic.as_ref())]
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
            WorkflowStep::new("content_review_gsc_sync", StepKind::GscSyncArticles.as_ref())
                .optional(),
            // Step 2: deterministic multi-check audit → writes content_audit.json.
            // Optional — still valuable even without GSC data.
            WorkflowStep::new("content_review_audit", StepKind::ContentAudit.as_ref())
                .optional(),
            // Step 3: native sync — validates articles.json ↔ content files, dates.
            WorkflowStep::new("content_review_sync", StepKind::ContentSync.as_ref())
                .optional(),
            // Step 4: select priority articles, build structured context, get agent recommendations.
            // One focused agent call (not N calls). Writes recommendations.json.
            WorkflowStep::new("content_review_recommend", StepKind::ContentReviewRecommend.as_ref()),
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
                | "technical_seo"
                | "landing_page_spec"
                | "create_landing_page"
        ) || t.starts_with("fix_")
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task_type(task) {
            "content_cleanup" => vec![
                WorkflowStep::new("content_cleanup_run", StepKind::Deterministic.as_ref())
                    .with_param(step_params::CMD, "pageseeds content clean --workspace-dir {automation_dir}"),
            ],
            "publish_content" => vec![
                WorkflowStep::new("publish_content_run", StepKind::Deterministic.as_ref())
                    .with_param(step_params::CMD, "pageseeds content validate --workspace-dir {automation_dir}"),
            ],
            "fix_content_article" => vec![
                // Per-article content fix: reads the recommendations artifact embedded in the task
                // and applies SEO improvements (title, meta, intro, internal links, FAQ, EEAT, CTA)
                // to a single MDX file. One focused agent call per article.
                WorkflowStep::new("fix_content_article_apply", StepKind::Agentic.as_ref()),
            ],
            "indexing_diagnostics" => vec![
                // Stateful GSC indexing diagnostics: native Rust, tracks per-URL history in SQLite,
                // only re-checks stale or known-bad URLs, and spawns fix tasks for new/regressed
                // or unresolved issues. Deterministic because it is pure API calls + DB comparison.
                WorkflowStep::new("indexing_diagnostics_run", StepKind::IndexingDiagnosticsRun.as_ref()),
            ],
            "fix_indexing" | "fix_technical" => vec![
                // Step 1 (deterministic): load the target MDX file and extract structured context
                // (word count, H1, title, internal links, canonical). This is obvious file I/O —
                // no judgment required — and saves the agent from hunting around the repo.
                WorkflowStep::new("indexing_fix_context", StepKind::IndexingFixContext.as_ref()),
                // Step 2 (agentic): apply the fix. The agent gets the GSC issue + structured
                // context and edits the MDX file directly. Judgment is required because the fix
                // depends on intent, content quality, and site-specific conventions.
                WorkflowStep::new("indexing_fix_apply", StepKind::IndexingFixApply.as_ref()),
            ],
            "cluster_and_link" => vec![
                // Step 1 (deterministic, native Rust): scan all MDX files, build the full link
                // map, identify orphans and coverage gaps.  Pure file I/O + regex — no judgment.
                // Writes link_scan.json to the automation dir for the next step to consume.
                WorkflowStep::new("cluster_and_link_scan", StepKind::ClusterLinkScan.as_ref()),
                // Step 2 (agentic): interpret the scan output, determine pillar/cluster
                // structure, and recommend specific missing links to add.
                // Cannot be deterministic: deciding which articles are pillars vs supports,
                // and which gaps matter most, requires understanding article intent and
                // business priorities — not just graph connectivity.
                // Writes links_to_add.json (output contract: {links_to_add:[{source_article_id,
                // source_file, target_article_id, target_title, target_slug, reason}]}).
                WorkflowStep::new("cluster_and_link_strategy", StepKind::ClusterLinkStrategy.as_ref()),
                // Step 3 (deterministic): read links_to_add.json and append "Related Articles"
                // sections to the MDX files that are missing them.
                // Skips files that already have a Related Articles section or already link
                // to the target slug.
                WorkflowStep::new("cluster_and_link_apply", StepKind::ClusterLinkApply.as_ref()),
            ],
            "create_landing_page" | "landing_page_spec" => vec![
                // Deterministic: build a structured spec file from keyword metadata
                // already on the task. No LLM needed — the spec is a structured template
                // populated with keyword, page type, intent, volume, and KD.
                WorkflowStep::new("landing_page_spec_write", StepKind::LandingPageSpecWrite.as_ref()),
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
            _ => vec![WorkflowStep::new("implementation_agent_stage", StepKind::Agentic.as_ref())],
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
                WorkflowStep::new("reddit_config_parse_stage", StepKind::RedditConfigParse.as_ref()),
                // Step 2 (deterministic): API search using the structured parameters.
                WorkflowStep::new("reddit_search_stage", StepKind::RedditSearch.as_ref()),
                // Step 3 (agentic): relevance scoring, pain point extraction, reply drafting.
                WorkflowStep::new("reddit_enrich_stage", StepKind::RedditEnrich.as_ref()),
                // Step 4 (deterministic): Fetch enriched opportunities from DB.
                WorkflowStep::new("reddit_results_stage", StepKind::RedditFetchResults.as_ref()),
            ],
            "reddit_reply" => vec![
                // Step 1 (deterministic): Post the reply to Reddit via API.
                // Extracts post_id and reply_text from task description.
                WorkflowStep::new("reddit_post_reply", StepKind::RedditPostReply.as_ref()),
            ],
            _ => {
                // Other reddit tasks use agent + optional normalizer.
                let mut steps = vec![WorkflowStep::new("reddit_agent_stage", StepKind::Agentic.as_ref())];
                steps.push(
                    WorkflowStep::new("reddit_normalize_stage", StepKind::Normalizer.as_ref())
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
            WorkflowStep::new("coverage_load_articles", StepKind::CoverageLoadArticles.as_ref()),
            // Step 2 (agentic): Cluster articles by semantic similarity
            // Cannot be deterministic: understanding topic relationships and naming
            // clusters requires semantic judgment about content themes.
            WorkflowStep::new("coverage_cluster_analysis", StepKind::CoverageClusterAnalysis.as_ref()),
            // Step 3 (deterministic): Save results to keyword_coverage.json
            WorkflowStep::new("coverage_save", StepKind::CoverageSave.as_ref()),
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
        vec![WorkflowStep::new("performance_manual", StepKind::Manual.as_ref())]
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
                WorkflowStep::new("social_collect_sources", StepKind::SocialCollectSources.as_ref()),
                WorkflowStep::new("social_load_templates", StepKind::SocialLoadTemplates.as_ref()),
                WorkflowStep::new("social_generate_posts", StepKind::SocialGeneratePosts.as_ref()),
                WorkflowStep::new("social_build_visuals", StepKind::SocialBuildVisuals.as_ref()),
                WorkflowStep::new("social_save_campaign", StepKind::SocialSaveCampaign.as_ref()),
            ],
            "social_generate_from_article" => vec![
                WorkflowStep::new("social_extract_article", StepKind::SocialExtractArticle.as_ref()),
                WorkflowStep::new("social_generate_posts", "social_generate_posts"),
                WorkflowStep::new("social_build_visuals", "social_build_visuals"),
                WorkflowStep::new("social_save_campaign", "social_save_campaign"),
            ],
            "social_regenerate_post" => vec![
                WorkflowStep::new("social_regenerate_single", StepKind::SocialRegenerateSingle.as_ref()),
                WorkflowStep::new("social_rebuild_visual", StepKind::SocialRebuildVisual.as_ref()),
                WorkflowStep::new("social_update_post", StepKind::SocialUpdatePost.as_ref()),
            ],
            "social_create_template" => vec![
                WorkflowStep::new("social_design_template", StepKind::SocialDesignTemplate.as_ref()),
                WorkflowStep::new("social_save_template", StepKind::SocialSaveTemplate.as_ref()),
            ],
            _ => vec![WorkflowStep::new(&format!("{}_manual", task_type(task)), StepKind::Manual.as_ref())],
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
        vec![WorkflowStep::new(&format!("{}_manual", task_type(task)), StepKind::Manual.as_ref())]
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
pub async fn exec_deterministic(step: &WorkflowStep, _task: &Task, project_path: &str, _seo_provider: &str) -> StepResult {
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
    latest_raw_output: Option<&str>,
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

    // Check if this is a research workflow step that uses ToolCallingAgent
    // Note: research_final_selection is now deterministic, not agentic
    let is_research_step = matches!(
        step.name.as_str(),
        "research_seed_extraction" | "research_keyword_discovery" | "research_seed_validation"
    );

    if is_research_step {
        // Research steps use the same CLI agent path as all other agentic steps
        return crate::engine::exec::research::exec_research_workflow_step(
            step, task, project_path, agent_provider, latest_raw_output
        ).await;
    }

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


#[cfg(test)]
mod registry_tests {
    use super::*;
    use crate::config::TASK_TYPES;
    use crate::models::task::{AgentPolicy, ExecutionMode, Priority, Task, TaskRun, TaskStatus};

    fn make_task(task_type: &str) -> Task {
        Task {
            id: format!("test-{task_type}"),
            task_type: task_type.to_string(),
            phase: "research".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            execution_mode: ExecutionMode::Manual,
            agent_policy: AgentPolicy::Optional,
            title: Some(format!("{task_type} test")),
            description: None,
            project_id: "proj1".to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun { attempts: 0, last_error: None, provider: None },
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Every task type in config::TASK_TYPES must match a non-fallback handler.
    /// This catches silent registration failures where a new task type is added
    /// to the config but not to the handler registry.
    #[test]
    #[cfg(debug_assertions)]
    fn all_task_types_have_non_fallback_handler() {
        let handlers = default_handlers();
        // The last handler is always ManualFallbackHandler
        let non_fallback_handlers: Vec<&Box<dyn WorkflowHandler>> = handlers
            .iter()
            .filter(|h| {
                // ManualFallbackHandler matches everything, so we skip it
                !h.supports(&make_task("__manual_fallback_probe__"))
            })
            .collect();

        for task_type in TASK_TYPES {
            let task = make_task(task_type);
            let matched = handlers.iter().find(|h| h.supports(&task));
            assert!(
                matched.is_some(),
                "Task type '{}' has no handler at all",
                task_type
            );
            // Ensure it's not the fallback handler
            let steps = matched.unwrap().plan(&task);
            let is_fallback = steps.len() == 1 && steps[0].kind == StepKind::Manual;
            assert!(
                !is_fallback,
                "Task type '{}' falls through to ManualFallbackHandler. Add a real handler.",
                task_type
            );
        }

        log::info!(
            "[registry_test] All {} task types have non-fallback handlers",
            TASK_TYPES.len()
        );
    }
}
