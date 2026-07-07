//! Step execution helpers used by the executor for agentic and deterministic steps.
//!
//! Extracted verbatim from `engine/workflows/handlers.rs` as part of Stage A of
//! the structural-debt cleanup (issue #4). No behavior changes — only relocated
//! across module boundaries. Stage C will refactor `exec_agentic`'s boolean
//! lattice; until then this file intentionally preserves the existing shape.

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::{step_params, StepResult, WorkflowStep};
use crate::models::task::Task;

// Helpers relocated to `content/naming.rs` (Stage A.1). Imported by name so the
// call sites inside `exec_agentic` remain byte-for-byte identical to the
// pre-move source.
use crate::content::naming::{
    detect_numbered_mdx_style, rename_new_files_to_numbered_mdx, rename_new_or_modified_md_to_mdx,
    snapshot_markdown_mtime,
};

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
pub async fn exec_deterministic(
    step: &WorkflowStep,
    _task: &Task,
    project_path: &str,
    _seo_provider: &str,
) -> StepResult {
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
    })
    .await
    {
        Ok(Ok(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = if stderr.is_empty() {
                stdout.clone()
            } else {
                format!("{}\n[stderr]\n{}", stdout, stderr)
            };
            if out.status.success() {
                StepResult {
                    success: true,
                    message: format!("Step '{}' OK", step_name),
                    output: Some(combined),
                }
            } else {
                StepResult {
                    success: false,
                    message: format!(
                        "Step '{}' failed (exit {}): {}",
                        step_name,
                        out.status,
                        stderr.trim()
                    ),
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
fn hub_spoke_context(task: &Task, project_path: &str) -> String {
    let hub_topic = task
        .title
        .as_deref()
        .and_then(|t| {
            t.strip_prefix("Create hub:")
                .or_else(|| t.strip_prefix("Refresh hub:"))
        })
        .unwrap_or("")
        .trim();

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
        | "create_hub_page" | "refresh_hub_page"
        | "fix_content_article" | "fix_ctr_article" => Some("acp"),
        _ => Some("direct"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Prompt budget preflight
// ─────────────────────────────────────────────────────────────────────────────

struct PromptBudget {
    target: usize,
    hard: usize,
}

fn default_budget_for_backend(_backend_preference: Option<&str>) -> PromptBudget {
    // Defaults mirror the Kimi bridge limits until health data is wired.
    // Bridge hard limit is 100 KB based on live evidence (reddit_enrich ~25 KB,
    // CTR audit ~46 KB). Target leaves headroom for JSON/preamble overhead.
    PromptBudget {
        target: 80 * 1024,
        hard: 90 * 1024,
    }
}

fn estimate_prompt_bytes(prompt: &str) -> usize {
    prompt.len()
}

pub async fn exec_agentic(
    step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    site_url: &str,
    agent_provider: &str,
    latest_raw_output: Option<&str>,
    next_publish_date: Option<String>,
) -> StepResult {
    use crate::engine::project_paths::ProjectPaths;
    use crate::engine::{agent, prompts, skills};
    use std::path::Path;

    let repo_root = Path::new(project_path);
    let paths = ProjectPaths::from_path(project_path);

    let is_content_task = matches!(
        task.task_type.as_str(),
        "write_article"
            | "optimize_article"
            | "create_content"
            | "optimize_content"
            | "create_hub_page"
            | "refresh_hub_page"
            | "fix_content_article"
    );
    let is_new_article_task = matches!(
        task.task_type.as_str(),
        "write_article" | "create_content" | "create_hub_page" | "refresh_hub_page"
    );
    let is_hub_task = step.params.get(step_params::SKILL).map(|s| s.as_str()) == Some("hub-write");

    let content_context = if is_content_task {
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

    // 1. Load skill if specified.  A declared skill is required — missing it is
    //    a hard error so the step does not silently degrade to a vague generic
    //    prompt that produces unparseable prose.
    let skill = if let Some(name) = step.params.get("skill") {
        match skills::load_skill(repo_root, name) {
            Some(s) => Some(s),
            None => {
                return StepResult {
                    success: false,
                    message: format!(
                        "Required skill '{}' not found in project repo or app defaults",
                        name
                    ),
                    output: None,
                };
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
            site_url,
            project_path,
            desc_section,
        ) + &artifact_context
    };

    // Include embedded task artifacts so follow-up fix tasks receive parent context
    // (e.g. ctr_recommendations, cannibalization_strategy attached by create_*_fix_tasks).
    let task_artifacts: Vec<String> = task
        .artifacts
        .iter()
        .filter(|a| {
            // Hub tasks get focused hub context via hub_spoke_context(); inlining the
            // full cannibalization_strategy here duplicates data and blows the prompt
            // budget (it can be 20-90KB). Skip it for hub tasks.
            !(is_hub_task && a.key == "cannibalization_strategy")
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

    if is_content_task {
        // Pre-compute the next safe publish date and inject it into the prompt.
        // Without this, the agent defaults to today's date which conflicts with
        // articles already in the database and breaks the date distribution.
        // We read from SQLite (canonical source of truth) rather than articles.json
        // so queued tasks see the most current state after orphan ingestion.
        if let Some(ref date) = next_publish_date {
            if is_new_article_task {
                prompt.push_str(&format!(
                    "\n\n## Publish Date (Required)\n\
                             - The frontmatter `date:` field MUST be exactly: `{date}`\n\
                             - Do not use today's date or any other value — use the date above."
                ));
            } else {
                // Modification tasks: preserve existing date, avoid collisions
                prompt.push_str(&format!(
                    "\n\n## Publish Date (Preserve)\n\
                             - Preserve the existing `date:` field in the frontmatter.\n\
                             - Do NOT change it to today's date, a future date, or a date already used by another article.\n\
                             - If the article has no date, use exactly: `{date}`"
                ));
            }
        }

        prompt.push_str(
            "\n\n## Content File Format (Required)\n\
                     - New articles must be written as `.mdx` files (never `.md`).\n\
                     - If you propose a filename, it must end in `.mdx`.\n\
                     - Preserve valid frontmatter and markdown/MDX syntax.",
        );

        prompt.push_str(
            "\n\n## Internal Link Format (Required)\n\
                     - All internal links MUST use standard markdown syntax: `[anchor text](/blog/slug)`\n\
                     - The URL path must be wrapped in parentheses `()` immediately after the closing bracket `]`.\n\
                     - WRONG: `[anchor text]/blog/slug` or `[anchor text] /blog/slug`\n\
                     - CORRECT: `[anchor text](/blog/slug)`\n\
                     - If you include a 'Related Articles' section, use the same `[title](/blog/slug)` format for every bullet.",
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

    if is_hub_task {
        prompt.push_str(&hub_spoke_context(task, project_path));
        prompt.push_str(
            "\n\n## Hub Page Requirements\n\
             - Write a comprehensive pillar / hub page MDX document.\n\
             - YAML frontmatter MUST include `type: hub` and `hub_topic`.\n\
             - Include an H1 matching the hub title.\n\
             - Link to every spoke article using `/blog/{slug}` format.\n\
             - Total word count MUST be 1500+ words.\n\
             - The frontmatter `title:` and the body H1 must be complete, grammatically correct phrases. They must NOT end mid-sentence or with dangling words such as `a`, `an`, `the`, `and`, `or`, `to`, `for`, `of`, `in`, `on`, `with`, `by`, `from`, `as`, `is`, `are`, `what`, `how`, `when`, `where`, `why`, `which`, `complete`, `guide`, `income`, `without`, `track`, `close`, `compared`, or trailing punctuation (`:`, `,`, `-`). Rewrite rather than truncate.\n\
             - The first body paragraph must begin with a complete sentence; do not drop leading characters from the opening sentence.\n\
             - Return ONLY the complete MDX content. No explanations outside the MDX.\n",
        );
    }

    // Check if this is a research workflow step that uses ToolCallingAgent
    // Note: research_final_selection is now deterministic, not agentic
    let is_research_step = matches!(
        step.name.as_str(),
        "research_seed_extraction" | "research_keyword_discovery" | "research_seed_validation"
    );

    if is_research_step {
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
        return StepResult {
            success: false,
            message: format!(
                "Prompt size ({} bytes) exceeds hard budget ({} bytes) for step '{}'. \
                 Trim artifacts, reduce context, or batch the workflow before retrying.",
                prompt_bytes, budget.hard, step_name
            ),
            output: None,
        };
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

/// Read all articles for a project from SQLite and return the next
/// unoccupied past date using the canonical `date_policy::suggest_next_safe_date`.
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
}
