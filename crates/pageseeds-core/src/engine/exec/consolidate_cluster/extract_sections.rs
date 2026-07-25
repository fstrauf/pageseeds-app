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
            return StepResult::fail(format!("Invalid merge plan JSON: {}", e));
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
            return StepResult::fail(e);
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
    let full_keeper_outline: Vec<OutlineHeading> = keeper_body
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("## ") || t.starts_with("### ") || t.starts_with("#### ")
        })
        .map(|l| {
            let level = l.trim_start().chars().take_while(|&c| c == '#').count() as u8;
            let text = l.trim_start_matches('#').trim().to_string();
            OutlineHeading { level, text }
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
    let keeper_heading_texts: std::collections::HashSet<String> = full_keeper_outline
        .iter()
        .map(|h| h.text.to_lowercase())
        .collect();
    // Cap the outline that goes into the merge context deterministically so it
    // cannot blow the per-round prompt budget. Coverage detection above uses
    // the full outline, so capping never changes which sections are kept.
    let keeper_outline = cap_keeper_outline(full_keeper_outline);

    let mut pages: Vec<RedirectPage> = Vec::new();

    for url in &redirect_urls {
        let slug = url.trim_start_matches("/blog/").trim_start_matches('/');
        let file = match find_file_by_slug(project_path, slug) {
            Ok(Some(p)) => p,
            Ok(None) => continue,
            Err(e) => {
                return StepResult::fail(e);
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

        let sections: Vec<MergeSection> = extract_headings(&body)
            .into_iter()
            .map(|h| {
                let covered = keeper_heading_texts.contains(&h.text.to_lowercase());
                MergeSection {
                    level: h.level,
                    text: h.text,
                    // Sections the keeper already covers get title-only; unique
                    // sections carry their full body so real content survives.
                    body: if covered { String::new() } else { h.body },
                    covered_by_keeper: covered,
                }
            })
            .collect();

        let tables: Vec<MergeTable> = extract_tables(&body)
            .into_iter()
            .map(|t| MergeTable { markdown: t.markdown })
            .collect();
        let examples: Vec<MergeExample> = extract_examples(&body)
            .into_iter()
            .map(|e| MergeExample { language: e.language, code: e.code })
            .collect();
        let faqs: Vec<MergeFaq> = extract_faqs(&body)
            .into_iter()
            .map(|f| MergeFaq { question: f.question, answer: f.answer })
            .collect();

        pages.push(RedirectPage {
            file: file.to_string_lossy().to_string(),
            url: url.clone(),
            title,
            word_count,
            sections,
            tables,
            examples,
            faqs,
            truncation_note: None,
        });
    }

    // Sort by word count (most content-rich first), then pack into batches.
    // Pages are never dropped: clusters with more redirect pages than fit one
    // prompt are processed in sequential draft→apply rounds against the keeper.
    //
    // The per-batch byte budget accounts for everything else that lands in the
    // merge prompt (skill content + keeper outline + keeper excerpt +
    // instruction tail), so a packed batch cannot exceed the shared prompt
    // budget once the draft step assembles the full prompt.
    let skill_bytes = crate::engine::skills::load_skill_or_fail(Path::new(project_path), "merge-content")
        .map(|s| s.content.len())
        .unwrap_or(FALLBACK_SKILL_BYTES_RESERVE);
    let keeper_outline_bytes = serde_json::to_string(&keeper_outline)
        .map(|s| s.len())
        .unwrap_or(0);
    let batch_byte_budget =
        merge_batch_byte_budget(skill_bytes, keeper_outline_bytes, keeper_excerpt.len());
    pages.sort_by(|a, b| b.word_count.cmp(&a.word_count));
    let batches: Vec<MergeBatch> = pack_redirect_batches(pages, batch_byte_budget)
        .into_iter()
        .enumerate()
        .map(|(i, redirect_pages)| MergeBatch { batch_index: i, redirect_pages })
        .collect();

    let total_redirects: usize = batches.iter().map(|b| b.redirect_pages.len()).sum();

    let context = MergeContext {
        keeper_file: keeper_file.map(|p| p.to_string_lossy().to_string()).unwrap_or_default(),
        keeper_outline,
        keeper_excerpt,
        total_redirects,
        batch_count: batches.len(),
        batches,
    };

    let output_json = serde_json::to_string_pretty(&context).unwrap_or_default();
    const MAX_EXTRACT_OUTPUT_BYTES: usize = 250_000;
    if output_json.len() > MAX_EXTRACT_OUTPUT_BYTES {
        return StepResult::fail(format!(
                "Merge context too large ({} bytes) after extraction. \
                 The cluster has too much redirect content even after batching. \
                 Try splitting the cluster into smaller groups.",
                output_json.len()
            ));
    }

    StepResult {
        success: true,
        message: format!(
            "Extracted unique sections from {} redirect pages in {} batch(es) ({} bytes)",
            total_redirects,
            context.batch_count,
            output_json.len(),
        ),
        output: Some(output_json),
        artifact_key: None,
    }
}

/// Maximum redirect pages per merge batch — bounds the number of draft rounds
/// independently of the byte budget.
pub(crate) const MAX_PAGES_PER_BATCH: usize = 5;

/// Maximum headings kept in the keeper outline sent to the agent.
const KEEPER_OUTLINE_MAX_ENTRIES: usize = 100;

/// Approximate byte cap for the serialized keeper outline (heading text plus
/// JSON overhead). Past this, remaining headings are dropped and a marker
/// heading records how many were omitted.
const KEEPER_OUTLINE_MAX_BYTES: usize = 4_000;

/// Reserve used for the merge-content skill size when it cannot be loaded
/// (the draft step will fail loudly in that case anyway).
const FALLBACK_SKILL_BYTES_RESERVE: usize = 8_000;

/// Margin for the merge-context JSON wrapper and the instruction tail the
/// draft step appends per round (see `draft_patch::assemble_merge_prompt`).
const PROMPT_ASSEMBLY_MARGIN_BYTES: usize = 1_024;

/// Floor for the per-batch byte budget. If the non-batch prompt overhead ever
/// grows past the target budget, batches still get a minimal budget so
/// packing terminates; the draft step's hard-budget guard stays the loud
/// last resort.
const MIN_BATCH_BYTE_BUDGET: usize = 4_000;

/// Truncate the keeper outline deterministically: keep the first headings up
/// to `KEEPER_OUTLINE_MAX_ENTRIES` entries and `KEEPER_OUTLINE_MAX_BYTES`
/// bytes, then append a marker heading recording the omission.
pub(crate) fn cap_keeper_outline(outline: Vec<OutlineHeading>) -> Vec<OutlineHeading> {
    let total = outline.len();
    let mut kept: Vec<OutlineHeading> = Vec::new();
    let mut bytes = 0usize;
    for h in outline {
        let h_bytes = h.text.len() + 16; // level + JSON field overhead
        if kept.len() >= KEEPER_OUTLINE_MAX_ENTRIES || bytes + h_bytes > KEEPER_OUTLINE_MAX_BYTES {
            break;
        }
        bytes += h_bytes;
        kept.push(h);
    }
    if kept.len() < total {
        kept.push(OutlineHeading {
            level: 2,
            text: format!(
                "[keeper outline truncated: {} more heading(s) omitted]",
                total - kept.len()
            ),
        });
    }
    kept
}

/// Byte budget for one batch's serialized redirect pages, derived from the
/// shared prompt budget minus everything else the merge prompt contains:
/// skill content + keeper outline + keeper excerpt + assembly margin.
pub(crate) fn merge_batch_byte_budget(
    skill_bytes: usize,
    keeper_outline_bytes: usize,
    keeper_excerpt_bytes: usize,
) -> usize {
    let overhead =
        skill_bytes + keeper_outline_bytes + keeper_excerpt_bytes + PROMPT_ASSEMBLY_MARGIN_BYTES;
    crate::config::prompt_budget::default_prompt_budget()
        .target
        .saturating_sub(overhead)
        .max(MIN_BATCH_BYTE_BUDGET)
}

/// Pack redirect pages into batches of at most `MAX_PAGES_PER_BATCH` pages and
/// at most `byte_budget` serialized bytes each. Every page lands in exactly
/// one batch — batching replaces the old top-5 truncation so clusters with
/// >5 redirect pages lose no content. A single page that alone exceeds the
/// budget is deterministically truncated (`fit_page_to_budget`) rather than
/// dropped or left to fail the draft step.
pub(crate) fn pack_redirect_batches(
    pages: Vec<RedirectPage>,
    byte_budget: usize,
) -> Vec<Vec<RedirectPage>> {
    let mut batches: Vec<Vec<RedirectPage>> = Vec::new();
    let mut current: Vec<RedirectPage> = Vec::new();
    let mut current_bytes = 0usize;

    for page in pages {
        let page = fit_page_to_budget(page, byte_budget);
        let page_bytes = serde_json::to_string(&page).map(|s| s.len()).unwrap_or(0);
        if !current.is_empty()
            && (current.len() >= MAX_PAGES_PER_BATCH || current_bytes + page_bytes > byte_budget)
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

/// Rows of table body kept verbatim when a table must be truncated (header
/// and separator rows are always kept).
const TRUNCATED_TABLE_KEEP_ROWS: usize = 10;

/// Smallest field still worth truncating — below this, further cutting saves
/// nothing meaningful and the draft step's hard guard stays the last resort.
const MIN_TRUNCATABLE_FIELD_BYTES: usize = 64;

/// Room reserved for the `truncation_note` the fitter appends after trimming,
/// so the note itself cannot push the page back over the budget.
const TRUNCATION_NOTE_RESERVE_BYTES: usize = 768;

/// Hard cap on the serialized truncation note — keeps the note (and thus the
/// page) inside the reserve above even when a page needed many cuts.
const MAX_TRUNCATION_NOTE_BYTES: usize = 700;

fn serialized_len(page: &RedirectPage) -> usize {
    serde_json::to_string(page).map(|s| s.len()).unwrap_or(0)
}

/// Deterministic last-resort reduction for a page that alone exceeds the
/// batch budget: cap tables to header + first rows, then trim the largest
/// section bodies / example code / FAQ answers until the page fits. Records
/// what was cut and why in `truncation_note` so the reduction is visible in
/// the merge context the agent receives.
fn fit_page_to_budget(mut page: RedirectPage, byte_budget: usize) -> RedirectPage {
    let original_bytes = serialized_len(&page);
    if original_bytes <= byte_budget {
        return page;
    }

    let mut notes: Vec<String> = Vec::new();
    // Trim against the budget minus the note reserve so the note appended
    // below cannot push the page back over `byte_budget`.
    let fit_budget = byte_budget.saturating_sub(TRUNCATION_NOTE_RESERVE_BYTES);

    // 1. Cap oversized tables: header + separator + first rows verbatim,
    //    then a summary marker.
    for table in &mut page.tables {
        let lines: Vec<&str> = table.markdown.lines().collect();
        let keep_lines = TRUNCATED_TABLE_KEEP_ROWS + 2; // header + separator + rows
        if lines.len() > keep_lines {
            let dropped = lines.len() - keep_lines;
            let kept = lines[..keep_lines].join("\n");
            table.markdown =
                format!("{}\n\n[…table truncated: {} row(s) omitted…]", kept, dropped);
            notes.push(format!("{} table row(s)", dropped));
        }
    }

    // 2. Trim the largest text fields until the page fits.
    while serialized_len(&page) > fit_budget {
        let overflow = serialized_len(&page) - fit_budget;
        // Find the largest truncatable field and cut it by the overflow.
        let max_len = page
            .sections
            .iter()
            .map(|s| s.body.len())
            .chain(page.examples.iter().map(|e| e.code.len()))
            .chain(page.faqs.iter().map(|f| f.answer.len()))
            .max()
            .unwrap_or(0);
        if max_len < MIN_TRUNCATABLE_FIELD_BYTES {
            break; // nothing left worth cutting — loud failure remains upstream
        }
        // Cut the overflow plus slack that exceeds the marker length, so every
        // iteration makes the page strictly smaller and the loop terminates.
        let cut = overflow + 256;
        let mut cut_field = |field: &mut String| -> bool {
            if field.len() == max_len {
                let keep = field.len().saturating_sub(cut);
                let mut boundary = keep;
                while boundary > 0 && !field.is_char_boundary(boundary) {
                    boundary -= 1;
                }
                let omitted = field.len() - boundary;
                field.truncate(boundary);
                field.push_str(&format!(
                    "\n\n[…truncated: {} bytes omitted to fit the merge prompt budget…]",
                    omitted
                ));
                true
            } else {
                false
            }
        };
        let mut done = false;
        for s in &mut page.sections {
            if cut_field(&mut s.body) {
                notes.push(format!("tail of section '{}'", s.text));
                done = true;
                break;
            }
        }
        if !done {
            for e in &mut page.examples {
                if cut_field(&mut e.code) {
                    notes.push("a code example".to_string());
                    done = true;
                    break;
                }
            }
        }
        if !done {
            for f in &mut page.faqs {
                if cut_field(&mut f.answer) {
                    notes.push(format!("the answer to '{}'", f.question));
                    done = true;
                    break;
                }
            }
        }
        if !done {
            break;
        }
    }

    let final_bytes = serialized_len(&page);
    let mut note = format!(
        "Page exceeded the {}-byte merge batch budget ({} bytes serialized); \
         truncated {} to fit (now {} bytes). Re-run the merge with smaller \
         clusters if the omitted content is needed.",
        byte_budget,
        original_bytes,
        notes.join(", "),
        final_bytes
    );
    // Cap the note so it always fits the reserve: section titles and FAQ
    // questions embedded in the cut list are unbounded.
    if note.len() > MAX_TRUNCATION_NOTE_BYTES {
        let mut boundary = MAX_TRUNCATION_NOTE_BYTES;
        while boundary > 0 && !note.is_char_boundary(boundary) {
            boundary -= 1;
        }
        note.truncate(boundary);
        note.push_str(" […further cuts omitted from note…]");
    }
    page.truncation_note = Some(note);
    page
}

