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
use crate::engine::{agent, skills};
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

    let mut inventories: Vec<SectionInventory> = Vec::new();

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
        let _ = frontmatter;

        let headings = extract_headings(body);
        let tables = extract_tables(body);
        let examples = extract_examples(body);
        let faqs = extract_faqs(body);

        inventories.push(SectionInventory {
            file: file.to_string_lossy().to_string(),
            headings,
            tables,
            examples,
            faqs,
        });
    }

    let output_doc = serde_json::json!({
        "keeper_file": keeper_file.map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        "keeper_content": keeper_content,
        "redirect_inventories": inventories,
    });

    StepResult {
        success: true,
        message: format!(
            "Extracted sections from {} redirect pages",
            inventories.len()
        ),
        output: Some(serde_json::to_string_pretty(&output_doc).unwrap_or_default()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 4: Draft Patch (agentic)
// ═══════════════════════════════════════════════════════════════════════════════

/// Agentic step: draft a ContentMergePatch JSON.
pub(crate) fn exec_merge_draft_patch(
    _task: &Task,
    project_path: &str,
    agent_provider: &str,
    context_json: &str,
) -> StepResult {
    let repo_root = Path::new(project_path);

    let skill = match skills::load_skill(repo_root, "merge-content") {
        Some(s) => s,
        None => {
            return StepResult {
                success: false,
                message: "Skill 'merge-content' not found".to_string(),
                output: None,
            };
        }
    };

    let prompt = skill.content
        + "\n\n---\n\n## Merge Context\n\n"
        + context_json
        + "\n\nPlease draft a ContentMergePatch JSON that merges the most valuable unique content from the redirect pages into the keeper."
        + "\n\nCRITICAL: Return ONLY a single JSON object matching the ContentMergePatch structure."
        + " Do not include markdown prose, summaries, or explanations outside the JSON.";

    match agent::run_agent(agent_provider, &prompt, repo_root) {
        Ok(output) => {
            let final_output = crate::engine::text::extract_json(&output)
                .and_then(|v| serde_json::to_string_pretty(&v).ok())
                .unwrap_or(output);
            StepResult {
                success: true,
                message: "Merge patch drafted".to_string(),
                output: Some(final_output),
            }
        }
        Err(e) => StepResult {
            success: false,
            message: format!("Agent error during merge patch draft: {}", e),
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

    // Snapshot original
    let snapshot_dir = keeper_path
        .parent()
        .map(|p| p.join(".snapshots"))
        .unwrap_or_default();
    let _ = std::fs::create_dir_all(&snapshot_dir);
    let snapshot_path = snapshot_dir.join(
        keeper_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
            + ".backup",
    );
    if let Err(e) = std::fs::write(&snapshot_path, &original) {
        log::warn!("[consolidate_cluster] Failed to write snapshot: {}", e);
    }

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

    let word_count = modified.split_whitespace().count();

    StepResult {
        success: validation.is_ok(),
        message: format!(
            "Patch applied: {} additions, {} transitions, {} words, snapshot at {}",
            patch.additions.len(),
            patch.transitions.len(),
            word_count,
            snapshot_path.display(),
        ),
        output: Some(
            serde_json::json!({
                "keeper_file": keeper_path.to_string_lossy().to_string(),
                "snapshot_path": snapshot_path.to_string_lossy().to_string(),
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

    // Write generic CSV
    let csv_path = paths.automation_dir.join("redirects.csv");
    let mut csv = String::from("source,destination,status\n");
    for rule in &rules {
        csv.push_str(&format!(
            "{},{},{}\n",
            rule.source, rule.destination, rule.status
        ));
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
        .map(|c| c.split_whitespace().count())
        .unwrap_or(0);

    let snapshot_dir = keeper_file
        .as_ref()
        .and_then(|p| p.parent())
        .map(|p| p.join(".snapshots"));
    let snapshot_path = snapshot_dir.as_ref().map(|d| {
        d.join(
            keeper_file
                .as_ref()
                .unwrap()
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string()
                + ".backup",
        )
    });
    let has_snapshot = snapshot_path.as_ref().map(|p| p.exists()).unwrap_or(false);
    if !has_snapshot {
        issues.push("No snapshot found for keeper file".to_string());
    }

    let csv_path = paths.automation_dir.join("redirects.csv");
    let has_redirect_map = csv_path.exists();
    if !has_redirect_map {
        issues.push("No redirects.csv found".to_string());
    }

    let report = MergeValidationReport {
        keeper_valid,
        keeper_word_count: word_count,
        snapshot_path: snapshot_path.map(|p| p.to_string_lossy().to_string()),
        redirect_map_path: Some(csv_path.to_string_lossy().to_string()),
        issues: issues.clone(),
    };

    let all_ok = keeper_valid && has_snapshot && has_redirect_map && issues.is_empty();

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

    for file in files {
        let name = file.file_stem()?.to_string_lossy().to_string();
        // Slug might be in filename like "001_best_stocks_csp" → we look for the slug part
        if name == slug || name.ends_with(&format!("_{}", slug)) || name.contains(slug) {
            return Some(file);
        }
        // Also check frontmatter for url_slug match
        if let Ok(content) = std::fs::read_to_string(&file) {
            let scalars = crate::content::frontmatter::top_level_scalars(&content);
            for field in scalars {
                if field.key == "url_slug"
                    && field.raw_value.trim_matches('"').trim_matches('\'') == slug
                {
                    return Some(file);
                }
            }
        }
    }

    None
}

/// Extract headings from markdown body.
fn extract_headings(body: &str) -> Vec<ExtractedHeading> {
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
            while i < lines.len() {
                let next = lines[i].trim_start();
                if next.starts_with("## ") || next.starts_with("# ") {
                    break;
                }
                body_lines.push(lines[i]);
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
fn extract_examples(body: &str) -> Vec<ExtractedExample> {
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
                code_lines.push(lines[i]);
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
fn extract_faqs(body: &str) -> Vec<ExtractedFaq> {
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
                answer_lines.push(lines[i]);
                i += 1;
            }
            // Check if next line is "A:"
            if i < lines.len() {
                let a_line = lines[i].trim();
                if a_line.starts_with("A:") || a_line.starts_with("**A:**") {
                    answer_lines.push(
                        a_line
                            .strip_prefix("A:")
                            .or_else(|| a_line.strip_prefix("**A:**"))
                            .unwrap_or(a_line)
                            .trim(),
                    );
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
