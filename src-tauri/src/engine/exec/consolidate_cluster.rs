/// Consolidate cluster execution module.
///
/// Covers:
///   - merge_load_plan          (deterministic)
///   - merge_preflight          (deterministic)
///   - merge_extract_sections   (deterministic)
///   - merge_draft_patch        (agentic with merge-content skill)
///   - merge_apply_patch        (deterministic)
///   - merge_generate_redirects (deterministic)
///   - merge_validate_output    (deterministic)
use std::path::{Path, PathBuf};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::merge_patch::{
    ContentMergePatch, ExtractedExample, ExtractedFaq, ExtractedHeading, ExtractedTable,
    MergePreflightReport, MergeValidationReport, RedirectRule, SectionInventory,
};
use crate::models::task::Task;

// ═══════════════════════════════════════════════════════════════════════════════
// Step 1: Load Plan
// ═══════════════════════════════════════════════════════════════════════════════

/// Load the approved merge plan for this cluster from the strategy artifact.
pub(crate) fn exec_merge_load_plan(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Extract cluster_id from task title (e.g. "Merge cluster: cash_secured_puts")
    let cluster_id = task
        .title
        .as_deref()
        .and_then(|t| t.strip_prefix("Merge cluster:"))
        .unwrap_or("")
        .trim();

    if cluster_id.is_empty() {
        return StepResult {
            success: false,
            message: "Cannot determine cluster_id from task title".to_string(),
            output: None,
        };
    }

    // Find strategy artifact on task
    let strategy_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "cannibalization_strategy")
        .and_then(|a| a.content.clone())
        .unwrap_or_default();

    let strategy_json = if strategy_json.is_empty() {
        // Fallback: read from automation dir
        let path = paths.automation_dir.join("cannibalization_strategy.json");
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        strategy_json
    };

    if strategy_json.is_empty() {
        return StepResult {
            success: false,
            message: "No cannibalization_strategy artifact found".to_string(),
            output: None,
        };
    }

    let strategy: serde_json::Value = match serde_json::from_str(&strategy_json) {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid strategy JSON: {}", e),
                output: None,
            };
        }
    };

    let recommendations = strategy["merge_recommendations"].as_array();
    let rec = recommendations.and_then(|arr| {
        arr.iter()
            .find(|r| r["cluster_id"].as_str().unwrap_or("") == cluster_id)
    });

    let rec = match rec {
        Some(r) => r.clone(),
        None => {
            return StepResult {
                success: false,
                message: format!("No merge recommendation found for cluster '{}'", cluster_id),
                output: None,
            };
        }
    };

    let output = serde_json::to_string_pretty(&rec).unwrap_or_default();
    StepResult {
        success: true,
        message: format!("Loaded merge plan for cluster: {}", cluster_id),
        output: Some(output),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 2: Preflight
// ═══════════════════════════════════════════════════════════════════════════════

/// Run preflight checks before merging.
pub(crate) fn exec_merge_preflight(
    task: &Task,
    project_path: &str,
    _plan_json: &str,
) -> StepResult {
    let plan_json = load_plan_from_task_or_file(task, project_path);
    let plan: serde_json::Value = match serde_json::from_str(&plan_json) {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid merge plan JSON: {}", e),
                output: None,
            };
        }
    };

    let keep_url = plan["keep_url"].as_str().unwrap_or("");
    let redirect_urls: Vec<String> = plan["redirect_urls"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    // Resolve keeper file from URL slug
    let keeper_slug = keep_url
        .trim_start_matches("/blog/")
        .trim_start_matches('/');
    let keeper_file = find_file_by_slug(project_path, keeper_slug);
    let keeper_exists = keeper_file.as_ref().map(|p| p.exists()).unwrap_or(false);

    // Check keeper is indexable (no noindex in frontmatter)
    let keeper_indexable = keeper_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|content| !content.to_lowercase().contains("noindex"))
        .unwrap_or(false);

    // Check redirect files exist
    let mut redirect_files_exist = Vec::new();
    let mut redirect_files_missing = Vec::new();
    let mut redirect_cycles = Vec::new();

    for url in &redirect_urls {
        let slug = url.trim_start_matches("/blog/").trim_start_matches('/');
        if slug == keeper_slug {
            redirect_cycles.push(url.clone());
            continue;
        }
        match find_file_by_slug(project_path, slug) {
            Some(p) if p.exists() => redirect_files_exist.push(url.clone()),
            _ => redirect_files_missing.push(url.clone()),
        }
    }

    let can_proceed = keeper_exists
        && keeper_indexable
        && redirect_files_missing.is_empty()
        && redirect_cycles.is_empty();

    let report = MergePreflightReport {
        keeper_file_exists: keeper_exists,
        keeper_is_indexable: keeper_indexable,
        redirect_files_exist: redirect_files_exist.clone(),
        redirect_files_missing: redirect_files_missing.clone(),
        redirect_cycles_detected: redirect_cycles.clone(),
        can_proceed,
        notes: vec![],
    };

    let output = serde_json::to_string_pretty(&report).unwrap_or_default();
    StepResult {
        success: can_proceed,
        message: if can_proceed {
            "Preflight passed: all files exist, no cycles detected".to_string()
        } else {
            format!(
                "Preflight failed: keeper_exists={}, keeper_indexable={}, missing={:?}, cycles={:?}",
                keeper_exists, keeper_indexable, redirect_files_missing, redirect_cycles
            )
        },
        output: Some(output),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3: Extract Unique Sections
// ═══════════════════════════════════════════════════════════════════════════════

/// Extract headings, tables, examples, and FAQs from redirect pages.
pub(crate) fn exec_merge_extract_sections(task: &Task, project_path: &str) -> StepResult {
    let plan_json = load_plan_from_task_or_file(task, project_path);
    let plan: serde_json::Value = match serde_json::from_str(&plan_json) {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid merge plan JSON: {}", e),
                output: None,
            };
        }
    };

    let keep_url = plan["keep_url"].as_str().unwrap_or("");
    let redirect_urls: Vec<String> = plan["redirect_urls"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    let keeper_slug = keep_url
        .trim_start_matches("/blog/")
        .trim_start_matches('/');
    let keeper_file = find_file_by_slug(project_path, keeper_slug);
    let keeper_content = keeper_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();

    // Build a capped keeper representation.
    // The agent needs the heading structure for insertion points and a prose excerpt
    // for tone matching and duplicate detection, but not the full article.
    let (_keeper_fm, keeper_body) = crate::content::frontmatter::split_mdx(&keeper_content)
        .map(|(fm, b)| (fm, b))
        .unwrap_or(("", keeper_content.as_str()));
    // Lightweight keeper outline: just heading levels and titles (no body).
    // The agent only needs these for insertion-point selection.
    let keeper_outline: Vec<serde_json::Value> = keeper_body
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("## ") || t.starts_with("### ") || t.starts_with("#### ")
        })
        .map(|l| {
            let level = l.trim_start().chars().take_while(|&c| c == '#').count() as u8;
            let text = l.trim_start_matches('#').trim().to_string();
            serde_json::json!({"level": level, "text": text})
        })
        .collect();
    const KEEPER_EXCERPT_CHARS: usize = 1_500;
    let keeper_excerpt = if keeper_body.chars().count() > KEEPER_EXCERPT_CHARS {
        let mut excerpt = String::new();
        let mut count = 0;
        for ch in keeper_body.chars() {
            if count >= KEEPER_EXCERPT_CHARS {
                excerpt.push_str("\n\n[…excerpt truncated…]");
                break;
            }
            excerpt.push(ch);
            count += 1;
        }
        excerpt
    } else {
        keeper_body.to_string()
    };

    // Build compact summaries for each redirect page.
    // The agent only needs enough context to identify unique topics and decide
    // what to merge — full tables, code blocks, and FAQ answers are too large
    // when there are many redirect pages.
    #[derive(Debug)]
    struct RedirectSummary {
        file: String,
        url: String,
        title: String,
        word_count: usize,
        excerpt: String,
        headings: Vec<String>,
        has_tables: bool,
        has_examples: bool,
        has_faqs: bool,
    }
    let mut summaries: Vec<RedirectSummary> = Vec::new();

    for url in &redirect_urls {
        let slug = url.trim_start_matches("/blog/").trim_start_matches('/');
        let file = match find_file_by_slug(project_path, slug) {
            Some(p) => p,
            None => continue,
        };
        let content = match std::fs::read_to_string(&file) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let (frontmatter, body) = match crate::content::frontmatter::split_mdx(&content) {
            Some((fm, b)) => (fm, b),
            None => ("", content.as_str()),
        };

        let title = crate::content::frontmatter::parse(frontmatter)
            .ok()
            .and_then(|fm| {
                fm.parsed
                    .get("title")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            })
            .unwrap_or_else(|| slug.replace('-', " "));

        let word_count = crate::content::ops::count_words(&body);

        // Excerpt: first 200 chars of body
        const EXCERPT_CHARS: usize = 200;
        let excerpt = if body.chars().count() > EXCERPT_CHARS {
            let mut e = String::new();
            let mut count = 0;
            for ch in body.chars() {
                if count >= EXCERPT_CHARS {
                    e.push_str("…");
                    break;
                }
                e.push(ch);
                count += 1;
            }
            e
        } else {
            body.to_string()
        };

        // Heading titles only (no body), capped at 15
        let heading_titles: Vec<String> = body
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                t.starts_with("## ") || t.starts_with("### ") || t.starts_with("#### ")
            })
            .map(|l| l.trim_start_matches('#').trim().to_string())
            .take(15)
            .collect();

        let has_tables = body.lines().any(|l| l.trim_start().starts_with('|'));
        let has_examples = body.lines().any(|l| l.trim_start().starts_with("```"));
        let has_faqs = body.lines().any(|l| {
            let t = l.trim();
            t.starts_with("Q:") || t.starts_with("**Q:**")
        });

        summaries.push(RedirectSummary {
            file: file.to_string_lossy().to_string(),
            url: url.clone(),
            title,
            word_count,
            excerpt,
            headings: heading_titles,
            has_tables,
            has_examples,
            has_faqs,
        });
    }

    // Sort by word count (most content-rich first) and cap at 5 to stay within budget.
    summaries.sort_by(|a, b| b.word_count.cmp(&a.word_count));
    const MAX_REDIRECTS: usize = 5;
    let truncated = summaries.len() > MAX_REDIRECTS;
    summaries.truncate(MAX_REDIRECTS);

    let summary_values: Vec<serde_json::Value> = summaries
        .into_iter()
        .map(|s| {
            serde_json::json!({
                "file": s.file,
                "url": s.url,
                "title": s.title,
                "word_count": s.word_count,
                "excerpt": s.excerpt,
                "headings": s.headings,
                "has_tables": s.has_tables,
                "has_examples": s.has_examples,
                "has_faqs": s.has_faqs,
            })
        })
        .collect();

    let output_doc = serde_json::json!({
        "keeper_file": keeper_file.map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        "keeper_outline": keeper_outline,
        "keeper_excerpt": keeper_excerpt,
        "redirect_summaries": summary_values,
        "truncated": truncated,
    });

    let output_json = serde_json::to_string_pretty(&output_doc).unwrap_or_default();
    const MAX_EXTRACT_OUTPUT_BYTES: usize = 50_000;
    if output_json.len() > MAX_EXTRACT_OUTPUT_BYTES {
        return StepResult {
            success: false,
            message: format!(
                "Merge context too large ({} bytes) after extraction. \
                 The cluster has too many redirect pages to fit the prompt budget. \
                 Try splitting the cluster into smaller groups.",
                output_json.len()
            ),
            output: None,
        };
    }

    StepResult {
        success: true,
        message: format!(
            "Summarized {} redirect pages ({} bytes){}",
            summary_values.len(),
            output_json.len(),
            if truncated {
                " — truncated to top 5 by word count"
            } else {
                ""
            }
        ),
        output: Some(output_json),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 4: Draft Patch (agentic)
// ═══════════════════════════════════════════════════════════════════════════════

/// Agentic: draft a ContentMergePatch JSON that merges unique valuable content
/// from redirect pages into the keeper page.
///
/// Why not deterministic: merging overlapping articles requires editorial judgment
/// about which sections are redundant, which contain unique value worth preserving,
/// and where in the keeper's structure they best fit. A deterministic algorithm
/// cannot evaluate content quality, relevance, or narrative flow. The output is a
/// structured `ContentMergePatch` with precise insertion points, extracted via
/// Rig's `extract_structured`.
pub(crate) fn exec_merge_draft_patch(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: &str,
) -> StepResult {
    let repo_root = Path::new(project_path);

    let skill = match crate::engine::skills::load_skill_or_fail(repo_root, "merge-content") {
        Ok(s) => s,
        Err(msg) => {
            return StepResult::fail(msg);
        }
    };

    let prompt = skill.content
        + "\n\n---\n\n## Merge Context\n\n"
        + context_json
        + "\n\nPlease draft a ContentMergePatch JSON that merges the most valuable unique content from the redirect pages into the keeper."
        + "\n\nCRITICAL: Return ONLY a single JSON object matching the ContentMergePatch structure."
        + " Do not include markdown prose, summaries, or explanations outside the JSON.";

    const HARD_PROMPT_LIMIT_BYTES: usize = 20_000;
    let prompt_bytes = prompt.len();
    if prompt_bytes > HARD_PROMPT_LIMIT_BYTES {
        return StepResult {
            success: false,
            message: format!(
                "Merge prompt too large ({} bytes). Limit: {} bytes. \
                 The cluster has too much redirect content to fit the Kimi bridge limit. \
                 Consider splitting the cluster into smaller groups or running merge manually.",
                prompt_bytes, HARD_PROMPT_LIMIT_BYTES
            ),
            output: None,
        };
    }

    // Run the structured extractor inside a fresh runtime because this function
    // is called from within tokio::task::spawn_blocking.
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to create runtime for merge extraction: {}", e),
                output: None,
            };
        }
    };

    let extract_result = rt.block_on(async {
        crate::rig::extraction::extract_structured::<crate::models::merge_patch::ContentMergePatch>(
            agent_provider,
            &prompt,
            Some("You are an expert content editor. Draft a precise ContentMergePatch JSON."),
            Some("direct"),
            None,
        )
        .await
    });

    match extract_result {
        Ok(patch) => {
            let patch_json = match serde_json::to_string_pretty(&patch) {
                Ok(j) => j,
                Err(e) => {
                    return StepResult {
                        success: false,
                        message: format!("Failed to serialize merge patch: {}", e),
                        output: None,
                    };
                }
            };
            StepResult {
                success: true,
                message: format!(
                    "Merge patch drafted: {} additions, {} transitions",
                    patch.additions.len(),
                    patch.transitions.len()
                ),
                output: Some(patch_json),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Structured extraction failed for merge patch: {}", e),
            output: None,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 5: Apply Patch
// ═══════════════════════════════════════════════════════════════════════════════

/// Apply a ContentMergePatch to the keeper file, snapshotting the original.
pub(crate) fn exec_merge_apply_patch(
    _task: &Task,
    project_path: &str,
    patch_json: &str,
) -> StepResult {
    let patch: ContentMergePatch = match serde_json::from_str(patch_json) {
        Ok(p) => p,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid ContentMergePatch JSON: {}", e),
                output: None,
            };
        }
    };

    let keeper_path = Path::new(&patch.keeper_file);
    let keeper_path = if keeper_path.is_absolute() {
        keeper_path.to_path_buf()
    } else {
        Path::new(project_path).join(keeper_path)
    };

    if !keeper_path.exists() {
        return StepResult {
            success: false,
            message: format!("Keeper file not found: {}", keeper_path.display()),
            output: None,
        };
    }

    let original = match std::fs::read_to_string(&keeper_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to read keeper file: {}", e),
                output: None,
            };
        }
    };

    // Apply patch
    let mut modified = original.clone();

    // Apply transitions first
    for transition in &patch.transitions {
        modified = modified.replace(&transition.find, &transition.replace);
    }

    // Apply additions
    for addition in &patch.additions {
        let section_text = format!("\n\n## {}\n\n{}", addition.heading, addition.content);

        match addition.position.as_str() {
            pos if pos.starts_with("after:") => {
                let target = pos.strip_prefix("after:").unwrap_or("").trim();
                let pattern = format!("## {}", target);
                if let Some(idx) = modified.find(&pattern) {
                    // Find end of that section (next ## or EOF)
                    let rest = &modified[idx + pattern.len()..];
                    let next_heading = rest.find("\n## ").unwrap_or(rest.len());
                    let insert_pos = idx + pattern.len() + next_heading;
                    modified.insert_str(insert_pos, &section_text);
                } else {
                    modified.push_str(&section_text);
                }
            }
            pos if pos.starts_with("before:") => {
                let target = pos.strip_prefix("before:").unwrap_or("").trim();
                let pattern = format!("## {}", target);
                if let Some(idx) = modified.find(&pattern) {
                    modified.insert_str(idx, &section_text);
                } else {
                    modified.push_str(&section_text);
                }
            }
            _ => {
                // Default: append at end of body (before any Related Articles section if present)
                if let Some(idx) = modified.to_lowercase().find("\n## related articles") {
                    modified.insert_str(idx, &section_text);
                } else {
                    modified.push_str(&section_text);
                }
            }
        }
    }

    // Write modified file
    if let Err(e) = std::fs::write(&keeper_path, &modified) {
        return StepResult {
            success: false,
            message: format!("Failed to write modified keeper: {}", e),
            output: None,
        };
    }

    // Validate MDX structure
    let validation = crate::content::cleaner::validate_mdx_structure(&modified);

    let word_count = crate::content::ops::count_words(&modified);

    StepResult {
        success: validation.is_ok(),
        message: format!(
            "Patch applied: {} additions, {} transitions, {} words",
            patch.additions.len(),
            patch.transitions.len(),
            word_count,
        ),
        output: Some(
            serde_json::json!({
                "keeper_file": keeper_path.to_string_lossy().to_string(),
                "word_count": word_count,
                "validation_valid": validation.is_ok(),
                "validation_issues": validation.err().map(|e| vec![e]).unwrap_or_default(),
            })
            .to_string(),
        ),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 6: Generate Redirects
// ═══════════════════════════════════════════════════════════════════════════════

/// Generate redirect rules as generic CSV.
pub(crate) fn exec_merge_generate_redirects(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let plan_json = load_plan_from_task_or_file(task, project_path);
    let plan: serde_json::Value = match serde_json::from_str(&plan_json) {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid merge plan JSON: {}", e),
                output: None,
            };
        }
    };

    let keep_url = plan["keep_url"].as_str().unwrap_or("");
    let redirect_urls: Vec<String> = plan["redirect_urls"]
        .as_array()
        .unwrap_or(&vec![])
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    let rules: Vec<RedirectRule> = redirect_urls
        .iter()
        .map(|source| RedirectRule {
            source: source.clone(),
            destination: keep_url.to_string(),
            status: 301,
        })
        .collect();

    // Merge with existing redirects.csv (append, no duplicates)
    let csv_path = paths.automation_dir.join("redirects.csv");
    let mut existing_rules: std::collections::HashMap<String, (String, i32)> =
        std::collections::HashMap::new();

    if let Ok(existing) = std::fs::read_to_string(&csv_path) {
        for line in existing.lines().skip(1) {
            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() >= 3 {
                if let Ok(status) = parts[2].trim().parse::<i32>() {
                    existing_rules.insert(
                        parts[0].trim().to_string(),
                        (parts[1].trim().to_string(), status),
                    );
                }
            }
        }
    }

    for rule in &rules {
        existing_rules.insert(
            rule.source.clone(),
            (rule.destination.clone(), rule.status as i32),
        );
    }

    let mut csv = String::from("source,destination,status\n");
    for (source, (destination, status)) in &existing_rules {
        csv.push_str(&format!("{},{},{}\n", source, destination, status));
    }

    if let Err(e) = std::fs::write(&csv_path, &csv) {
        return StepResult {
            success: false,
            message: format!("Failed to write redirects.csv: {}", e),
            output: None,
        };
    }

    let output = serde_json::json!({
        "rules": rules,
        "csv_path": csv_path.to_string_lossy().to_string(),
        "count": rules.len(),
    });

    StepResult {
        success: true,
        message: format!(
            "Generated {} redirect rules -> {}",
            rules.len(),
            csv_path.display()
        ),
        output: Some(serde_json::to_string_pretty(&output).unwrap_or_default()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 7: Validate Output
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate the merged keeper and redirect map.
pub(crate) fn exec_merge_validate_output(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let plan_json = load_plan_from_task_or_file(task, project_path);
    let plan: serde_json::Value = match serde_json::from_str(&plan_json) {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid merge plan JSON: {}", e),
                output: None,
            };
        }
    };

    let keep_url = plan["keep_url"].as_str().unwrap_or("");
    let keeper_slug = keep_url
        .trim_start_matches("/blog/")
        .trim_start_matches('/');
    let keeper_file = find_file_by_slug(project_path, keeper_slug);

    let mut issues: Vec<String> = Vec::new();

    let keeper_valid = keeper_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|content| {
            let validation = crate::content::cleaner::validate_mdx_structure(&content);
            if let Err(e) = &validation {
                issues.push(format!("keeper: {}", e));
            }
            validation.is_ok()
        })
        .unwrap_or_else(|| {
            issues.push("Keeper file not found".to_string());
            false
        });

    let word_count = keeper_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|c| crate::content::ops::count_words(&c))
        .unwrap_or(0);

    let csv_path = paths.automation_dir.join("redirects.csv");
    let has_redirect_map = csv_path.exists();
    if !has_redirect_map {
        issues.push("No redirects.csv found".to_string());
    }

    let report = MergeValidationReport {
        keeper_valid,
        keeper_word_count: word_count,
        redirect_map_path: Some(csv_path.to_string_lossy().to_string()),
        issues: issues.clone(),
    };

    let all_ok = keeper_valid && has_redirect_map && issues.is_empty();

    StepResult {
        success: all_ok,
        message: if all_ok {
            "Merge validation passed".to_string()
        } else {
            format!("Merge validation found {} issues", issues.len())
        },
        output: Some(serde_json::to_string_pretty(&report).unwrap_or_default()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 8: Sync Articles
// ═══════════════════════════════════════════════════════════════════════════════

/// Sync merged content back to SQLite and articles.json.
pub(crate) fn exec_merge_sync_articles(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let repo_root = std::path::Path::new(project_path);

    let db_path = crate::db::default_db_path();
    let conn = match rusqlite::Connection::open(&db_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to open DB for sync: {}", e),
                output: None,
            };
        }
    };

    match crate::content::ops::sync_and_validate(
        &paths.automation_dir,
        repo_root,
        true, // apply_sync
        &conn,
        &task.project_id,
    ) {
        Ok(report) => StepResult {
            success: true,
            message: format!(
                "Synced {} checked entries, {} dates patched",
                report.checked_entries, report.dates_synced
            ),
            output: Some(
                serde_json::json!({
                    "checked_entries": report.checked_entries,
                    "content_files": report.content_files,
                    "orphan_files": report.orphan_files,
                    "dates_synced": report.dates_synced,
                })
                .to_string(),
            ),
        },
        Err(e) => StepResult {
            success: false,
            message: format!("Failed to sync merged articles: {}", e),
            output: None,
        },
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Load the merge plan JSON from the task artifact or automation directory.
fn load_plan_from_task_or_file(task: &Task, project_path: &str) -> String {
    let paths = ProjectPaths::from_path(project_path);

    // Extract cluster_id from task title
    let cluster_id = task
        .title
        .as_deref()
        .and_then(|t| t.strip_prefix("Merge cluster:"))
        .unwrap_or("")
        .trim();

    let strategy_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "cannibalization_strategy")
        .and_then(|a| a.content.clone())
        .unwrap_or_default();

    let strategy_json = if strategy_json.is_empty() {
        let path = paths.automation_dir.join("cannibalization_strategy.json");
        std::fs::read_to_string(&path).unwrap_or_default()
    } else {
        strategy_json
    };

    if strategy_json.is_empty() || cluster_id.is_empty() {
        return String::new();
    }

    let strategy: serde_json::Value = serde_json::from_str(&strategy_json).unwrap_or_default();
    let rec = strategy["merge_recommendations"]
        .as_array()
        .and_then(|arr| {
            arr.iter()
                .find(|r| r["cluster_id"].as_str().unwrap_or("") == cluster_id)
        });

    match rec {
        Some(r) => serde_json::to_string(r).unwrap_or_default(),
        None => String::new(),
    }
}

/// Find an MDX file in the content directory by its URL slug.
fn find_file_by_slug(project_path: &str, slug: &str) -> Option<PathBuf> {
    let repo_root = Path::new(project_path);
    let content_resolution = crate::content::locator::resolve(repo_root, None);
    let content_dir = content_resolution.selected?;

    let files = crate::content::locator::collect_markdown_files(&content_dir);

    // Normalize slug for matching: kebab-case and snake_case should match
    let slug_normalized = slug.replace('-', "_");

    for file in files {
        let name = file.file_stem()?.to_string_lossy().to_string();
        let name_normalized = name.replace('-', "_");

        // Slug might be in filename like "001_best_stocks_csp" → we look for the slug part
        if name == slug
            || name.ends_with(&format!("_{}", slug))
            || name.contains(slug)
            || name_normalized == slug_normalized
            || name_normalized.ends_with(&format!("_{}", slug_normalized))
            || name_normalized.contains(&slug_normalized)
        {
            return Some(file);
        }
        // Also check frontmatter for url_slug match
        if let Ok(content) = std::fs::read_to_string(&file) {
            let scalars = crate::content::frontmatter::top_level_scalars(&content);
            for field in scalars {
                let fm_slug = field.raw_value.trim_matches('"').trim_matches('\'');
                let fm_slug_normalized = fm_slug.replace('-', "_");
                if field.key == "url_slug"
                    && (fm_slug == slug || fm_slug_normalized == slug_normalized)
                {
                    return Some(file);
                }
            }
        }
    }

    None
}

/// Extract headings from markdown body.
#[allow(dead_code)]
fn extract_headings(body: &str) -> Vec<ExtractedHeading> {
    const MAX_BODY_LINES: usize = 30;
    let mut headings = Vec::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim_start();
        if line.starts_with("## ") || line.starts_with("### ") || line.starts_with("#### ") {
            let level = line.chars().take_while(|&c| c == '#').count() as u8;
            let text = line.trim_start_matches('#').trim().to_string();
            let mut body_lines = Vec::new();
            i += 1;
            while i < lines.len() && body_lines.len() < MAX_BODY_LINES {
                let next = lines[i].trim_start();
                if next.starts_with("## ") || next.starts_with("# ") {
                    break;
                }
                body_lines.push(lines[i]);
                i += 1;
            }
            // Skip remaining lines of this section if truncated
            while i < lines.len() {
                let next = lines[i].trim_start();
                if next.starts_with("## ") || next.starts_with("# ") {
                    break;
                }
                i += 1;
            }
            headings.push(ExtractedHeading {
                level,
                text,
                body: body_lines.join("\n"),
            });
            continue;
        }
        i += 1;
    }
    headings
}

