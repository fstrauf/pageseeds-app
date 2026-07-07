//! Observation-based SEO feature specification generator.
//!
//! Crawls the live site's sitemap, observes rendered HTML for each URL,
//! and produces a developer feature spec. Framework-agnostic.
//!
//! Principle: observe symptoms in the output. The repo diagnoses root cause.

pub mod intelligence;



use crate::engine::project_paths::ProjectPaths;
use crate::engine::exec::feature_spec::intelligence::collect_site_observations;
use crate::engine::workflows::StepResult;
use crate::models::feature_spec::{FeatureSpecAgentOutput, FeatureSpecFinding, VerifiedEvidence, VerifiedFinding};
use crate::models::task::Task;
use rig::client::CompletionClient;
use rig::completion::Prompt;

/// Agentic step: generate a developer feature spec from live site observations.
///
/// 1. Crawls sitemap.xml to discover all URLs.
/// 2. Fetches rendered HTML for each URL and extracts SEO signals.
/// 3. Detects issues deterministically from observations.
/// 4. Feeds observations to agent for spec writing.
/// 5. Renders findings into markdown.
/// 6. Writes to `.github/automation/seo_feature_spec_{task_id}.md`.
pub async fn exec_generate_feature_spec(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let automation_dir = &paths.automation_dir;

    // ── Phase 1: Agentic discovery ────────────────────────────────────────────

    let agent_output = match run_feature_spec_agent(task, project_path, agent_provider).await {
        Ok(output) => output,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Agent investigation failed: {e}"),
                output: None,
            };
        }
    };

    if agent_output.findings.is_empty() {
        return StepResult {
            success: true,
            message: "Agent found no actionable infrastructure issues — spec not generated".to_string(),
            output: None,
        };
    }

    // ── Phase 2: Deterministic verification ───────────────────────────────────

    let verified = match verify_findings(&agent_output.findings, task, project_path).await {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Verification failed: {e}"),
                output: None,
            };
        }
    };

    if verified.is_empty() {
        return StepResult {
            success: true,
            message: "All agent findings were rejected by source verification — spec not generated".to_string(),
            output: None,
        };
    }

    // ── Phase 3: Deterministic rendering ──────────────────────────────────────

    let spec_content = render_spec(&verified, &agent_output.executive_summary, agent_output.tech_stack.clone(), task);

    // Validate structure
    if let Err(reason) = validate_spec_content(&spec_content) {
        return StepResult {
            success: false,
            message: format!("Rendered spec failed validation: {reason}"),
            output: None,
        };
    }

    // Write output
    let spec_filename = format!("seo_feature_spec_{}.md", task.id);
    let spec_path = automation_dir.join(&spec_filename);
    if let Err(e) = std::fs::create_dir_all(automation_dir) {
        return StepResult {
            success: false,
            message: format!("Failed to create automation dir: {e}"),
            output: None,
        };
    }
    if let Err(e) = std::fs::write(&spec_path, &spec_content) {
        return StepResult {
            success: false,
            message: format!("Failed to write feature spec: {e}"),
            output: None,
        };
    }

    // Stable hard-link for convenience
    let latest_path = automation_dir.join("seo_feature_spec.md");
    let _ = std::fs::remove_file(&latest_path);
    let _ = std::fs::hard_link(&spec_path, &latest_path);

    let word_count = crate::content::ops::count_words(&spec_content);

    StepResult {
        success: true,
        message: format!(
            "Feature spec generated ({} words, {} verified findings) → {}",
            word_count,
            verified.len(),
            spec_path.display()
        ),
        output: Some(spec_path.to_string_lossy().to_string()),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 1: Agentic discovery
// ═══════════════════════════════════════════════════════════════════════════════

async fn run_feature_spec_agent(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> Result<FeatureSpecAgentOutput, String> {
    // ── Phase 1a: Live site observation ───────────────────────────────────────
    let site_url = {
        let db_path = crate::db::default_db_path();
        let conn = rusqlite::Connection::open(&db_path)
            .map_err(|e| format!("DB open: {e}"))?;
        let project = crate::engine::task_store::get_project(&conn, &task.project_id)
            .map_err(|e| format!("get_project: {e}"))?;
        
        // Try site_url first, then fall back to extracting from sitemap_url
        if let Some(url) = project.site_url.filter(|s| !s.is_empty()) {
            url
        } else if let Some(sitemap) = project.sitemap_url.filter(|s| !s.is_empty()) {
            // Extract base URL from sitemap URL (e.g. https://example.com/sitemap.xml → https://example.com)
            sitemap
                .trim_end_matches("/sitemap.xml")
                .trim_end_matches("/sitemap_index.xml")
                .trim_end_matches('/')
                .to_string()
        } else {
            return Err(
                "Project has no Site URL configured. \
                To set it: open the project dropdown in the left sidebar → 'Edit Project' → enter your Site URL (e.g. https://mysite.com). \
                The feature spec generator crawls your live site — it needs a public URL."
                .to_string()
            );
        }
    };

    let report = collect_site_observations(site_url, task.project_id.clone(), project_path.to_string())
        .await
        .map_err(|e| format!("Site observation failed: {e}"))?;

    // Build compact prompt report: only issues + crawl stats.
    // Full page_observations are NOT sent to keep prompt small.
    let compact_report = serde_json::json!({
        "project": {
            "site_url": report.project.site_url,
            "tech_stack": report.project.tech_stack,
        },
        "crawl": {
            "total_urls": report.crawl.total_urls,
            "blog_urls": report.crawl.blog_urls.len(),
            "temporal_urls": report.crawl.blog_urls.iter()
                .filter(|u| u.contains("-2025") || u.contains("-2026") || u.contains("-2024"))
                .take(10)
                .collect::<Vec<_>>(),
        },
        "detected_issues": report.detected_issues,
    });

    let report_json = serde_json::to_string(&compact_report)
        .map_err(|e| format!("JSON serialize: {e}"))?;

    // ── Phase 1b: Agentic analysis (single turn, no tools) ────────────────────

    let prompt = format!(
        "You are a senior SEO engineer writing a developer feature specification. \
        The dev team will read this and implement the fixes.\n\n\
        CRITICAL: Do NOT use any tools. Do NOT call any functions. \
        Return ONLY the JSON object requested below. No XML, no tool calls, no commentary.\n\n\
        SCOPE: Report ONLY issues visible in the live site's rendered HTML output. \
        We crawled the sitemap and inspected pages. Report what we observed.\n\n\
        RULES:\n\
        1. Every finding MUST cite specific observations from the report (exact URLs, exact HTML values).\n\
        2. detected_issues already contains verified issues with evidence. Use these as the primary source.\n\
        3. For each issue, describe the IMPACT on SEO (CTR, crawl budget, rankings, social sharing).\n\
        4. The fix direction should describe WHAT to achieve, not HOW to implement it in code.\n\
        5. NEVER mention PageSeeds, automation, or tasks.\n\
        6. NEVER claim a feature is 'missing' if the observations show it exists.\n\
        7. TEMPORAL URLs: Year suffixes in URLs are NOT automatically bad. Consider:\n\
           - Is the year also in the <title>? (Intentional freshness signaling — may be correct for the niche)\n\
           - Are there MULTIPLE year variants of the same topic? (e.g. budget-2024 AND budget-2025) → only THEN is fragmentation a real problem\n\
           - Is the usage INCONSISTENT? (some posts have years, others don't) → recommend a consistent strategy, not blind migration\n\
           - For personal finance, tax, trends niches: year-dated URLs often outperform evergreen because searchers want current-year info\n\
           - ONLY recommend evergreen migration if there's actual fragmentation or the niche genuinely benefits from it\n\n\
        PRIORITY DEFINITIONS:\n\
        - P0: Broken SEO output visible to crawlers/users (truncated titles, duplicate titles, relative OG images)\n\
        - P2: Structural or configuration gaps (temporal URLs, missing meta descriptions, missing lazy loading)\n\n\
        Return ONLY valid JSON — no markdown, no explanations, no tool calls:\n\
        {{\"executive_summary\":\"2-3 sentences on the most critical observed issues\",\"findings\":[{{\"priority\":\"P0|P2\",\"issue_type\":\"truncated_title|duplicate_title|relative_og_image|temporal_url|missing_meta_description|missing_canonical|missing_lazy_loading|missing_structured_data\",\"description\":\"What we observed\",\"affected_slugs\":[\"slug\"],\"evidence_tool_calls\":[\"URL → exact observation\"],\"suggested_fix\":\"What the dev team should achieve\",\"confidence\":0.0-1.0}}]}}\n\n\
        --- SITE OBSERVATION REPORT ---\n{}",
        report_json
    );

    let backend = crate::rig::provider::resolve_backend(agent_provider, None, None, None).await
        .map_err(|e| format!("Provider error: {e}"))?;

    let response = match &backend {
        crate::rig::provider::LlmBackend::KimiBridge { base_url, model } => {
            let client = rig::providers::openai::Client::builder()
                .base_url(base_url)
                .api_key("dummy")
                .build()
                .map_err(|e| format!("Failed to build bridge client: {e}"))?;
            let agent = client
                .completions_api()
                .agent(model)
                .build();
            agent.prompt(&prompt).await
                .map_err(|e| format!("Agent error: {e}"))?
        }
        crate::rig::provider::LlmBackend::Claude { api_key, model } => {
            let client = rig::providers::anthropic::Client::new(api_key)
                .map_err(|e| format!("Failed to build Claude client: {e}"))?;
            let agent = client
                .agent(model)
                .build();
            agent.prompt(&prompt).await
                .map_err(|e| format!("Agent error: {e}"))?
        }
        crate::rig::provider::LlmBackend::OpenAi { api_key, model } => {
            let client = rig::providers::openai::Client::new(api_key)
                .map_err(|e| format!("Failed to build OpenAI client: {e}"))?;
            let agent = client
                .agent(model)
                .build();
            agent.prompt(&prompt).await
                .map_err(|e| format!("Agent error: {e}"))?
        }
        crate::rig::provider::LlmBackend::Ollama { base_url, model } => {
            use rig::client::Nothing;
            let client = rig::providers::ollama::Client::builder()
                .api_key(Nothing)
                .base_url(base_url)
                .build()
                .map_err(|e| format!("Failed to build Ollama client: {e}"))?;
            let agent = client
                .agent(model)
                .build();
            agent.prompt(&prompt).await
                .map_err(|e| format!("Agent error: {e}"))?
        }
        _ => {
            return Err(format!(
                "Backend '{}' not supported for feature spec generation.",
                agent_provider
            ));
        }
    };

    // Extract JSON from agent response
    let json_str = crate::engine::text::extract_json_string(&response)
        .unwrap_or_else(|| response.clone());

    let mut parsed: FeatureSpecAgentOutput = serde_json::from_str(&json_str)
        .map_err(|e| format!("Failed to parse agent output as JSON: {e}. Raw: {}", &response[..response.len().min(500)]))?;

    // Use observation-based tech stack, not source-file detection
    parsed.tech_stack = report.project.tech_stack;

    Ok(parsed)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 2: Deterministic verification
// ═══════════════════════════════════════════════════════════════════════════════

/// Extract a URL slug from evidence text containing a URL.
/// Handles patterns like "https://brewedlate.com/blog/my-post → ..."
fn extract_slug_from_url(text: &str) -> Option<String> {
    // Find a URL in the text
    let url_start = text.find("http://").or_else(|| text.find("https://"))?;
    let url_end = text[url_start..].find(' ').map(|i| url_start + i).unwrap_or(text.len());
    let url = &text[url_start..url_end];
    
    // Extract path after domain
    let path_start = url.find("://").map(|i| i + 3)?;
    let after_domain = &url[path_start..];
    let path = after_domain.find('/').map(|i| &after_domain[i..]).unwrap_or("");
    
    if path.is_empty() || path == "/" {
        return None;
    }
    
    // Remove leading slash and trailing slashes
    let slug = path.trim_start_matches('/').trim_end_matches('/');
    if slug.is_empty() {
        return None;
    }
    
    Some(slug.to_string())
}

async fn verify_findings(
    findings: &[FeatureSpecFinding],
    task: &Task,
    project_path: &str,
) -> Result<Vec<VerifiedFinding>, String> {
    let _root = std::path::Path::new(project_path);
    let db = rusqlite::Connection::open(crate::db::default_db_path())
        .map_err(|e| format!("DB: {e}"))?;

    let all_slugs: std::collections::HashSet<String> =
        crate::engine::task_store::list_articles(&db, &task.project_id)
            .map_err(|e| format!("DB: {e}"))?
            .into_iter()
            .map(|a| a.url_slug)
            .collect();

    let mut verified = Vec::new();

    for finding in findings {
        let mut evidence = Vec::new();
        let mut valid = true;
        let mut verified_slugs: Vec<String> = Vec::new();

        // For findings with slugs, verify each slug exists in the project.
        // Observation-based findings use URLs as primary evidence; DB slug match
        // is a nice-to-have for linking, not a hard requirement. The crawl already
        // verified the URLs exist.
        if !finding.affected_slugs.is_empty() {
            for slug in &finding.affected_slugs {
                if all_slugs.contains(slug) {
                    verified_slugs.push(slug.clone());
                    evidence.push(VerifiedEvidence {
                        slug: slug.clone(),
                        metric: "exists".to_string(),
                        value: "true".to_string(),
                    });
                }
                // If slug not in DB, still keep it for reporting — don't reject.
            }
        }
        // Also try to extract slugs from URL evidence for extra linking
        for ev in &finding.evidence_tool_calls {
            if let Some(slug) = extract_slug_from_url(ev) {
                if all_slugs.contains(&slug) && !verified_slugs.contains(&slug) {
                    verified_slugs.push(slug.clone());
                    evidence.push(VerifiedEvidence {
                        slug,
                        metric: "exists".to_string(),
                        value: "true".to_string(),
                    });
                }
            }
        }

        // Verify evidence quality: must reference source files, build output,
        // config files, specific audit report fields, or live site observations.
        // Observation-based evidence uses URLs and exact HTML values.
        let has_concrete_evidence = finding.evidence_tool_calls.iter().any(|e| {
            let e_lower = e.to_lowercase();
            e.contains("src/")
                || e.contains("dist/")
                || e.contains("public/")
                || e.contains(".vue")
                || e.contains(".ts")
                || e.contains(".js")
                || e.contains(".json")
                || e.contains("vite.config")
                || e.contains("next.config")
                || e.contains("build_output_audit")
                || e.contains("template_audit")
                || e.contains("performance_signals")
                || e.contains("url_architecture")
                || e.contains("title_quality")
                || e.contains("og_image_audit")
                || e.contains("has_404")
                || e.contains("has_sitemap")
                || e.contains("has_robots")
                || e.contains("has_lazy")
                || e.contains("temporal_url")
                || e.contains("truncated")
                || e.contains("duplicate")
                || e.contains("og:image")
                // Observation-based evidence: URLs, domains, HTML values
                || e_lower.contains("http://")
                || e_lower.contains("https://")
                || e.contains(" → ")  // URL → observation pattern
                || e.contains("<title>")
                || e.contains("<meta")
                || e.contains("og:image")
        });
        if !has_concrete_evidence {
            valid = false;
        }

        if valid {
            verified.push(VerifiedFinding {
                priority: finding.priority.clone(),
                issue_type: finding.issue_type.clone(),
                description: finding.description.clone(),
                affected_slugs: if verified_slugs.is_empty() {
                    finding.affected_slugs.clone()
                } else {
                    verified_slugs
                },
                evidence,
                evidence_tool_calls: finding.evidence_tool_calls.clone(),
                suggested_fix: finding.suggested_fix.clone(),
            });
        }
    }

    Ok(verified)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 3: Deterministic rendering
// ═══════════════════════════════════════════════════════════════════════════════

fn render_spec(
    verified: &[VerifiedFinding],
    executive_summary: &str,
    tech_stack: String,
    task: &Task,
) -> String {
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M UTC").to_string();
    let task_title = task.title.as_deref().unwrap_or("untitled");

    let p0: Vec<_> = verified.iter().filter(|f| f.priority == "P0").collect();
    let p2: Vec<_> = verified.iter().filter(|f| f.priority == "P2").collect();

    let mut lines = vec![
        "# SEO Feature Specification".to_string(),
        String::new(),
        format!("Generated: {}", timestamp),
        format!("Triggered by: {} ({})", task_title, task.id),
        format!("Tech stack: {}", tech_stack),
        String::new(),
        "## Executive Summary".to_string(),
        executive_summary.to_string(),
    ];

    if !p0.is_empty() {
        lines.push(String::new());
        lines.push("## P0 — Code Changes Required".to_string());
        for (i, finding) in p0.iter().enumerate() {
            lines.push(String::new());
            lines.push(format!("### P0.{}: {}", i + 1, finding.issue_type.replace('_', " ")));
            lines.push(format!("- **Problem**: {}", finding.description));
            lines.push("- **Evidence**:".to_string());
            // Show agent evidence (file paths, line numbers, exact data)
            for ev_call in &finding.evidence_tool_calls {
                lines.push(format!("  - {}", ev_call));
            }
            // Only show verified DB evidence for slug-based findings
            let is_systemic = finding.affected_slugs.is_empty();
            if !is_systemic {
                for ev in &finding.evidence {
                    lines.push(format!("  - `{}`: {} = {}", ev.slug, ev.metric, ev.value));
                }
            }
            lines.push(format!("- **Fix**: {}", finding.suggested_fix));
            lines.push("- **Estimated Effort**: small".to_string());
        }
    }

    if !p2.is_empty() {
        lines.push(String::new());
        lines.push("## P2 — Structural Changes".to_string());
        for (i, finding) in p2.iter().enumerate() {
            lines.push(String::new());
            lines.push(format!("### P2.{}: {}", i + 1, finding.issue_type.replace('_', " ")));
            lines.push(format!("- **Problem**: {}", finding.description));
            lines.push("- **Evidence**:".to_string());
            for ev_call in &finding.evidence_tool_calls {
                lines.push(format!("  - {}", ev_call));
            }
            let is_systemic = finding.affected_slugs.is_empty();
            if !is_systemic {
                for ev in &finding.evidence {
                    lines.push(format!("  - `{}`: {} = {}", ev.slug, ev.metric, ev.value));
                }
            }
            lines.push(format!("- **Migration Plan**: {}", finding.suggested_fix));
        }
    }

    // Issue matrix
    lines.push(String::new());
    lines.push("## Issue Matrix".to_string());
    lines.push("| Issue | Priority | Type | Count | Status |".to_string());
    lines.push("|-------|----------|------|-------|--------|".to_string());

    let mut all_issues: Vec<_> = Vec::new();
    all_issues.extend(p0.iter().map(|f| ("P0", &f.issue_type, f.affected_slugs.len())));
    all_issues.extend(p2.iter().map(|f| ("P2", &f.issue_type, f.affected_slugs.len())));

    for (priority, issue_type, count) in all_issues {
        let count_str = if count == 0 { "N/A".to_string() } else { count.to_string() };
        lines.push(format!(
            "| {} | {} | {} | {} | open |",
            issue_type.replace('_', " "),
            priority,
            if priority == "P0" { "Code" } else { "Structural" },
            count_str
        ));
    }

    lines.join("\n")
}

// ═══════════════════════════════════════════════════════════════════════════════
// Shared helpers
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate that the cleaned output is an actual feature spec, not commentary.
fn validate_spec_content(content: &str) -> Result<(), &'static str> {
    let trimmed = content.trim();

    if trimmed.is_empty() {
        return Err("output is empty");
    }

    if !trimmed.starts_with('#') {
        return Err("output does not start with a markdown heading (#)");
    }

    if !trimmed.contains("# SEO Feature Specification") {
        return Err("output is missing '# SEO Feature Specification' heading");
    }

    let has_priority_section =
        trimmed.contains("P0") || trimmed.contains("P2");
    if !has_priority_section {
        return Err("output is missing priority sections (P0/P2)");
    }

    let word_count = crate::content::ops::count_words(trimmed);
    if word_count < 100 {
        return Err("output is too short to be a valid feature spec (<100 words)");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_spec_content_valid() {
        let content = "# SEO Feature Specification\n\nGenerated: 2024-01-01 00:00 UTC\nTriggered by: test (task-id)\n\n## Executive Summary\nThis is a comprehensive summary of the most critical issues and their business impact. We have identified several problems that require immediate attention from the development team.\n\n## P0 — Code Changes Required\n\n### Problem: Template rendering failure\n**Evidence**: Multiple pages show generic titles.\n**Root Cause**: Layout component overrides page titles.\n**Fix**: Edit layout.tsx to respect page-level title metadata.\n**Estimated Effort**: small\n\n## P2 — Structural Changes\n\n### Problem: Orphaned pages\n**Evidence**: /old-page-1\n**Migration Plan**: Add internal links from related articles.\n\n## Issue Matrix\n| Issue | Priority | Type | Count | Status |\n|-------|----------|------|-------|--------|\n| Template failure | P0 | Code | 5 | Open |\n";
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
    fn test_renderer_groups_by_priority() {
        let verified = vec![
            VerifiedFinding {
                priority: "P2".to_string(),
                issue_type: "temporal_url".to_string(),
                description: "URLs contain years".to_string(),
                affected_slugs: vec!["slug-a".to_string()],
                evidence: vec![],
                evidence_tool_calls: vec![],
                suggested_fix: "Migrate to evergreen".to_string(),
            },
            VerifiedFinding {
                priority: "P0".to_string(),
                issue_type: "path_mismatch".to_string(),
                description: "File not at expected path".to_string(),
                affected_slugs: vec!["slug-b".to_string()],
                evidence: vec![VerifiedEvidence {
                    slug: "slug-b".to_string(),
                    metric: "actual_path".to_string(),
                    value: "src/blog/posts/slug-b.mdx".to_string(),
                }],
                evidence_tool_calls: vec!["src/blog/posts/slug-b.mdx not found".to_string()],
                suggested_fix: "Update DB path".to_string(),
            },
        ];

        let rendered = render_spec(&verified, "Summary here.", "Vue+Vite".to_string(), &Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "generate_feature_spec".to_string(),
            title: Some("Test".to_string()),
            ..Default::default()
        });

        // P0 should come before P2
        let p0_pos = rendered.find("P0").unwrap();
        let p2_pos = rendered.find("P2").unwrap();
        assert!(p0_pos < p2_pos);

        // Evidence should be rendered
        assert!(rendered.contains("actual_path = src/blog/posts/slug-b.mdx"));
        assert!(rendered.contains("Summary here."));
    }
}
