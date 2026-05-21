/// Hybrid handler for fix_indexing and fix_technical tasks.
///
/// Step 1 (deterministic): Load the target MDX file and extract structured context
/// so the agent doesn't waste time hunting for files.
///
/// Step 2 (agentic): Apply the fix based on the GSC issue and page context.
use std::collections::HashSet;
use std::path::Path;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct IndexingFixContext {
    pub url: String,
    pub file_path: Option<String>,
    pub exists: bool,
    pub word_count: usize,
    pub h1: Option<String>,
    pub title: Option<String>,
    pub meta_description: Option<String>,
    pub canonical: Option<String>,
    pub publish_date: Option<String>,
    pub internal_links: Vec<String>,
    pub internal_link_count: usize,
}

/// Deterministic pre-step: gather structured context for the target URL.
pub(crate) fn exec_indexing_fix_context(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Parse URL from task description
    let url = task
        .description
        .as_deref()
        .and_then(|d| d.lines().next())
        .and_then(|line| line.strip_prefix("URL: "))
        .unwrap_or("")
        .to_string();

    if url.is_empty() {
        return StepResult {
            success: false,
            message: "Task description missing URL".to_string(),
            output: None,
        };
    }

    // Resolve content directory
    let content_dir = crate::content::locator::resolve(Path::new(project_path), None)
        .selected
        .unwrap_or_else(|| paths.repo_root.clone());

    // Try to find the MDX file matching the URL slug
    let slug = crate::content::slug::extract_slug_from_url(&url);
    let file_match = find_mdx_by_slug(&content_dir, &slug);

    log::info!(
        "[indexing_fix_context] url={} content_dir={} slug={} matched={}",
        url,
        content_dir.display(),
        slug,
        file_match
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "none".to_string())
    );

    let mut ctx = IndexingFixContext {
        url: url.clone(),
        file_path: file_match.as_ref().map(|p| p.to_string_lossy().to_string()),
        exists: file_match.is_some(),
        word_count: 0,
        h1: None,
        title: None,
        meta_description: None,
        canonical: None,
        publish_date: None,
        internal_links: vec![],
        internal_link_count: 0,
    };

    if let Some(ref path) = file_match {
        if let Ok(content) = std::fs::read_to_string(path) {
            ctx.word_count = crate::content::ops::count_words(&content);
            ctx.h1 = extract_first_h1(&content);
            ctx.title = crate::content::frontmatter::extract_frontmatter_string(&content, "title");
            ctx.meta_description = crate::content::frontmatter::extract_frontmatter_string(&content, "description");
            ctx.canonical = crate::content::frontmatter::extract_frontmatter_string(&content, "canonical");
            ctx.publish_date = crate::content::frontmatter::extract_frontmatter_string(&content, "date");
            ctx.internal_links = extract_internal_links(&content);
            ctx.internal_link_count = ctx.internal_links.len();
        }
    }

    let output = serde_json::to_string_pretty(&ctx).unwrap_or_default();

    if !ctx.exists {
        return StepResult {
            success: false,
            message: format!(
                "No MDX file found for {} (slug={}). Cannot fix indexing for a page that has no content file.",
                url, slug
            ),
            output: Some(output),
        };
    }

    StepResult {
        success: true,
        message: format!(
            "Context loaded for {}: {} words, {} internal links{}",
            url,
            ctx.word_count,
            ctx.internal_link_count,
            if ctx.exists { "" } else { " (file not found)" }
        ),
        output: Some(output),
    }
}

