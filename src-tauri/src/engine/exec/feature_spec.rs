/// Agentic feature specification generator.
///
/// Reads all available audit artifacts, synthesizes findings via an LLM,
/// and writes a prioritized developer feature spec to the automation directory.
use std::path::Path;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;

/// Agentic step: generate a comprehensive developer feature spec.
pub(crate) fn exec_generate_feature_spec(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let automation_dir = &paths.automation_dir;

    let mut sections: Vec<String> = Vec::new();

    if let Some(audit) = load_json(automation_dir.join("content_audit.json")) {
        sections.push(format_content_audit_section(&audit));
    }

    if let Some(ctr) = load_json(automation_dir.join("ctr_audit_context.json")) {
        sections.push(format_ctr_section(&ctr));
    }

    if let Some(clusters) = load_json(automation_dir.join("cannibalization_clusters.json")) {
        sections.push(format_cannibalization_section(&clusters));
    }

    if let Some(candidates) = load_json(automation_dir.join("cannibalization_candidates.json")) {
        sections.push(format_candidates_section(&candidates));
    }

    if let Some(plan) = load_json(automation_dir.join("indexing_campaign_plan.json")) {
        sections.push(format_indexing_section(&plan));
    }

    if let Some(templates) = load_json(automation_dir.join("ctr_template_detections.json")) {
        sections.push(format_template_section(&templates));
    }

    if sections.is_empty() {
        return StepResult {
            success: true,
            message: "No audit artifacts found — nothing to synthesize".to_string(),
            output: None,
        };
    }

    let combined = sections.join("\n\n---\n\n");

    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    let task_title = task.title.as_deref().unwrap_or("untitled");
    let task_id = &task.id;

    let prompt = format!(
        r#"You are a senior SEO technical lead writing a developer feature specification.

Your job: read the audit findings below, identify which issues require **code changes** (framework/template fixes), which require **content changes** (rewrites, merges), and which require **structural changes** (URL migrations, architecture decisions).

Write a markdown document with this exact structure. Start IMMEDIATELY with the # heading. Do NOT write any introduction, summary, or meta-commentary about what you are doing.

# SEO Feature Specification

Generated: {timestamp}
Triggered by: {task_title} ({task_id})

## Executive Summary
2-3 sentences on the most critical issue and its business impact.

## P0 — Code Changes Required (Developer)
Issues that can only be fixed by editing framework/template code (e.g., layout.tsx, _app.js, route handlers).

For each issue:
- **Problem**: one-line description
- **Evidence**: specific pages/data points from the audit
- **Root Cause**: why this is happening
- **Fix**: specific file(s) to edit and what to change
- **Estimated Effort**: small / medium / large

## P1 — Content Fixes (PageSeeds Can Handle)
Issues that the content fix pipeline can auto-fix or that writers can handle.

For each issue:
- **Problem**
- **Affected Pages**
- **Fix Action**

## P2 — Structural Changes (Architecture)
Issues requiring URL migrations, 301 redirects, or site architecture changes.

For each issue:
- **Problem**
- **Affected Pages**
- **Migration Plan**

## Issue Matrix
A table summarizing all issues: | Issue | Priority | Type | Count | Status |

---

## Audit Findings

{combined}

---

CRITICAL RULES:
- Be specific. Name exact files, exact slugs, exact titles.
- Do not invent data. Only use what's in the findings above.
- If an issue is clearly a framework/template bug (e.g., generic titles on many pages, duplicate brand names in template), mark it as P0 Code Change.
- If an issue is a content-level problem (e.g., thin content, missing keywords), mark it as P1 Content Fix.
- If an issue requires URL changes or redirects, mark it as P2 Structural.
- Your ENTIRE output must be the markdown document. No preamble like "Done" or "Here is the spec". No postamble. No mentions of file paths you "saved" to. No commentary about the generation process.
"#,
        timestamp = timestamp,
        task_title = task_title,
        task_id = task_id,
        combined = combined,
    );

    let raw_content = match crate::engine::agent::run_agent(
        agent_provider,
        &prompt,
        Path::new(project_path),
    ) {
        Ok(content) => content,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Agent failed to generate feature spec: {}", e),
                output: None,
            };
        }
    };

    // Strip common meta-preambles/postambles that LLMs sometimes emit
    let spec_content = strip_meta_commentary(&raw_content);

    // Use a unique filename per task so multiple specs don't clobber each other
    let spec_filename = format!("seo_feature_spec_{}.md", task.id);
    let spec_path = automation_dir.join(&spec_filename);
    if let Err(e) = std::fs::create_dir_all(automation_dir) {
        return StepResult {
            success: false,
            message: format!("Failed to create automation dir: {}", e),
            output: None,
        };
    }
    if let Err(e) = std::fs::write(&spec_path, &spec_content) {
        return StepResult {
            success: false,
            message: format!("Failed to write feature spec: {}", e),
            output: None,
        };
    }

    // Also write a stable symlink/latest reference for convenience
    let latest_path = automation_dir.join("seo_feature_spec.md");
    let _ = std::fs::remove_file(&latest_path);
    let _ = std::fs::hard_link(&spec_path, &latest_path);

    let word_count = crate::content::ops::count_words(&spec_content);

    StepResult {
        success: true,
        message: format!(
            "Feature spec generated ({} words) → {}",
            word_count,
            spec_path.display()
        ),
        output: Some(spec_path.to_string_lossy().to_string()),
    }
}

