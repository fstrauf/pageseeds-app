//! Agentic relevance check for research final selection.
//!
//! DataForSEO expansion can return same-vocabulary but off-domain / vertical-drift
//! candidates. One batched LLM call flags them; a pure filter applies the result.
//! Non-fatal: on failure the deterministic shortlist stands.

use crate::strategy::ProjectStrategy;

use super::final_selection::{selected_count, KeywordPickerOutput};

/// Apply an off-domain list to the shortlist (case-insensitive, trimmed).
/// Pure — unit-tested without an LLM. Returns the number removed.
pub(crate) fn apply_off_domain_filter(
    output: &mut KeywordPickerOutput,
    off_domain: &std::collections::HashSet<String>,
) -> usize {
    if off_domain.is_empty() {
        return 0;
    }
    let before = selected_count(output);
    output
        .landing_page_candidates
        .retain(|c| !off_domain.contains(&c.keyword.trim().to_lowercase()));
    if let Some(d) = &mut output.difficulty {
        d.results
            .retain(|k| !off_domain.contains(&k.keyword.trim().to_lowercase()));
    }
    before - selected_count(output)
}

/// Build optional strategy block for the relevance user prompt.
///
/// Surfaces `do_not_expand` phrases plus primary / ACTIVE cluster names so the
/// LLM can prefer product-adjacent expansions and reject vertical drift.
/// Returns `None` when strategy is empty or has nothing useful to inject.
pub(crate) fn strategy_context_for_relevance(strategy: &ProjectStrategy) -> Option<String> {
    if strategy.is_empty() {
        return None;
    }
    let mut lines: Vec<String> = Vec::new();
    if !strategy.do_not_expand.is_empty() {
        lines.push(format!(
            "do_not_expand phrases: {}",
            strategy.do_not_expand.join(", ")
        ));
    }
    if !strategy.primary_keywords.is_empty() {
        lines.push(format!(
            "primary keywords: {}",
            strategy.primary_keywords.join(", ")
        ));
    }
    let active: Vec<&str> = strategy
        .clusters
        .iter()
        .filter(|c| c.status == crate::strategy::ClusterStatus::Active)
        .map(|c| c.name.as_str())
        .collect();
    if !active.is_empty() {
        lines.push(format!("ACTIVE clusters: {}", active.join(", ")));
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

/// One batched LLM call flagging off-domain / vertical-drift candidates.
/// Non-fatal: returns 0 (keeps everything) when the check is unavailable.
pub(crate) fn filter_off_domain_candidates(
    output: &mut KeywordPickerOutput,
    themes: &[String],
    project_path: &str,
    agent_provider: &str,
    strategy: &ProjectStrategy,
) -> usize {
    let keywords: Vec<String> = if !output.landing_page_candidates.is_empty() {
        output
            .landing_page_candidates
            .iter()
            .map(|c| c.keyword.clone())
            .collect()
    } else {
        output
            .difficulty
            .as_ref()
            .map(|d| d.results.iter().map(|k| k.keyword.clone()).collect())
            .unwrap_or_default()
    };
    if keywords.is_empty() {
        return 0;
    }

    let brief = std::fs::read_to_string(
        crate::engine::project_paths::ProjectPaths::from_path(project_path)
            .automation_dir
            .join("project.md"),
    )
    .unwrap_or_else(|_| "(no brief found)".to_string());
    const MAX_BRIEF_LEN: usize = 8_000;
    let brief_trimmed = if brief.len() > MAX_BRIEF_LEN {
        format!("{}… [truncated]", &brief[..MAX_BRIEF_LEN])
    } else {
        brief
    };

    let strategy_section = match strategy_context_for_relevance(strategy) {
        Some(ctx) => format!("\n\n## Strategy Context\n\n{}", ctx),
        None => {
            log::info!("[relevance_check] strategy empty — omitting strategy section");
            String::new()
        }
    };

    let system = include_str!("../../../prompts/candidate_relevance.md");
    let user = format!(
        "## Project Context\n\n{}{}\n\n## Research Themes\n\n{}\n\n## Candidate Keywords\n\n{}",
        brief_trimmed,
        strategy_section,
        themes.join(", "),
        keywords.join("\n")
    );
    let prompt = format!("{}\n\n---\n\n{}", system, user);

    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[relevance_check] Failed to create runtime: {}", e);
            return 0;
        }
    };

    let result = rt.block_on(async {
        crate::rig::extraction::extract_structured::<
            crate::models::research::CandidateRelevanceOutput,
        >(agent_provider, &prompt, Some(system), Some("direct"), None)
        .await
    });

    match result {
        Ok(check) => {
            let off_domain: std::collections::HashSet<String> = check
                .off_domain_keywords
                .iter()
                .map(|k| k.trim().to_lowercase())
                .filter(|k| !k.is_empty())
                .collect();
            let removed = apply_off_domain_filter(output, &off_domain);
            if removed > 0 {
                log::info!(
                    "[relevance_check] removed {} off-domain candidates: {:?}",
                    removed,
                    off_domain
                );
            } else {
                log::info!("[relevance_check] all {} candidates on-domain", keywords.len());
            }
            removed
        }
        Err(e) => {
            log::warn!(
                "[relevance_check] extraction failed ({}); keeping deterministic shortlist",
                e
            );
            0
        }
    }
}