/// Agentic step: apply the indexing fix.
pub(crate) fn exec_indexing_fix_apply(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: Option<&str>,
) -> StepResult {
    use crate::engine::agent;
    use std::path::Path;

    let url = task
        .description
        .as_deref()
        .and_then(|d| d.lines().next())
        .and_then(|line| line.strip_prefix("URL: "))
        .unwrap_or("")
        .to_string();

    let reason = task
        .description
        .as_deref()
        .and_then(|d| d.lines().nth(1))
        .and_then(|line| line.strip_prefix("Issue: "))
        .unwrap_or("unknown");

    let action = task
        .description
        .as_deref()
        .and_then(|d| d.lines().nth(2))
        .and_then(|line| line.strip_prefix("Action: "))
        .unwrap_or("");

    let (context_block, file_path_hint) = context_json
        .and_then(|j| serde_json::from_str::<IndexingFixContext>(j).ok())
        .map(|ctx| {
            let block = format!(
                "\n\n## Page Context (Deterministic)\n\n```json\n{}\n```",
                serde_json::to_string_pretty(&ctx).unwrap_or_default()
            );
            let hint = ctx.file_path.unwrap_or_default();
            (block, hint)
        })
        .unwrap_or_default();

    // Load cluster context from task artifacts (set by indexing_health_campaign)
    let cluster_context_block = task
        .artifacts
        .iter()
        .find(|a| a.key == "indexing_target_context")
        .and_then(|a| a.content.as_ref())
        .and_then(|json| serde_json::from_str::<crate::models::indexing_health::IndexingTargetContext>(json).ok())
        .map(|ctx| {
            let siblings = match &ctx.cluster {
                Some(c) => serde_json::to_string_pretty(&c.siblings).unwrap_or_default(),
                None => "[]".to_string(),
            };
            format!(
                "\n\n## Cluster Context (from site-wide audit)\n\nThis page belongs to the '{}' cluster.\n\nSibling articles that may overlap topically:\n```json\n{}```\n\nShared headings detected in cluster: {:?}\n\nWhen editing, ensure the title, H1, and opening sections are DISTINCT from these siblings.",
                ctx.cluster.as_ref().map(|c| c.theme.clone()).unwrap_or_default(),
                siblings,
                ctx.cluster.as_ref().and_then(|c| c.shared_headings.clone()).unwrap_or_default()
            )
        })
        .unwrap_or_default();

    let file_instruction = if file_path_hint.is_empty() {
        "Find the MDX file for this URL in the content directory and edit it directly.".to_string()
    } else {
        format!(
            "The target MDX file is: {}\n\
             You MUST edit this exact file. Do not create a new file unless this one does not exist.",
            file_path_hint
        )
    };

    let prompt = format!(
        "## Task: Fix Indexing Issue\n\n\
         - Task ID: {}\n\
         - URL: {}\n\
         - Issue: {}\n\
         - Suggested Action: {}\n\
         - Repo: {}\n\
         {}\
         {}\n\n\
         ## Instructions\n\n\
         {}\n\n\
         For `not_indexed_crawled` / `not_indexed_discovered` / `not_indexed_other`:\n\
         - Improve content depth and uniqueness (aim for 600+ words if currently thin)\n\
         - Add 3-5 relevant internal links to other pages on the site\n\
         - Ensure the H1 and title are specific and distinct from similar pages\n\
         - Add a clear meta description\n\n\
         For `robots_blocked` / `noindex` / `fetch_error` / `canonical_mismatch`:\n\
         - Fix the technical root cause in the MDX frontmatter or site config\n\
         - Explain what you changed and why\n\n\

         For `not_indexed_crawled` specifically (page is crawled but not indexed):\n\
         - This usually means Google sees the page but chooses not to index it.\n\
         - The page is already long and may have internal links — focus on DISTINCTIVENESS, not just length.\n\
         - Make the title, H1, and opening sections clearly different from cluster siblings listed above.\n\
         - Remove or merge sections that overlap with sibling articles.\n\
         - If the page cannot be made distinct enough, suggest a merge target instead.\n\


         CRITICAL: You MUST actually write changes to the file. Do NOT just describe what you would do.\n\
         Do NOT create any markdown reports, summary files, or documentation.\n\
         Only edit the MDX file and return a brief text summary of what you changed.\n\n\
         Return a brief summary of changes made.",
        task.id,
        url,
        reason,
        action,
        project_path,
        context_block,
        cluster_context_block,
        file_instruction,
    );

    match agent::run_agent(agent_provider, &prompt, Path::new(project_path)) {
        Ok(output) => StepResult {
            success: true,
            message: format!("Fix applied to {} ({} chars)", url, output.len()),
            output: Some(output),
        },
        Err(e) => StepResult {
            success: false,
            message: format!("Agent failed to apply fix: {}", e),
            output: None,
        },
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn find_mdx_by_slug(content_dir: &Path, slug: &str) -> Option<std::path::PathBuf> {
    if slug.is_empty() {
        return None;
    }

    // Strip numeric prefix from URL segments too (e.g. "127_net_worth_tracker" → "net_worth_tracker")
    let last_segment = crate::content::slug::strip_numeric_prefix(
        slug.trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or(slug),
    )
    .replace('_', "-");

    let full_slug_dashed = crate::content::slug::strip_numeric_prefix(slug.trim_end_matches('/'))
        .replace('/', "-")
        .replace('_', "-");

    let mut best_match: Option<std::path::PathBuf> = None;

    for entry in walkdir::WalkDir::new(content_dir)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("mdx") {
            continue;
        }

        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        let stem_clean = crate::content::slug::strip_numeric_prefix(stem).replace('_', "-");

        // Exact stem match on last segment — highest confidence
        if stem_clean == last_segment {
            return Some(path.to_path_buf());
        }

        // Full slug match (for flat structures)
        if stem_clean == full_slug_dashed && best_match.is_none() {
            best_match = Some(path.to_path_buf());
        }

        // Also check if the relative path (without extension) matches the slug
        if let Ok(rel) = path.strip_prefix(content_dir) {
            let rel_str = rel
                .to_string_lossy()
                .replace(std::path::MAIN_SEPARATOR, "/");
            let rel_without_ext = rel_str.trim_end_matches(".mdx").trim_end_matches(".md");
            let rel_clean = crate::content::slug::strip_numeric_prefix(rel_without_ext)
                .replace('/', "-")
                .replace('_', "-");
            if rel_clean == full_slug_dashed {
                return Some(path.to_path_buf());
            }
        }
    }

    best_match
}

fn extract_first_h1(content: &str) -> Option<String> {
    for line in content.lines() {
        if line.trim_start().starts_with("# ") {
            return Some(line.trim_start_matches("# ").trim().to_string());
        }
    }
    None
}

fn extract_internal_links(content: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut seen = HashSet::new();

    // Simple markdown link extraction: [text](path)
    let re = regex::Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").unwrap();
    for cap in re.captures_iter(content) {
        let href = cap[2].to_string();
        if href.starts_with('/') && !href.starts_with("//") && !seen.contains(&href) {
            seen.insert(href.clone());
            links.push(href);
        }
    }
    links
}