/// Extract markdown tables from body.
#[allow(dead_code)]
fn extract_tables(body: &str) -> Vec<ExtractedTable> {
    let mut tables = Vec::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with('|') {
            let mut table_lines = Vec::new();
            while i < lines.len() && lines[i].trim_start().starts_with('|') {
                table_lines.push(lines[i]);
                i += 1;
            }
            let markdown = table_lines.join("\n");
            tables.push(ExtractedTable {
                caption: None,
                markdown,
            });
            continue;
        }
        i += 1;
    }
    tables
}

/// Extract code block examples from body.
#[allow(dead_code)]
fn extract_examples(body: &str) -> Vec<ExtractedExample> {
    const MAX_CODE_LINES: usize = 40;
    let mut examples = Vec::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].trim_start().starts_with("```") {
            let fence = lines[i].trim_start();
            let lang = fence
                .strip_prefix("```")
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            let mut code_lines = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                if code_lines.len() < MAX_CODE_LINES {
                    code_lines.push(lines[i]);
                }
                i += 1;
            }
            i += 1; // skip closing fence
            examples.push(ExtractedExample {
                caption: None,
                code: code_lines.join("\n"),
                language: lang,
            });
            continue;
        }
        i += 1;
    }
    examples
}

/// Extract FAQ-style Q&A from body (lines matching "Q:" / "A:" or "**Q:**" patterns).
#[allow(dead_code)]
fn extract_faqs(body: &str) -> Vec<ExtractedFaq> {
    const MAX_ANSWER_LINES: usize = 20;
    let mut faqs = Vec::new();
    let lines: Vec<&str> = body.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        let q_match =
            line.starts_with("Q:") || line.starts_with("**Q:**") || line.starts_with("**Q:**");
        if q_match {
            let question = line
                .strip_prefix("Q:")
                .or_else(|| line.strip_prefix("**Q:**"))
                .unwrap_or(line)
                .trim()
                .to_string();
            i += 1;
            let mut answer_lines = Vec::new();
            while i < lines.len() {
                let next = lines[i].trim();
                if next.starts_with("Q:")
                    || next.starts_with("**Q:**")
                    || next.starts_with("A:")
                    || next.starts_with("**A:**")
                {
                    break;
                }
                if answer_lines.len() < MAX_ANSWER_LINES {
                    answer_lines.push(lines[i]);
                }
                i += 1;
            }
            // Check if next line is "A:"
            if i < lines.len() {
                let a_line = lines[i].trim();
                if a_line.starts_with("A:") || a_line.starts_with("**A:**") {
                    if answer_lines.len() < MAX_ANSWER_LINES {
                        answer_lines.push(
                            a_line
                                .strip_prefix("A:")
                                .or_else(|| a_line.strip_prefix("**A:**"))
                                .unwrap_or(a_line)
                                .trim(),
                        );
                    }
                    i += 1;
                }
            }
            faqs.push(ExtractedFaq {
                question,
                answer: answer_lines.join("\n").trim().to_string(),
            });
            continue;
        }
        i += 1;
    }
    faqs
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_headings() {
        let body = r#"# Title

## Section One
Some text here.

### Subsection
More text.

## Section Two
Final text.
"#;
        let headings = extract_headings(body);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "Section One");
        assert_eq!(headings[0].level, 2);
        assert_eq!(headings[1].text, "Section Two");
    }

    #[test]
    fn test_extract_tables() {
        let body = r#"# Title

| Col A | Col B |
|-------|-------|
| 1     | 2     |

Some text.
"#;
        let tables = extract_tables(body);
        assert_eq!(tables.len(), 1);
        assert!(tables[0].markdown.contains("Col A"));
    }

    #[test]
    fn test_extract_examples() {
        let body = r#"# Title

```python
print("hello")
```

Some text.
"#;
        let examples = extract_examples(body);
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].language.as_deref(), Some("python"));
        assert!(examples[0].code.contains("hello"));
    }

    #[test]
    fn test_extract_faqs() {
        let body = r#"# Title

Q: What is this?
A: It is a test.

Q: Why?
A: Because.
"#;
        let faqs = extract_faqs(body);
        assert_eq!(faqs.len(), 2);
        assert_eq!(faqs[0].question, "What is this?");
        assert_eq!(faqs[0].answer, "It is a test.");
    }

    #[test]
    fn test_merge_preflight_report_roundtrip() {
        let report = MergePreflightReport {
            keeper_file_exists: true,
            keeper_is_indexable: true,
            redirect_files_exist: vec!["/blog/a".to_string()],
            redirect_files_missing: vec![],
            redirect_cycles_detected: vec![],
            can_proceed: true,
            notes: vec!["ok".to_string()],
        };
        let json = serde_json::to_string(&report).unwrap();
        let decoded: MergePreflightReport = serde_json::from_str(&json).unwrap();
        assert!(decoded.can_proceed);
    }
}
