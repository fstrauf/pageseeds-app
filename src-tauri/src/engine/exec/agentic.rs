//! Step execution helpers used by the executor for agentic steps.
//!
//! Stage C of issue #4 replaced `exec_agentic`'s boolean lattice with
//! per-step [`PromptSection`] declarations: handlers attach the prompt
//! sections a step needs to its `WorkflowStep`, and `exec_agentic` is a
//! generic assembler that iterates the declared sections. The section
//! builders below (`content_directives`, `hub_directives`) own the actual
//! section text.

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::{step_params, PromptSection, StepResult, WorkflowStep};
use crate::models::task::Task;

// Helpers relocated to `content/naming.rs` (Stage A.1).
use crate::content::naming::{
    detect_numbered_mdx_style, rename_new_files_to_numbered_mdx, rename_new_or_modified_md_to_mdx,
    snapshot_markdown_mtime,
};

// ─── Task-type classification helpers ─────────────────────────────────────────

fn is_content_task(task: &Task) -> bool {
    matches!(
        task.task_type.as_str(),
        "write_article"
            | "optimize_article"
            | "create_content"
            | "optimize_content"
            | "create_hub_page"
            | "refresh_hub_page"
            | "create_landing_page"
            | "fix_content_article"
    )
}

/// Derive the filename stem for a new-article task: the target keyword from the
/// task description when present, otherwise the task title with known
/// imperative prefixes stripped.
fn task_topic_stem(task: &Task) -> String {
    if let Some(keyword) = crate::engine::post_actions::content_task_target_keyword(task) {
        return keyword;
    }
    task.title
        .as_deref()
        .map(crate::engine::post_actions::strip_content_task_title_prefix)
        .filter(|t| !t.is_empty())
        .unwrap_or("article")
        .to_string()
}

fn is_research_step(step: &WorkflowStep) -> bool {
    matches!(
        step.name.as_str(),
        "research_seed_extraction" | "research_keyword_discovery" | "research_seed_validation"
    )
}

// ─── Content-task prompt sections ─────────────────────────────────────────────

/// Snapshot of the resolved content directory taken before the agent runs:
/// the directory, per-file mtimes, and the detected numbered-MDX style.
type ContentDirSnapshot = (
    std::path::PathBuf,
    std::collections::HashMap<std::path::PathBuf, std::time::SystemTime>,
    Option<crate::content::naming::NumberedMdxStyle>,
);

