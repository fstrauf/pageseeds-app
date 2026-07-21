use std::path::{Path, PathBuf};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::merge_patch::{
    ContentMergePatch, ExtractedExample, ExtractedFaq, ExtractedHeading, ExtractedTable,
    MergePreflightReport, MergeValidationReport, RedirectRule, SectionInventory,
};
use crate::models::task::Task;

use super::*;
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
    let keeper_file = match find_file_by_slug(project_path, keeper_slug) {
        Ok(f) => f,
        Err(e) => {
            return StepResult {
                success: false,
                message: e,
                output: None,
            };
        }
    };
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

    // Extract full unique-section content for each redirect page.
    // The agent needs the actual body content of unique sections (tables, FAQs,
    // examples) to preserve it in the keeper — a short excerpt forces it to
    // write generative filler instead. Sections whose heading already exists in
    // the keeper are marked covered and their bodies are dropped to save budget.
    let keeper_heading_texts: std::collections::HashSet<String> = keeper_outline
        .iter()
        .filter_map(|h| h["text"].as_str().map(|t| t.to_lowercase()))
        .collect();

    let mut pages: Vec<serde_json::Value> = Vec::new();

    for url in &redirect_urls {
        let slug = url.trim_start_matches("/blog/").trim_start_matches('/');
        let file = match find_file_by_slug(project_path, slug) {
            Ok(Some(p)) => p,
            Ok(None) => continue,
            Err(e) => {
                return StepResult {
                    success: false,
                    message: e,
                    output: None,
                };
            }
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

        let sections: Vec<serde_json::Value> = extract_headings(&body)
            .into_iter()
            .map(|h| {
                let covered = keeper_heading_texts.contains(&h.text.to_lowercase());
                serde_json::json!({
                    "level": h.level,
                    "text": h.text,
                    // Sections the keeper already covers get title-only; unique
                    // sections carry their full body so real content survives.
                    "body": if covered { String::new() } else { h.body },
                    "covered_by_keeper": covered,
                })
            })
            .collect();

        let tables: Vec<serde_json::Value> = extract_tables(&body)
            .into_iter()
            .map(|t| serde_json::json!({"markdown": t.markdown}))
            .collect();
        let examples: Vec<serde_json::Value> = extract_examples(&body)
            .into_iter()
            .map(|e| serde_json::json!({"language": e.language, "code": e.code}))
            .collect();
        let faqs: Vec<serde_json::Value> = extract_faqs(&body)
            .into_iter()
            .map(|f| serde_json::json!({"question": f.question, "answer": f.answer}))
            .collect();

        pages.push(serde_json::json!({
            "file": file.to_string_lossy().to_string(),
            "url": url,
            "title": title,
            "word_count": word_count,
            "sections": sections,
            "tables": tables,
            "examples": examples,
            "faqs": faqs,
        }));
    }

    // Sort by word count (most content-rich first), then pack into batches.
    // Pages are never dropped: clusters with more redirect pages than fit one
    // prompt are processed in sequential draft→apply rounds against the keeper.
    pages.sort_by(|a, b| {
        let wc = |v: &serde_json::Value| v["word_count"].as_u64().unwrap_or(0);
        wc(b).cmp(&wc(a))
    });
    let batches = pack_redirect_batches(pages);

    let batch_values: Vec<serde_json::Value> = batches
        .into_iter()
        .enumerate()
        .map(|(i, pages)| serde_json::json!({"batch_index": i, "redirect_pages": pages}))
        .collect();

    let total_redirects: usize = batch_values
        .iter()
        .map(|b| b["redirect_pages"].as_array().map(|a| a.len()).unwrap_or(0))
        .sum();

    let output_doc = serde_json::json!({
        "keeper_file": keeper_file.map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        "keeper_outline": keeper_outline,
        "keeper_excerpt": keeper_excerpt,
        "total_redirects": total_redirects,
        "batch_count": batch_values.len(),
        "batches": batch_values,
    });

    let output_json = serde_json::to_string_pretty(&output_doc).unwrap_or_default();
    const MAX_EXTRACT_OUTPUT_BYTES: usize = 250_000;
    if output_json.len() > MAX_EXTRACT_OUTPUT_BYTES {
        return StepResult {
            success: false,
            message: format!(
                "Merge context too large ({} bytes) after extraction. \
                 The cluster has too much redirect content even after batching. \
                 Try splitting the cluster into smaller groups.",
                output_json.len()
            ),
            output: None,
        };
    }

    StepResult {
        success: true,
        message: format!(
            "Extracted unique sections from {} redirect pages in {} batch(es) ({} bytes)",
            total_redirects,
            output_doc["batch_count"],
            output_json.len(),
        ),
        output: Some(output_json),
    }
}

/// Maximum redirect pages per merge batch — matches the prompt budget the
/// `merge_draft_patch` step enforces per draft round.
pub(crate) const MAX_PAGES_PER_BATCH: usize = 5;

/// Byte budget for one batch's serialized redirect pages. Keeps the per-round
/// merge prompt (skill text + keeper context + one batch) under the hard
/// prompt limit enforced by `merge_draft_patch`.
pub(crate) const BATCH_BYTE_BUDGET: usize = 12_000;

/// Pack redirect pages into batches of at most `MAX_PAGES_PER_BATCH` pages and
/// at most `BATCH_BYTE_BUDGET` serialized bytes each. Every page lands in
/// exactly one batch — batching replaces the old top-5 truncation so clusters
/// with >5 redirect pages lose no content.
pub(crate) fn pack_redirect_batches(
    pages: Vec<serde_json::Value>,
) -> Vec<Vec<serde_json::Value>> {
    let mut batches: Vec<Vec<serde_json::Value>> = Vec::new();
    let mut current: Vec<serde_json::Value> = Vec::new();
    let mut current_bytes = 0usize;

    for page in pages {
        let page_bytes = serde_json::to_string(&page).map(|s| s.len()).unwrap_or(0);
        if !current.is_empty()
            && (current.len() >= MAX_PAGES_PER_BATCH || current_bytes + page_bytes > BATCH_BYTE_BUDGET)
        {
            batches.push(std::mem::take(&mut current));
            current_bytes = 0;
        }
        current_bytes += page_bytes;
        current.push(page);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

