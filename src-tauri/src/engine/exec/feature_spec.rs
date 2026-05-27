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

    // Load content audit from DB (new primary storage)
    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(conn) => conn,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to open app database: {}", e),
                output: None,
            };
        }
    };

    let audit_json = crate::db::content_audit::get_audit_report_as_json(&db, &task.project_id)
        .ok()
        .flatten();

    if let Some(ref audit) = audit_json {
        sections.push(format_content_audit_section(audit));
    } else if let Some(audit) = load_json(automation_dir.join("content_audit.json")) {
        // Fallback to legacy JSON file during transition
        let sv = audit["schema_version"].as_u64().unwrap_or(0);
        if sv < 2 {
            sections.push(format!(
                "> **Note:** The content audit data was generated with schema v{} (current: v2). Some extended checks (temporal_url, page_bloat_proxy, literal_template_variable, title_token_duplication) are not available. Baseline check data will be used instead.",
                sv
            ));
        }
        sections.push(format_content_audit_section(&audit));
    }

    let ctr_json = crate::db::content_audit::get_latest_audit_artifact(&db, &task.project_id, "ctr_audit_context")
        .ok()
        .flatten()
        .or_else(|| load_json(automation_dir.join("ctr_audit_context.json")));
    if let Some(ref ctr) = ctr_json {
        sections.push(format_ctr_section(ctr));
    }

    let clusters_json = crate::db::content_audit::get_latest_audit_artifact(&db, &task.project_id, "cannibalization_clusters")
        .ok()
        .flatten()
        .or_else(|| load_json(automation_dir.join("cannibalization_clusters.json")));
    if let Some(ref clusters) = clusters_json {
        sections.push(format_cannibalization_section(clusters));
    }

    let candidates_json = crate::db::content_audit::get_latest_audit_artifact(&db, &task.project_id, "cannibalization_candidates")
        .ok()
        .flatten()
        .or_else(|| load_json(automation_dir.join("cannibalization_candidates.json")));
    if let Some(ref candidates) = candidates_json {
        sections.push(format_candidates_section(candidates));
    }

    let plan_json = crate::db::content_audit::get_latest_audit_artifact(&db, &task.project_id, "indexing_campaign_plan")
        .ok()
        .flatten()
        .or_else(|| load_json(automation_dir.join("indexing_campaign_plan.json")));
    if let Some(ref plan) = plan_json {
        sections.push(format_indexing_section(plan));
    }

    let templates_json = crate::db::content_audit::get_latest_audit_artifact(&db, &task.project_id, "ctr_template_detections")
        .ok()
        .flatten()
        .or_else(|| load_json(automation_dir.join("ctr_template_detections.json")));
    if let Some(ref templates) = templates_json {
        sections.push(format_template_section(templates));
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

    // Detect actual tech stack so the LLM doesn't assume Next.js
    let tech_stack = crate::content::ops::detect_tech_stack(Path::new(project_path));

    let prompt = format!(
        r#"You are a senior SEO technical lead writing a developer feature specification.

DETECTED TECH STACK: {tech_stack}
You MUST NOT assume a different framework. Do not mention files or patterns that belong to other frameworks (e.g., do not mention `next.config.js` or `getStaticProps` unless the stack is actually Next.js).

Your job: read the audit findings below, identify which issues require **code changes** (framework/template fixes), which require **content changes** (rewrites, merges), and which require **structural changes** (URL migrations, architecture decisions).

Write a markdown document with this exact structure. Start IMMEDIATELY with the # heading. Do NOT write any introduction, summary, or meta-commentary about what you are doing.

# SEO Feature Specification

Generated: {timestamp}
Triggered by: {task_title} ({task_id})

## Executive Summary
2-3 sentences on the most critical issue and its business impact.

## P0 — Code Changes Required (Developer)
Issues that can only be fixed by editing framework/template code (layout files, global components, route handlers, SSR config).

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
- A URL with "word_count: 0" and "NOT TRACKED IN CONTENT DIRECTORY" means the page exists in search console but has no corresponding MDX file in the repo — do NOT invent a rendering bug. Flag it as a structural/orphan issue.
- Your ENTIRE output must be the markdown document. No preamble like "Done" or "Here is the spec". No postamble. No mentions of file paths you "saved" to. No commentary about the generation process.
"#,
        tech_stack = tech_stack,
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

    // Strip common meta-preambles/postambles that LLMs sometimes emit and extract
    // the actual markdown document from the raw agent output.
    let spec_content = crate::engine::text::extract_markdown_document(
        &raw_content,
        Some("# SEO Feature Specification"),
    )
    .unwrap_or_default();

    // Validate that we got an actual markdown spec, not LLM commentary
    let validation = validate_spec_content(&spec_content);
    if let Err(reason) = validation {
        // Write the raw output to a debug file for inspection, but fail the step
        let debug_path = automation_dir.join(format!("seo_feature_spec_{}_raw.md", task.id));
        let _ = std::fs::create_dir_all(automation_dir);
        let _ = std::fs::write(&debug_path, &raw_content);
        return StepResult {
            success: false,
            message: format!(
                "LLM output was not a valid feature spec: {}. Raw output saved to {} for inspection.",
                reason,
                debug_path.display()
            ),
            output: None,
        };
    }

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

/// Validate that the cleaned LLM output is an actual feature spec, not commentary.
fn validate_spec_content(content: &str) -> Result<(), &'static str> {
    let trimmed = content.trim();

    if trimmed.is_empty() {
        return Err("output is empty");
    }

    // Must start with a markdown heading
    if !trimmed.starts_with('#') {
        return Err("output does not start with a markdown heading (#)");
    }

    // Must contain the expected top-level heading
    if !trimmed.contains("# SEO Feature Specification") {
        return Err("output is missing '# SEO Feature Specification' heading");
    }

    // Must contain at least one priority section — LLMs that output only commentary
    // never include these structured sections.
    let has_priority_section =
        trimmed.contains("P0") || trimmed.contains("P1") || trimmed.contains("P2");
    if !has_priority_section {
        return Err("output is missing priority sections (P0/P1/P2)");
    }

    // Must be reasonably substantial — a real spec is >200 words. Commentary/summaries
    // are typically under 100 words.
    let word_count = crate::content::ops::count_words(trimmed);
    if word_count < 100 {
        return Err("output is too short to be a valid feature spec (<100 words)");
    }

    Ok(())
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

        // Detect which check keys are present in the audit data so we don't silently
        // produce empty sections when the audit was run with an older version that
        // doesn't include extended checks (temporal_url, page_bloat_proxy, etc.).
        let available_checks: std::collections::HashSet<&str> = articles
            .first()
            .and_then(|a| a["checks"].as_object())
            .map(|o| o.keys().map(|k| k.as_str()).collect())
            .unwrap_or_default();

        let has_check = |key: &str| available_checks.contains(key);

        // Literal template variables
        if has_check("literal_template_variable") {
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
        }

        // Temporal URLs
        if has_check("temporal_url") {
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
        }

        // Title token duplication
        if has_check("title_token_duplication") {
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
        }

        // Exact duplicates
        if let Some(dup_groups) = audit["duplicate_groups"].as_array() {
            if !dup_groups.is_empty() {
                parts.push(format!("\n### Exact Duplicate Content ({} groups)\n", dup_groups.len()));
                for g in dup_groups {
                    let article_lines: Vec<String> = g["articles"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .map(|art| {
                                    let id = art["id"].as_i64().unwrap_or(0);
                                    let title = art["title"].as_str().unwrap_or("");
                                    let file = art["file"].as_str().unwrap_or("");
                                    format!("  - [{}] `{}` ({})", id, title, file)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    parts.push(format!(
                        "- Hash `{}` → {} articles:\n{}",
                        g["hash"].as_str().unwrap_or(""),
                        article_lines.len(),
                        article_lines.join("\n")
                    ));
                }
            }
        }

        // Page bloat
        if has_check("page_bloat_proxy") {
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

        // Fallback: when the extended check keys are absent (audit was run with an
        // older version), extract actionable baseline check failures so the LLM has
        // real data to work with instead of hallucinating categories.
        let has_extended_checks = has_check("temporal_url")
            && has_check("page_bloat_proxy")
            && has_check("literal_template_variable")
            && has_check("title_token_duplication");
        if !has_extended_checks && (poor > 0 || needs > 0) {
            parts.push(format_baseline_audit_failures(articles));
        }
    }

    parts.join("\n")
}

/// Extract actionable failure categories from baseline content audit checks
/// (broken_links, word_count, keyword_density, etc.) when extended checks
/// (temporal_url, page_bloat_proxy, etc.) are not available.
fn format_baseline_audit_failures(articles: &[serde_json::Value]) -> String {
    let mut parts = vec!["\n### Content Issues (baseline checks)\n".to_string()];

    // P0-like: broken links, malformed links, missing source files
    let broken_links: Vec<_> = articles
        .iter()
        .filter(|a| a["checks"]["broken_links"]["pass"].as_bool() == Some(false))
        .collect();
    if !broken_links.is_empty() {
        parts.push(format!(
            "#### Broken/Placeholder Links ({} articles)\n",
            broken_links.len()
        ));
        for a in broken_links.iter().take(15) {
            parts.push(format!(
                "- `{}` → {} broken links",
                a["file"].as_str().unwrap_or("unknown"),
                a["checks"]["broken_links"]["value"].as_u64().unwrap_or(0)
            ));
        }
    }

    let malformed: Vec<_> = articles
        .iter()
        .filter(|a| a["checks"]["malformed_links"]["pass"].as_bool() == Some(false))
        .collect();
    if !malformed.is_empty() {
        parts.push(format!(
            "\n#### Malformed Markdown Links ({} articles)\n",
            malformed.len()
        ));
        for a in malformed.iter().take(15) {
            parts.push(format!(
                "- `{}` | title: `{}`",
                a["file"].as_str().unwrap_or("unknown"),
                a["title"].as_str().unwrap_or("")
            ));
        }
    }

    // P1-like: thin content, keyword issues, missing metadata
    let thin: Vec<_> = articles
        .iter()
        .filter(|a| a["checks"]["word_count"]["pass"].as_bool() == Some(false))
        .collect();
    if !thin.is_empty() {
        parts.push(format!(
            "\n#### Thin Content — Below 800 Words ({} articles)\n",
            thin.len()
        ));
        for a in thin.iter().take(15) {
            parts.push(format!(
                "- `{}` → {} words | title: `{}`",
                a["file"].as_str().unwrap_or("unknown"),
                a["checks"]["word_count"]["value"].as_u64().unwrap_or(0),
                a["title"].as_str().unwrap_or("")
            ));
        }
    }

    let missing_keywords: Vec<_> = articles
        .iter()
        .filter(|a| {
            a["checks"]["title_keyword"]["pass"].as_bool() == Some(false)
                || a["checks"]["h1_keyword"]["pass"].as_bool() == Some(false)
        })
        .collect();
    if !missing_keywords.is_empty() {
        parts.push(format!(
            "\n#### Keyword Missing from Title or H1 ({} articles)\n",
            missing_keywords.len()
        ));
        for a in missing_keywords.iter().take(15) {
            let kw = a["target_keyword"].as_str().unwrap_or("?");
            let title_ok = a["checks"]["title_keyword"]["pass"].as_bool() == Some(true);
            let h1_ok = a["checks"]["h1_keyword"]["pass"].as_bool() == Some(true);
            let missing = if !title_ok && !h1_ok {
                "title and H1"
            } else if !title_ok {
                "title"
            } else {
                "H1"
            };
            parts.push(format!(
                "- `{}` → keyword `{}` missing from {}",
                a["file"].as_str().unwrap_or("unknown"),
                kw,
                missing
            ));
        }
    }

    let missing_meta: Vec<_> = articles
        .iter()
        .filter(|a| {
            a["checks"]["meta_desc_present"]["pass"].as_bool() == Some(false)
                || a["checks"]["meta_desc_length"]["pass"].as_bool() == Some(false)
        })
        .collect();
    if !missing_meta.is_empty() {
        parts.push(format!(
            "\n#### Missing or Invalid Meta Description ({} articles)\n",
            missing_meta.len()
        ));
        for a in missing_meta.iter().take(10) {
            let absent = a["checks"]["meta_desc_present"]["pass"].as_bool() == Some(false);
            parts.push(format!(
                "- `{}` → {}",
                a["file"].as_str().unwrap_or("unknown"),
                if absent {
                    "no meta description"
                } else {
                    "meta description wrong length"
                }
            ));
        }
    }

    // P2-like: insufficient internal links
    let low_links: Vec<_> = articles
        .iter()
        .filter(|a| a["checks"]["internal_links"]["pass"].as_bool() == Some(false))
        .collect();
    if !low_links.is_empty() {
        parts.push(format!(
            "\n#### Insufficient Internal Links — Less Than 3 ({} articles)\n",
            low_links.len()
        ));
        for a in low_links.iter().take(15) {
            parts.push(format!(
                "- `{}` → {} internal links | title: `{}`",
                a["file"].as_str().unwrap_or("unknown"),
                a["checks"]["internal_links"]["value"].as_u64().unwrap_or(0),
                a["title"].as_str().unwrap_or("")
            ));
        }
    }

    // Top offenders summary
    let poor_articles: Vec<_> = articles
        .iter()
        .filter(|a| a["health"].as_str() == Some("poor"))
        .take(10)
        .collect();
    if !poor_articles.is_empty() {
        parts.push(format!(
            "\n#### Top Priority Articles (poor health, highest priority)\n"
        ));
        for a in &poor_articles {
            let failed = a["checks_failed"].as_u64().unwrap_or(0);
            let total_checks = a["checks_total"].as_u64().unwrap_or(0);
            parts.push(format!(
                "- `{}` → priority_score: {}, health: poor, failed {}/{} checks | title: `{}`",
                a["file"].as_str().unwrap_or("unknown"),
                a["priority_score"].as_i64().unwrap_or(0),
                failed,
                total_checks,
                a["title"].as_str().unwrap_or("")
            ));
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
            let file = t["file"].as_str().filter(|s| !s.is_empty());
            let wc = t["word_count"].as_u64().unwrap_or(0);
            let links = t["incoming_links"].as_u64().unwrap_or(0);
            let tracked_note = if file.is_some() {
                format!(" | file: `{}`", file.unwrap())
            } else {
                " | NOT TRACKED IN CONTENT DIRECTORY".to_string()
            };
            parts.push(format!(
                "- `{}` → action: fix_content | reason: `{}` | word_count: {} | incoming_links: {}{}",
                t["url"].as_str().unwrap_or(""),
                t["reason_code"].as_str().unwrap_or(""),
                wc,
                links,
                tracked_note
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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_spec_content_valid() {
        let content = "# SEO Feature Specification\n\nGenerated: 2024-01-01 00:00 UTC\nTriggered by: test (task-id)\n\n## Executive Summary\nThis is a comprehensive summary of the most critical issues and their business impact. We have identified several problems that require immediate attention from the development team.\n\n## P0 — Code Changes Required (Developer)\nIssues that can only be fixed by editing framework or template code.\n\n### Problem: Template rendering failure\n**Evidence**: Multiple pages show generic titles.\n**Root Cause**: Layout component overrides page titles.\n**Fix**: Edit layout.tsx to respect page-level title metadata.\n**Estimated Effort**: small\n\n## P1 — Content Fixes (PageSeeds Can Handle)\nIssues that the content fix pipeline can auto-fix.\n\n### Problem: Thin content\n**Affected Pages**: /blog/post-1, /blog/post-2\n**Fix Action**: Expand articles to minimum 500 words.\n\n## P2 — Structural Changes (Architecture)\nIssues requiring URL migrations or architecture changes.\n\n### Problem: Orphaned pages\n**Affected Pages**: /old-page-1\n**Migration Plan**: Add internal links from related articles.\n\n## Issue Matrix\n| Issue | Priority | Type | Count | Status |\n|-------|----------|------|-------|--------|\n| Template failure | P0 | Code | 5 | Open |\n";
        assert!(validate_spec_content(content).is_ok());
    }

    #[test]
    fn test_validate_spec_content_empty() {
        assert!(validate_spec_content("").is_err());
    }

    #[test]
    fn test_validate_spec_content_no_heading() {
        assert!(validate_spec_content("Some random text without a heading").is_err());
    }

    #[test]
    fn test_validate_spec_content_missing_priority_sections() {
        let content = "# SEO Feature Specification\n\n## Executive Summary\n\n## Issue Matrix\n";
        assert!(validate_spec_content(content).is_err());
    }

    #[test]
    fn test_validate_spec_content_too_short() {
        let content = "# SEO Feature Specification\n\n## P0\n\nfix it\n";
        assert!(validate_spec_content(content).is_err());
    }

    #[test]
    fn test_validate_spec_content_meta_commentary_fails() {
        // This is the exact pattern that was causing the bug — the shared
        // extract_markdown_document returns None/empty for pure commentary,
        // which validate_spec_content then rejects.
        let content = "Done. The spec has been written to:\n\n`docs/SEO_FEATURE_SPEC_indexing_health_campaign.md`\n\nIt contains:\n- **2 P0 code issues**\n- **3 P1 content issues**\n";
        assert!(validate_spec_content(content).is_err());
    }

    #[test]
    fn test_integration_with_shared_extraction() {
        // Verify that the shared extraction + local validation work together.
        let raw = "Done. Here is the spec:\n\n# SEO Feature Specification\n\n## Executive Summary\nThis is a comprehensive summary of the most critical issues and their business impact. We have identified several problems that require immediate attention from the development team. The issues span code changes, content fixes, and structural improvements. Each category has been prioritized based on severity and expected business impact. Addressing these will significantly improve search visibility and user experience.\n\n## P0 — Code Changes Required\n### Problem: Template rendering failure\n**Evidence**: Multiple pages show generic titles instead of unique ones.\n**Root Cause**: Layout component overrides page-level title metadata.\n**Fix**: Edit layout.tsx to respect page-level title metadata.\n**Estimated Effort**: small\n\n## P1 — Content Fixes\n### Problem: Thin content\n**Affected Pages**: /blog/post-1, /blog/post-2\n**Fix Action**: Expand articles to minimum 500 words.\n\n## P2 — Structural Changes\n### Problem: Orphaned pages\n**Affected Pages**: /old-page-1\n**Migration Plan**: Add internal links from related articles.\n";
        let extracted = crate::engine::text::extract_markdown_document(raw, Some("# SEO Feature Specification"))
            .expect("shared extraction should find the document");
        assert!(validate_spec_content(&extracted).is_ok());
    }
}