/// Build the publish-date, file-format, link-format, and filename-convention
/// directive sections for content tasks. Returns `None` for non-content tasks.
///
/// `new_article` is the flag declared on the step's
/// `PromptSection::ContentDirectives { new_article }` by the handler; it
/// selects the "Publish Date (Required)" variant over "(Preserve)". The
/// task-type list behind it lives exactly once, in `ContentHandler::plan`.
///
/// `target` is the deterministic target path for new-article tasks plus whether
/// the configured provider can write files itself. When present, the directive
/// names the exact path instead of the approximate numbering hint.
fn content_directives(
    task: &Task,
    new_article: bool,
    content_context: &Option<ContentDirSnapshot>,
    next_publish_date: &Option<String>,
    target: Option<(&std::path::Path, bool)>,
) -> Option<String> {
    if !is_content_task(task) {
        return None;
    }

    let mut sections = String::new();

    if let Some(date) = next_publish_date {
        if new_article {
            sections.push_str(&format!(
                "\n\n## Publish Date (Required)\n\
                 - The frontmatter `date:` field MUST be exactly: `{date}`\n\
                 - Do not use today's date or any other value — use the date above."
            ));
        } else {
            sections.push_str(&format!(
                "\n\n## Publish Date (Preserve)\n\
                 - Preserve the existing `date:` field in the frontmatter.\n\
                 - Do NOT change it to today's date, a future date, or a date already used by another article.\n\
                 - If the article has no date, use exactly: `{date}`"
            ));
        }
    }

    sections.push_str(
        "\n\n## Content File Format (Required)\n\
         - New articles must be written as `.mdx` files (never `.md`).\n\
         - If you propose a filename, it must end in `.mdx`.\n\
         - Preserve valid frontmatter and markdown/MDX syntax.",
    );

    sections.push_str(
        "\n\n## Internal Link Format (Required)\n\
         - All internal links MUST use standard markdown syntax: `[anchor text](/blog/slug)`\n\
         - The URL path must be wrapped in parentheses `()` immediately after the closing bracket `]`.\n\
         - WRONG: `[anchor text]/blog/slug` or `[anchor text] /blog/slug`\n\
         - CORRECT: `[anchor text](/blog/slug)`\n\
         - If you include a 'Related Articles' section, use the same `[title](/blog/slug)` format for every bullet.",
    );

    match target {
        Some((path, true)) => {
            sections.push_str(&format!(
                "\n\n## Target File (Required)\n\
                 - Write the article to EXACTLY this path: `{}`\n\
                 - Keep the numeric prefix and underscored slug exactly as given.\n\
                 - Do not invent a different filename, directory, or extension.",
                path.display()
            ));
        }
        Some((path, false)) => {
            sections.push_str(&format!(
                "\n\n## Target File (Required)\n\
                 - The target path for this article is: `{}`\n\
                 - You cannot write files. Return ONLY the complete MDX content \
                 (frontmatter + body) — the caller persists it to that exact path.\n\
                 - No explanations, commentary, or code fences outside the MDX document.",
                path.display()
            ));
        }
        None => {
            if let Some((_, _, Some(style))) = content_context {
                sections.push_str(&format!(
                    "\n\n## Content Filename Convention (Required)\n\
                     - Follow this naming format: `{{id}}_topic_slug.mdx`\n\
                     - Use lowercase with underscores in the slug.\n\
                     - Continue numbering from approximately {}.",
                    style.next_id
                ));
            }
        }
    }

    Some(sections)
}

// ─── Hub-task prompt sections ─────────────────────────────────────────────────

const HUB_PAGE_REQUIREMENTS: &str = "\n\n## Hub Page Requirements\n\
     - Write a comprehensive pillar / hub page MDX document.\n\
     - YAML frontmatter MUST include `type: hub` and `hub_topic`.\n\
     - Include an H1 matching the hub title.\n\
     - Link to every spoke article using `/blog/{slug}` format.\n\
     - Total word count MUST be 1500+ words.\n\
     - The frontmatter `title:` and the body H1 must be complete, grammatically correct phrases. They must NOT end mid-sentence or with dangling words such as `a`, `an`, `the`, `and`, `or`, `to`, `for`, `of`, `in`, `on`, `with`, `by`, `from`, `as`, `is`, `are`, `what`, `how`, `when`, `where`, `why`, `which`, `complete`, `guide`, `income`, `without`, `track`, `close`, `compared`, or trailing punctuation (`:`, `,`, `-`). Rewrite rather than truncate.\n\
     - The first body paragraph must begin with a complete sentence; do not drop leading characters from the opening sentence.\n\
     - Return ONLY the complete MDX content. No explanations outside the MDX.\n";

/// Build the hub-specific directive sections (spoke context + requirements).
/// Returns `None` for non-hub tasks.
fn hub_directives(task: &Task, project_path: &str) -> Option<String> {
    let mut sections = String::new();
    sections.push_str(&hub_spoke_context(task, project_path));
    sections.push_str(HUB_PAGE_REQUIREMENTS);
    Some(sections)
}