/// Strip common LLM meta-commentary that leaks outside the requested markdown format.
fn strip_meta_commentary(raw: &str) -> String {
    let mut cleaned = raw.trim().to_string();

    // Strip common preambles (case-insensitive, anchored to start)
    let preamble_patterns = [
        r"(?i)^\s*done\.\s*",
        r"(?i)^\s*ok\.\s*",
        r"(?i)^\s*alright\.\s*",
        r"(?i)^\s*here\s+is\s+(the\s+)?spec(ification)?[:\.]?\s*",
        r"(?i)^\s*here\s+is\s+(the\s+)?markdown\s+document[:\.]?\s*",
        r"(?i)^\s*i[''']?ve\s+written\s+(the\s+)?spec[:\.]?\s*",
        r"(?i)^\s*the\s+spec\s+has\s+been\s+written\s+to[:\.]?\s*",
        r"(?i)^\s*the\s+specification\s+is\s+below[:\.]?\s*",
        r"(?i)^\s*below\s+is\s+(the\s+)?spec[:\.]?\s*",
        r"(?i)^\s*generating\s+(the\s+)?spec[:\.]?\s*",
        r"(?i)^.*written\s+to\s+`[^`]+`\s*",
        r"(?i)^.*saved\s+to\s+`[^`]+`\s*",
    ];

    for pattern in &preamble_patterns {
        let re = regex::Regex::new(pattern).unwrap();
        cleaned = re.replace(&cleaned, "").to_string();
    }

    // Strip common postambles (case-insensitive, anchored to end)
    let postamble_patterns = [
        r"(?i)\s*let\s+me\s+know\s+if\s+you\s+need\s+anything\s+else[\.!]?\s*$",
        r"(?i)\s*feel\s+free\s+to\s+ask\s+if\s+you\s+need\s+changes[\.!]?\s*$",
        r"(?i)\s*this\s+spec\s+is\s+ready\s+for\s+implementation[\.!]?\s*$",
        r"(?i)\s*the\s+spec\s+has\s+been\s+saved[\.!]?\s*$",
        r"(?i)\s*saved\s+to\s+`[^`]+`\s*$",
        r"(?i)\s*written\s+to\s+`[^`]+`\s*$",
    ];

    for pattern in &postamble_patterns {
        let re = regex::Regex::new(pattern).unwrap();
        cleaned = re.replace(&cleaned, "").to_string();
    }

    // If the cleaned text doesn't start with #, try to find the first markdown heading
    let trimmed = cleaned.trim();
    if !trimmed.starts_with('#') {
        if let Some(pos) = trimmed.find("# SEO Feature Specification") {
            cleaned = trimmed[pos..].to_string();
        } else if let Some(pos) = trimmed.find('\n') {
            // If first line is not a heading, check if second line starts with #
            let after_first = &trimmed[pos + 1..].trim_start();
            if after_first.starts_with('#') {
                cleaned = after_first.to_string();
            }
        }
    }

    cleaned.trim().to_string()
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn load_json(path: std::path::PathBuf) -> Option<serde_json::Value> {
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn format_content_audit_section(audit: &serde_json::Value) -> String {
    let mut parts = vec!["## Content Audit Findings".to_string()];

    if let Some(articles) = audit["articles"].as_array() {
        let total = articles.len();
        let poor = articles.iter().filter(|a| a["health"].as_str() == Some("poor")).count();
        let needs = articles.iter().filter(|a| a["health"].as_str() == Some("needs_improvement")).count();
        parts.push(format!(
            "- Total articles audited: {}\n- Poor health: {}\n- Needs improvement: {}",
            total, poor, needs
        ));

        // Literal template variables
        let literal_vars: Vec<&serde_json::Value> = articles
            .iter()
            .filter(|a| a["checks"]["literal_template_variable"]["pass"].as_bool() == Some(false))
            .collect();
        if !literal_vars.is_empty() {
            parts.push(format!(
                "\n### Literal Template Variables ({} articles)\n",
                literal_vars.len()
            ));
            for a in &literal_vars {
                parts.push(format!(
                    "- `{}` → title: `{}`",
                    a["file"].as_str().unwrap_or("unknown"),
                    a["title"].as_str().unwrap_or("")
                ));
            }
        }

        // Temporal URLs
        let temporal: Vec<&serde_json::Value> = articles
            .iter()
            .filter(|a| a["checks"]["temporal_url"]["pass"].as_bool() == Some(false))
            .collect();
        if !temporal.is_empty() {
            parts.push(format!("\n### Temporal URLs ({} articles)\n", temporal.len()));
            for a in &temporal {
                parts.push(format!(
                    "- `{}` → `{}`",
                    a["url_slug"].as_str().unwrap_or(""),
                    a["file"].as_str().unwrap_or("unknown")
                ));
            }
        }

        // Title token duplication
        let dup_titles: Vec<&serde_json::Value> = articles
            .iter()
            .filter(|a| a["checks"]["title_token_duplication"]["pass"].as_bool() == Some(false))
            .collect();
        if !dup_titles.is_empty() {
            parts.push(format!("\n### Title Token Duplication ({} articles)\n", dup_titles.len()));
            for a in &dup_titles {
                parts.push(format!(
                    "- `{}` → title: `{}` (max token count: {})",
                    a["file"].as_str().unwrap_or("unknown"),
                    a["title"].as_str().unwrap_or(""),
                    a["title_token_max_count"].as_i64().unwrap_or(0)
                ));
            }
        }

        // Exact duplicates
        if let Some(dup_groups) = audit["duplicate_groups"].as_array() {
            if !dup_groups.is_empty() {
                parts.push(format!("\n### Exact Duplicate Content ({} groups)\n", dup_groups.len()));
                for g in dup_groups {
                    let ids = g["article_ids"].as_array().map(|a| {
                        a.iter().map(|id| id.as_i64().unwrap_or(0).to_string()).collect::<Vec<_>>().join(", ")
                    }).unwrap_or_default();
                    parts.push(format!("- Hash `{}` → articles: {}", g["hash"].as_str().unwrap_or(""), ids));
                }
            }
        }

        // Page bloat
        let bloated: Vec<&serde_json::Value> = articles
            .iter()
            .filter(|a| a["checks"]["page_bloat_proxy"]["pass"].as_bool() == Some(false))
            .collect();
        if !bloated.is_empty() {
            parts.push(format!("\n### Page Bloat ({} articles)\n", bloated.len()));
            for a in &bloated {
                parts.push(format!(
                    "- `{}` → file_size: {} bytes, images: {}, tables: {}, code_blocks: {}",
                    a["file"].as_str().unwrap_or("unknown"),
                    a["bloat_metrics"]["file_size"].as_u64().unwrap_or(0),
                    a["bloat_metrics"]["image_count"].as_u64().unwrap_or(0),
                    a["bloat_metrics"]["table_count"].as_u64().unwrap_or(0),
                    a["bloat_metrics"]["code_block_count"].as_u64().unwrap_or(0)
                ));
            }
        }
    }

    parts.join("\n")
}

fn format_ctr_section(ctr: &serde_json::Value) -> String {
    let mut parts = vec!["## CTR Audit Findings".to_string()];

    if let Some(articles) = ctr["articles"].as_array() {
        let template_issues: Vec<&serde_json::Value> = articles
            .iter()
            .filter(|a| {
                a["issues_detected"].as_array().map(|issues| {
                    issues.iter().any(|i| {
                        let s = i.as_str().unwrap_or("");
                        s.contains("template") || s.contains("duplicate") || s.contains("brand")
                    })
                }).unwrap_or(false)
            })
            .collect();
        if !template_issues.is_empty() {
            parts.push(format!("\n### CTR Template Issues ({} articles)\n", template_issues.len()));
            for a in &template_issues {
                parts.push(format!(
                    "- `{}` → rendered_title: `{}` | source_title: `{}` | issues: {:?}",
                    a["url_slug"].as_str().unwrap_or(""),
                    a["rendered_title"].as_str().unwrap_or(""),
                    a["source_title"].as_str().unwrap_or(""),
                    a["issues_detected"].as_array().map(|v| v.iter().filter_map(|i| i.as_str()).collect::<Vec<_>>()).unwrap_or_default()
                ));
            }
        }

        let low_ctr: Vec<&serde_json::Value> = articles
            .iter()
            .filter(|a| {
                let ctr_val = a["ctr"].as_f64().unwrap_or(0.0);
                let pos = a["avg_position"].as_f64().unwrap_or(0.0);
                ctr_val < 0.03 && pos >= 5.0 && pos <= 20.0
            })
            .collect();
        if !low_ctr.is_empty() {
            parts.push(format!("\n### Low CTR Opportunities ({} articles)\n", low_ctr.len()));
            for a in &low_ctr {
                parts.push(format!(
                    "- `{}` → position: {:.1}, CTR: {:.2}%, impressions: {}",
                    a["url_slug"].as_str().unwrap_or(""),
                    a["avg_position"].as_f64().unwrap_or(0.0),
                    a["ctr"].as_f64().unwrap_or(0.0) * 100.0,
                    a["impressions"].as_u64().unwrap_or(0)
                ));
            }
        }
    }

    parts.join("\n")
}

fn format_cannibalization_section(clusters: &serde_json::Value) -> String {
    let mut parts = vec!["## Cannibalization Clusters".to_string()];

    if let Some(items) = clusters["clusters"].as_array() {
        parts.push(format!("\n- Total clusters: {}\n", items.len()));
        for c in items {
            let size = c["article_ids"].as_array().map(|a| a.len()).unwrap_or(0);
            if size >= 2 {
                parts.push(format!(
                    "- Cluster `{}` ({} articles, similarity: {:.2}) → keyword: `{}`",
                    c["cluster_id"].as_str().unwrap_or(""),
                    size,
                    c["max_similarity"].as_f64().unwrap_or(0.0),
                    c["target_keyword"].as_str().unwrap_or("none")
                ));
            }
        }
    }

    parts.join("\n")
}

fn format_candidates_section(candidates: &serde_json::Value) -> String {
    let mut parts = vec!["## Cannibalization Merge Candidates".to_string()];

    if let Some(items) = candidates["candidates"].as_array() {
        parts.push(format!("\n- Total candidates: {}\n", items.len()));
        for c in items {
            let size = c["article_ids"].as_array().map(|a| a.len()).unwrap_or(0);
            if size >= 2 {
                parts.push(format!(
                    "- Candidate `{}` ({} articles) → action: `{}` | reason: `{}`",
                    c["theme"].as_str().unwrap_or(""),
                    size,
                    c["recommended_action"].as_str().unwrap_or("review"),
                    c["reason"].as_str().unwrap_or("")
                ));
            }
        }
    }

    parts.join("\n")
}

fn format_indexing_section(plan: &serde_json::Value) -> String {
    let mut parts = vec!["## Indexing Health Campaign".to_string()];

    if let Some(targets) = plan["targets"].as_array() {
        let not_indexed = targets.iter().filter(|t| t["reason_code"].as_str().unwrap_or("").starts_with("not_indexed")).count();
        let fix_content = targets.iter().filter(|t| t["recommended_action"].as_str() == Some("fix_content")).count();
        let add_links = targets.iter().filter(|t| t["recommended_action"].as_str() == Some("add_links")).count();
        let merge = targets.iter().filter(|t| t["recommended_action"].as_str() == Some("merge")).count();

        parts.push(format!(
            "\n- Not indexed targets: {}\n- Fix content: {}\n- Add links: {}\n- Merge: {}\n",
            not_indexed, fix_content, add_links, merge
        ));

        for t in targets.iter().filter(|t| t["recommended_action"].as_str() == Some("fix_content")) {
            parts.push(format!(
                "- `{}` → action: fix_content | reason: `{}` | word_count: {} | incoming_links: {}",
                t["url"].as_str().unwrap_or(""),
                t["reason_code"].as_str().unwrap_or(""),
                t["word_count"].as_u64().unwrap_or(0),
                t["incoming_links"].as_u64().unwrap_or(0)
            ));
        }
    }

    parts.join("\n")
}

fn format_template_section(templates: &serde_json::Value) -> String {
    let mut parts = vec!["## Title Template Detections".to_string()];

    if let Some(detections) = templates["detections"].as_array() {
        parts.push(format!("\n- Total detections: {}\n", detections.len()));
        for d in detections {
            parts.push(format!(
                "- Pattern: `{}` | affected: {} pages | example: `{}`",
                d["pattern"].as_str().unwrap_or(""),
                d["affected_count"].as_u64().unwrap_or(0),
                d["example_title"].as_str().unwrap_or("")
            ));
        }
    }

    // Missing dynamic title
    if let Some(missing) = templates["missing_dynamic_title"].as_array() {
        if !missing.is_empty() {
            parts.push(format!("\n### Missing Dynamic Title ({} pages)\n", missing.len()));
            for d in missing.iter().take(20) {
                parts.push(format!(
                    "- `{}` → rendered: `{}`",
                    d["url"].as_str().unwrap_or(""),
                    d["rendered_title"].as_str().unwrap_or("")
                ));
            }
        }
    }

    parts.join("\n")
}
