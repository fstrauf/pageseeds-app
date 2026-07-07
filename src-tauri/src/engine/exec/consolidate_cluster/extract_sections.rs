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