/// Gather the hub topic, suggested title/URL, and spoke article briefs for a
/// hub task and format them as the `## Hub Page Task` prompt section.
///
/// Data source: the focused `hub_brief` artifact when present, otherwise the
/// legacy `cannibalization_strategy` artifact (kept — still load-bearing for
/// hub tasks spawned before `hub_brief` existed).
fn hub_spoke_context(task: &Task, project_path: &str) -> String {
    // Only hub titles yield a topic — gate on the hub prefixes, then use the
    // shared stripper for the actual prefix removal.
    let hub_topic = task
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| t.starts_with("Create hub:") || t.starts_with("Refresh hub:"))
        .map(crate::engine::post_actions::strip_content_task_title_prefix)
        .unwrap_or("");

    if hub_topic.is_empty() {
        return String::new();
    }

    // Try focused hub_brief artifact first (new write_article path)
    let hub_brief_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "hub_brief")
        .and_then(|a| a.content.clone());

    let (suggested_url, suggested_title, spoke_pages) = if let Some(brief_json) = hub_brief_json {
        match serde_json::from_str::<serde_json::Value>(&brief_json) {
            Ok(brief) => {
                let url = brief
                    .get("suggested_url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .to_string();
                let title = brief
                    .get("suggested_title")
                    .and_then(|t| t.as_str())
                    .unwrap_or(hub_topic)
                    .to_string();
                let pages = brief
                    .get("spoke_pages")
                    .and_then(|p| p.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect::<Vec<_>>())
                    .unwrap_or_default();
                (url, title, pages)
            }
            Err(_) => return String::new(),
        }
    } else {
        // Legacy path: parse from full cannibalization_strategy artifact
        let strategy_json = task
            .artifacts
            .iter()
            .find(|a| a.key == "cannibalization_strategy")
            .and_then(|a| a.content.clone())
            .unwrap_or_default();

        if strategy_json.is_empty() {
            return String::new();
        }

        let strategy: serde_json::Value = match serde_json::from_str(&strategy_json) {
            Ok(v) => v,
            Err(_) => return String::new(),
        };

        let recommendations = strategy
            .get("hub_recommendations")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();

        let rec = recommendations.iter().find(|r| {
            r.get("suggested_title")
                .and_then(|t| t.as_str())
                .map(|t| t.trim().eq_ignore_ascii_case(hub_topic))
                .unwrap_or(false)
                || r.get("topic")
                    .and_then(|t| t.as_str())
                    .map(|t| t.trim().eq_ignore_ascii_case(hub_topic))
                    .unwrap_or(false)
        });

        let rec = match rec {
            Some(r) => r,
            None => return String::new(),
        };

        let url = rec
            .get("suggested_url")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string();
        let title = rec
            .get("suggested_title")
            .and_then(|t| t.as_str())
            .unwrap_or(hub_topic)
            .to_string();
        let pages = rec
            .get("spoke_pages")
            .and_then(|p| p.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect::<Vec<_>>())
            .unwrap_or_default();
        (url, title, pages)
    };

    let db_path = crate::db::default_db_path();
    let mut spokes = match rusqlite::Connection::open(&db_path) {
        Ok(conn) => {
            if spoke_pages.is_empty() {
                Vec::new()
            } else {
                crate::engine::exec::content::hub_page::gather_spoke_briefs(
                    &conn,
                    &task.project_id,
                    project_path,
                    &spoke_pages,
                )
            }
        }
        Err(_) => Vec::new(),
    };

    // Sort by impressions (highest first) and cap at 8 to stay within prompt budget.
    spokes.sort_by(|a, b| {
        b.impressions
            .partial_cmp(&a.impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    const MAX_SPOKES: usize = 8;
    spokes.truncate(MAX_SPOKES);

    let mut ctx = format!(
        "\n\n## Hub Page Task\n\n\
         You are writing a pillar / hub page MDX document.\n\
         - Topic: {}\n\
         - Title: {}\n\
         - URL slug: {}\n",
        hub_topic, suggested_title, suggested_url
    );

    if !spokes.is_empty() {
        ctx.push_str("\n### Spoke Articles to Connect\n\n");
        for spoke in &spokes {
            ctx.push_str(&format!(
                "- **{}** (`{}`)\n  Summary: {}\n\n",
                spoke.title,
                crate::content::slug::format_blog_link(&spoke.url_slug),
                spoke.excerpt
            ));
        }
    }

    const MAX_HUB_CONTEXT_BYTES: usize = 40_000;
    if ctx.len() > MAX_HUB_CONTEXT_BYTES {
        log::warn!(
            "[hub_spoke_context] context too large ({} bytes) for hub '{}'; truncating",
            ctx.len(),
            hub_topic
        );
        let mut truncated = ctx.chars().take(MAX_HUB_CONTEXT_BYTES).collect::<String>();
        truncated.push_str("\n\n[…hub context truncated…]\n");
        return truncated;
    }

    ctx
}

// ─────────────────────────────────────────────────────────────────────────────
// Kimi backend routing helper
// ─────────────────────────────────────────────────────────────────────────────

/// Return the backend preference for a given task/step.
///
/// In CLI mode, this controls the timeout: `"acp"` → 600s (content tasks),
/// `"direct"` → 300s (stateless analysis). The names are historical from the
/// bridge era; they now mean "long timeout" and "short timeout" respectively.
///
/// Content-writing and content-fixing tasks get the long timeout because they
/// involve heavy generation (reading articles, producing structured patches).
fn kimi_backend_preference_for_step(task: &Task, _step: &WorkflowStep) -> Option<&'static str> {
    match task.task_type.as_str() {
        "write_article" | "optimize_article" | "create_content" | "optimize_content"
        | "create_hub_page" | "refresh_hub_page" | "create_landing_page"
        | "fix_content_article" | "fix_ctr_article" => Some("acp"),
        _ => Some("direct"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Prompt budget preflight
// ─────────────────────────────────────────────────────────────────────────────

use crate::config::prompt_budget::{default_prompt_budget, PromptBudget};

fn default_budget_for_backend(_backend_preference: Option<&str>) -> PromptBudget {
    // Single shared budget (80 KB target / 90 KB hard) until per-backend
    // health data is wired.
    default_prompt_budget()
}

fn estimate_prompt_bytes(prompt: &str) -> usize {
    prompt.len()
}

/// Everything the generic prompt assembler needs, gathered by `exec_agentic`.
struct PromptInputs<'a> {
    step: &'a WorkflowStep,
    task: &'a Task,
    project_path: &'a str,
    site_url: &'a str,
    agent_provider: &'a str,
    content_context: &'a Option<ContentDirSnapshot>,
    next_publish_date: &'a Option<String>,
    target: Option<(&'a std::path::Path, bool)>,
}

/// Assemble the full agent prompt: skill body (or fallback task prompt),
/// optional artifact file, embedded task artifacts, then the `PromptSection`s
/// the step declares — in declaration order. Returns a failed `StepResult`
/// when a declared skill or artifact file is missing.
fn assemble_prompt(inputs: &PromptInputs) -> Result<String, StepResult> {
    use crate::engine::{prompts, skills};

    let step = inputs.step;
    let task = inputs.task;
    let repo_root = std::path::Path::new(inputs.project_path);
    let paths = ProjectPaths::from_path(inputs.project_path);

    // 1. Load skill if specified.  A declared skill is required — missing it is
    //    a hard error so the step does not silently degrade to a vague generic
    //    prompt that produces unparseable prose.
    let skill = if let Some(name) = step.params.get("skill") {
        match skills::load_skill(repo_root, name) {
            Some(s) => Some(s),
            None => {
                return Err(StepResult::fail(format!(
                        "Required skill '{}' not found in project repo or app defaults",
                        name
                    )));
            }
        }
    } else {
        None
    };

    // 2. Optionally load step artifact content into prompt context.
    let artifact_context = if let Some(artifact_name) = step.params.get(step_params::ARTIFACT) {
        let artifact_path = paths.automation_dir.join(artifact_name);
        match std::fs::read_to_string(&artifact_path) {
            Ok(content) => format!(
                "\n\n## Artifact: {}\n\n```json\n{}\n```",
                artifact_name, content
            ),
            Err(_) => {
                return Err(StepResult::fail(format!("{} not found — run collect_gsc first", artifact_name)));
            }
        }
    } else {
        String::new()
    };

    // 3. Build prompt
    let mut prompt = if let Some(ref s) = skill {
        let mut p = prompts::build_prompt(task, s, inputs.project_path, Some(inputs.site_url)).prompt;
        p.push_str(&artifact_context);
        p
    } else {
        // Fallback prompt when no skill is configured.
        // Include description so the agent knows exactly which file to edit and
        // what checks to fix — avoiding any need for shell-based file discovery.
        let desc_section = task
            .description
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
            inputs.site_url,
            inputs.project_path,
            desc_section,
        ) + &artifact_context
    };

    // Include embedded task artifacts so follow-up fix tasks receive parent context
    // (e.g. ctr_recommendations, cannibalization_strategy attached by create_*_fix_tasks).
    let wants_hub_directives = step
        .prompt_sections
        .contains(&PromptSection::HubDirectives);
    let task_artifacts: Vec<String> = task
        .artifacts
        .iter()
        .filter(|a| {
            // Hub tasks get focused hub context via hub_spoke_context(); inlining the
            // full cannibalization_strategy here duplicates data and blows the prompt
            // budget (it can be 20-90KB). Skip it for hub tasks.
            !(wants_hub_directives && a.key == "cannibalization_strategy")
        })
        .filter_map(|a| {
            a.content.as_ref().map(|c| {
                const MAX_ARTIFACT_CHARS: usize = 10_000;
                let preview = if c.len() > MAX_ARTIFACT_CHARS {
                    format!(
                        "{}… [truncated]",
                        crate::engine::text::char_prefix(c, MAX_ARTIFACT_CHARS)
                    )
                } else {
                    c.clone()
                };
                format!("\n\n## Artifact: {}\n\n```\n{}\n```", a.key, preview)
            })
        })
        .collect();
    if !task_artifacts.is_empty() {
        prompt.push_str("\n\n## Task Artifacts\n");
        prompt.push_str(&task_artifacts.join("\n"));
    }

    log::info!(
        "[executor] agentic step '{}' with provider '{}' (skill: {:?})",
        step.name,
        inputs.agent_provider,
        step.params.get(step_params::SKILL)
    );

    // 4. Append the prompt sections this step declared, in declaration order.
    for section in &step.prompt_sections {
        match section {
            PromptSection::ContentDirectives { new_article } => {
                if let Some(dirs) = content_directives(
                    task,
                    *new_article,
                    inputs.content_context,
                    inputs.next_publish_date,
                    inputs.target,
                ) {
                    prompt.push_str(&dirs);
                }
            }
            PromptSection::HubDirectives => {
                prompt.push_str(&hub_directives(task, inputs.project_path).unwrap_or_default());
            }
        }
    }

    Ok(prompt)
}

/// Execute an agentic step — invokes the configured agent with a built prompt.
///
/// Build order:
///   1. Resolve the content-dir snapshot / target path the declared sections need
///   2. Assemble the prompt via `assemble_prompt` (skill body + declared sections)
///   3. Call the agent and return the raw output as the step result
pub async fn exec_agentic(
    step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    site_url: &str,
    agent_provider: &str,
    latest_raw_output: Option<&str>,
    next_publish_date: Option<String>,
) -> StepResult {
    use crate::engine::agent;
    use std::path::Path;

    let repo_root = Path::new(project_path);

    // Prompt-assembly policy is declared on the step by its handler (issue #4
    // stage C). These derived flags also drive the content side-effects below
    // (file snapshot/rename, executor-write fallback).
    let wants_content_directives = step
        .prompt_sections
        .iter()
        .any(|s| matches!(s, PromptSection::ContentDirectives { .. }));
    let new_article = step
        .prompt_sections
        .iter()
        .any(|s| matches!(s, PromptSection::ContentDirectives { new_article: true }));

    let content_context = if wants_content_directives {
        let resolved = crate::content::locator::resolve(repo_root, None);
        resolved.selected.as_ref().map(|dir| {
            (
                dir.clone(),
                snapshot_markdown_mtime(dir),
                detect_numbered_mdx_style(dir),
            )
        })
    } else {
        None
    };

    // Text-only providers (Claude / OpenAI / Ollama rig backends) are pure
    // prompt→text completions — they cannot write the article file themselves,
    // so the executor must persist the returned MDX (see the fallback below).
    let provider_has_file_io = crate::rig::provider::provider_supports_file_io(agent_provider);
    // The provider name String below is moved into the blocking closure; keep
    // the original &str for post-call logging.
    let provider_name = agent_provider;

    // Deterministic target path for new-article tasks. Passed to the agent as
    // an exact directive and reused by the executor-write fallback, so prompt
    // and fallback never disagree on the filename.
    let target_path = if new_article {
        content_context.as_ref().map(|(dir, _, style)| {
            crate::content::naming::next_article_path(dir, *style, &task_topic_stem(task))
        })
    } else {
        None
    };

    let prompt = match assemble_prompt(&PromptInputs {
        step,
        task,
        project_path,
        site_url,
        agent_provider,
        content_context: &content_context,
        next_publish_date: &next_publish_date,
        target: target_path.as_deref().map(|p| (p, provider_has_file_io)),
    }) {
        Ok(p) => p,
        Err(result) => return result,
    };

    // Research steps use a separate workflow path
    if is_research_step(step) {
        // Research steps use the same CLI agent path as all other agentic steps
        return crate::engine::exec::research::exec_research_workflow_step(
            step,
            task,
            project_path,
            agent_provider,
            latest_raw_output,
        )
        .await;
    }

    // 4. Call agent (blocking subprocess, run in spawn_blocking)
    let agent_provider = agent_provider.to_string();
    let prompt = prompt.clone();
    let repo_root = repo_root.to_path_buf();
    let step_name = step.name.clone();

    let backend_preference = kimi_backend_preference_for_step(task, step);

    // Prompt budget preflight — fail before the provider call if the prompt
    // exceeds the hard budget for the chosen backend.
    let prompt_bytes = estimate_prompt_bytes(&prompt);
    let budget = default_budget_for_backend(backend_preference);
    if prompt_bytes > budget.hard {
        return StepResult::fail(format!(
                "Prompt size ({} bytes) exceeds hard budget ({} bytes) for step '{}'. \
                 Trim artifacts, reduce context, or batch the workflow before retrying.",
                prompt_bytes, budget.hard, step_name
            ));
    }
    if prompt_bytes > budget.target {
        log::warn!(
            "[executor] Prompt size {} exceeds target budget {} for step '{}'",
            prompt_bytes,
            budget.target,
            step_name
        );
    }

    match tokio::task::spawn_blocking(move || {
        agent::run_agent_with_backend(&agent_provider, &prompt, &repo_root, backend_preference)
    })
    .await
    {
        Ok(Ok(output)) => {
            let mut message = format!(
                "Agentic step '{}' complete ({} chars)",
                step_name,
                output.len()
            );

            if let Some((content_dir, before, style)) = content_context {
                let renamed = rename_new_or_modified_md_to_mdx(&content_dir, &before);
                if !renamed.is_empty() {
                    message.push_str(&format!(" · enforced MDX on {} file(s)", renamed.len()));
                    for (old, new) in &renamed {
                        log::info!(
                            "[content_mdx] renamed {} -> {}",
                            old.display(),
                            new.display()
                        );
                    }
                }

                if let Some(style) = style {
                    let renamed_style =
                        rename_new_files_to_numbered_mdx(&content_dir, &before, style.next_id);
                    if !renamed_style.is_empty() {
                        message.push_str(&format!(
                            " · normalized naming on {} file(s)",
                            renamed_style.len()
                        ));
                        for (old, new) in &renamed_style {
                            log::info!(
                                "[content_name] renamed {} -> {}",
                                old.display(),
                                new.display()
                            );
                        }
                    }
                }

                // Executor-write fallback (issue #13): text-only providers
                // return the article as chat text and cannot write it. When no
                // new file appeared, persist the returned MDX to the exact
                // target path so the downstream ingest/link-verify steps see a
                // real file. Skipped for file-IO providers — a missing file
                // there is an agent failure that content_write_verify reports.
                if !provider_has_file_io && new_article {
                    let new_file_appeared =
                        crate::content::locator::collect_markdown_files(&content_dir)
                            .iter()
                            .any(|p| !before.contains_key(p));
                    if !new_file_appeared {
                        if let Some(target) = &target_path {
                            match crate::engine::text::extract_mdx_document(&output) {
                                Some(mdx) => match std::fs::write(target, &mdx) {
                                    Ok(()) => {
                                        log::info!(
                                            "[content_write] provider '{}' returned MDX as text — wrote {} ({} chars)",
                                            provider_name,
                                            target.display(),
                                            mdx.len()
                                        );
                                        message.push_str(&format!(
                                            " · wrote returned MDX to {}",
                                            target.display()
                                        ));
                                    }
                                    Err(e) => {
                                        log::warn!(
                                            "[content_write] failed to write returned MDX to {}: {}",
                                            target.display(),
                                            e
                                        );
                                    }
                                },
                                None => {
                                    log::warn!(
                                        "[content_write] provider '{}' produced no file and no parseable MDX document — content_write_verify will fail the task",
                                        provider_name
                                    );
                                }
                            }
                        }
                    }
                }
            }

            StepResult {
                success: true,
                message,
                output: Some(output),
                artifact_key: None,
            }
        }
        Ok(Err(err)) => {
            log::warn!("[executor] agentic step '{}' failed: {}", step_name, err);
            StepResult::fail(format!("Agentic step '{}' failed: {}", step_name, err))
        }
        Err(e) => {
            log::warn!("[executor] agentic step '{}' task failed: {}", step_name, e);
            StepResult::fail(format!("Agentic step '{}' task failed: {}", step_name, e))
        }
    }
}

/// Load keyword coverage from the automation directory and format it for the agent prompt.
///
/// Returns a formatted string describing current topic clusters, or an empty string
/// if no coverage analysis exists.
#[allow(dead_code)]
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
    let article_count = coverage
        .get("article_count")
        .and_then(|a| a.as_i64())
        .unwrap_or(0);

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

            let keyword_list: Vec<&str> = keywords.iter().filter_map(|k| k.as_str()).collect();

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

/// Read all articles for a project from SQLite and return the next publish
/// date using the canonical `date_policy::suggest_next_safe_date` (most recent
/// unoccupied past date, capped at `MAX_LOOKBACK_DAYS` lookback).
///
/// This is more current than reading articles.json because SQLite is
/// updated by ingest_orphans before articles.json is exported.
pub(crate) fn compute_next_publish_date(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> Option<String> {
    let project_exists: bool = conn
        .query_row(
            "SELECT 1 FROM projects WHERE id = ?1 LIMIT 1",
            [project_id],
            |_| Ok(true),
        )
        .unwrap_or(false);
    if !project_exists {
        return None;
    }

    let articles = crate::engine::task_store::list_articles(conn, project_id).ok()?;
    Some(crate::content::date_policy::suggest_next_safe_date(&articles))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
        TaskStatus,
    };

    fn make_task(task_type: &str) -> Task {
        Task {
            id: format!("test-{task_type}"),
            task_type: task_type.to_string(),
            phase: "research".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::Optional,
            title: Some(format!("{task_type} test")),
            description: None,
            project_id: "proj1".to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun {
                attempts: 0,
                last_error: None,
                ..Default::default()
            },
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        }
    }

    #[test]
    fn test_kimi_backend_preference_for_step_content() {
        let step = WorkflowStep::new("test", crate::engine::workflows::StepKind::Agentic);
        for tt in [
            "write_article",
            "optimize_article",
            "create_content",
            "optimize_content",
            "create_hub_page",
            "refresh_hub_page",
        ] {
            let task = make_task(tt);
            assert_eq!(
                kimi_backend_preference_for_step(&task, &step),
                Some("acp"),
                "{} should prefer acp",
                tt
            );
        }
    }

    #[test]
    fn test_kimi_backend_preference_for_step_other() {
        let step = WorkflowStep::new("test", crate::engine::workflows::StepKind::Agentic);
        for tt in [
            "content_audit",
            "collect_gsc",
            "cluster_and_link",
            "reddit_reply",
        ] {
            let task = make_task(tt);
            assert_eq!(
                kimi_backend_preference_for_step(&task, &step),
                Some("direct"),
                "{} should prefer direct",
                tt
            );
        }
    }

    #[test]
    fn test_prompt_budget_defaults() {
        let b = default_budget_for_backend(Some("acp"));
        assert_eq!(b.target, 80 * 1024);
        assert_eq!(b.hard, 90 * 1024);
    }

    #[test]
    fn test_estimate_prompt_bytes() {
        assert_eq!(estimate_prompt_bytes("hello"), 5);
        assert_eq!(estimate_prompt_bytes(""), 0);
    }

    // ── task_topic_stem ──────────────────────────────────────────────────────

    #[test]
    fn test_task_topic_stem_prefers_target_keyword() {
        let mut task = make_task("write_article");
        task.description =
            Some("Target keyword: gamma scalping strategy\nKD: 35\nVolume: 3000".to_string());
        assert_eq!(task_topic_stem(&task), "gamma scalping strategy");
    }

    #[test]
    fn test_task_topic_stem_strips_title_prefixes() {
        let mut task = make_task("write_article");
        task.title = Some("Write article: delta hedging".to_string());
        assert_eq!(task_topic_stem(&task), "delta hedging");

        task.title = Some("Create hub: options greeks".to_string());
        assert_eq!(task_topic_stem(&task), "options greeks");
    }

    #[test]
    fn test_task_topic_stem_falls_back_to_article() {
        let mut task = make_task("write_article");
        task.title = None;
        assert_eq!(task_topic_stem(&task), "article");
    }

    // ── content_directives target path ───────────────────────────────────────

    fn numbered_ctx(
        next_id: i64,
    ) -> Option<(
        std::path::PathBuf,
        std::collections::HashMap<std::path::PathBuf, std::time::SystemTime>,
        Option<crate::content::naming::NumberedMdxStyle>,
    )> {
        Some((
            std::path::PathBuf::from("/repo/content"),
            std::collections::HashMap::new(),
            Some(crate::content::naming::NumberedMdxStyle { next_id }),
        ))
    }

    #[test]
    fn test_content_directives_exact_target_for_file_io_provider() {
        let task = make_task("write_article");
        let target = std::path::PathBuf::from("/repo/content/7_gamma_scalping.mdx");
        let out = content_directives(
            &task,
            true,
            &numbered_ctx(7),
            &Some("2024-01-01".to_string()),
            Some((&target, true)),
        )
        .unwrap();
        assert!(out.contains("/repo/content/7_gamma_scalping.mdx"));
        assert!(out.contains("EXACTLY"));
        assert!(!out.contains("approximately"));
    }

    #[test]
    fn test_content_directives_text_only_provider_returns_mdx() {
        let task = make_task("write_article");
        let target = std::path::PathBuf::from("/repo/content/7_gamma_scalping.mdx");
        let out =
            content_directives(&task, true, &numbered_ctx(7), &None, Some((&target, false))).unwrap();
        assert!(out.contains("/repo/content/7_gamma_scalping.mdx"));
        assert!(out.contains("You cannot write files"));
        assert!(out.contains("Return ONLY the complete MDX content"));
    }

    #[test]
    fn test_content_directives_approximate_hint_without_target() {
        let task = make_task("optimize_article");
        let out = content_directives(&task, false, &numbered_ctx(7), &None, None).unwrap();
        assert!(out.contains("approximately 7"));
    }

    #[test]
    fn test_content_directives_none_for_non_content_task() {
        let task = make_task("content_audit");
        assert!(content_directives(&task, false, &None, &None, None).is_none());
    }
}
