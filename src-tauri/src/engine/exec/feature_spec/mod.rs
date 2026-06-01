//! Agentic feature specification generator.
//!
//! Phase 1: Deterministic intelligence collector aggregates all known audit/data.
//! Phase 2: Agent analyzes the structured report and identifies systemic issues.
//! Phase 3: Verified findings are rendered into markdown by a template engine.
//!
//! Design principle: the system already knows the ground truth. The agent's job
//! is pattern recognition — spotting systemic implementation issues that raw
//! data doesn't scream about. No per-article tool exploration.

pub mod intelligence;

use std::path::Path;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::exec::feature_spec::intelligence::collect_project_intelligence;
use crate::engine::workflows::StepResult;
use crate::models::feature_spec::{FeatureSpecAgentOutput, FeatureSpecFinding, VerifiedEvidence, VerifiedFinding};
use crate::models::task::Task;
use rig::client::CompletionClient;
use rig::completion::Prompt;

/// Agentic step: generate a comprehensive developer feature spec.
///
/// 1. Collects pre-computed project intelligence from all audit sources.
/// 2. Feeds the structured report to an agent for systemic issue analysis.
/// 3. Verifies findings against the DB.
/// 4. Renders verified findings into markdown.
/// 5. Writes to `.github/automation/seo_feature_spec_{task_id}.md`.
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
            message: "Agent found no actionable issues — spec not generated".to_string(),
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
            message: "All agent findings were rejected by verification — spec not generated".to_string(),
            output: None,
        };
    }

    // ── Phase 3: Deterministic rendering ──────────────────────────────────────

    let tech_stack = crate::content::ops::detect_tech_stack(Path::new(project_path));
    let spec_content = render_spec(&verified, &agent_output.executive_summary, tech_stack, task);

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
    // ── Phase 1a: Deterministic intelligence collection ───────────────────────
    let db_path = crate::db::default_db_path();
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("DB open: {e}"))?;

    let report = collect_project_intelligence(&conn, &task.project_id, project_path)
        .map_err(|e| format!("Intelligence collection failed: {e}"))?;
    drop(conn);

    let report_json = serde_json::to_string(&report)
        .map_err(|e| format!("JSON serialize: {e}"))?;
    // Hard cap: 15KB keeps total prompt under bridge 100KB limit and
    // ensures model processing stays well under 300s ACP timeout.
    let report_json = if report_json.len() > 15000 {
        format!("{}...[truncated {} chars]", &report_json[..15000], report_json.len() - 15000)
    } else {
        report_json
    };

    // ── Phase 1b: Agentic analysis (single turn, no tools) ────────────────────

    let prompt = format!(
        "You are an SEO technical lead writing a feature specification for WEBSITE DEVELOPERS.\n\n\
        RULES (violations = hallucinations):\n\
        1. Cross-check ALL template/SEO claims against code_verification section — it is ground truth from actual source files.\n\
        2. CTR data is UNVERIFIED — only report if code_verification shows broken HTML output.\n\
        3. Temporal URLs = ONLY year SUFFIXES like '-2025' at END of slug. Date prefixes '2025-01-18-xxx' are STRIPPED by parser — ignore them.\n\
        4. If last_modified_supported=true, do NOT claim 'missing lastUpdated' — the feature exists.\n\
        5. Duplicate titles use ACTUAL frontmatter titles (verified from MDX files).\n\
        6. Every finding needs REPRODUCIBLE EVIDENCE: file paths, line numbers, or exact data samples.\n\n\
        NEVER mention PageSeeds, automation, or tasks.\n\n\
        Return ONLY valid JSON:\n\
        {{\"executive_summary\":\"2-3 sentences\",\"findings\":[{{\"priority\":\"P0|P1|P2\",\"issue_type\":\"template_bug|missing_seo|content_structure|url_issue|meta_config\",\"description\":\"...\",\"affected_slugs\":[\"slug\"],\"evidence_tool_calls\":[\"file:line or exact sample\"],\"suggested_fix\":\"...\",\"confidence\":0.0-1.0}}]}}\n\n\
        --- REPORT ---\n{}",
        report_json
    );

    let backend = crate::rig::provider::resolve_backend(agent_provider, None, None, None).await
        .map_err(|e| format!("Provider error: {e}"))?;

    let response = match &backend {
        crate::rig::provider::LlmBackend::KimiBridge { base_url, model } => {
            // Use ACP mode (300s timeout) — single-turn with no tools.
            // Direct mode has a 200s hard timeout which is too tight for
            // large intelligence reports. ACP gives 50% more headroom.
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

    let parsed: FeatureSpecAgentOutput = serde_json::from_str(&json_str)
        .map_err(|e| format!("Failed to parse agent output as JSON: {e}. Raw: {}", &response[..response.len().min(500)]))?;

    Ok(parsed)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Phase 2: Deterministic verification
// ═══════════════════════════════════════════════════════════════════════════════

async fn verify_findings(
    findings: &[FeatureSpecFinding],
    task: &Task,
    _project_path: &str,
) -> Result<Vec<VerifiedFinding>, String> {
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

        // Systemic issues without slugs are valid by default
        // (they describe architectural / implementation problems)
        if finding.affected_slugs.is_empty() {
            evidence.push(VerifiedEvidence {
                slug: "-".to_string(),
                metric: "systemic".to_string(),
                value: "no slugs — implementation-level issue".to_string(),
            });
            verified.push(VerifiedFinding {
                priority: finding.priority.clone(),
                issue_type: finding.issue_type.clone(),
                description: finding.description.clone(),
                affected_slugs: vec![],
                evidence,
                suggested_fix: finding.suggested_fix.clone(),
            });
            continue;
        }

        // For findings with slugs, verify each slug exists in the project
        for slug in &finding.affected_slugs {
            if all_slugs.contains(slug) {
                evidence.push(VerifiedEvidence {
                    slug: slug.clone(),
                    metric: "exists".to_string(),
                    value: "true".to_string(),
                });
            } else {
                valid = false;
                break;
            }
        }
        if valid {
            verified.push(VerifiedFinding {
                priority: finding.priority.clone(),
                issue_type: finding.issue_type.clone(),
                description: finding.description.clone(),
                affected_slugs: finding.affected_slugs.clone(),
                evidence,
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
    let p1: Vec<_> = verified.iter().filter(|f| f.priority == "P1").collect();
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
        lines.push("## P0 — Code Changes Required (Developer)".to_string());
        for (i, finding) in p0.iter().enumerate() {
            lines.push(String::new());
            lines.push(format!("### P0.{}: {}", i + 1, finding.issue_type.replace('_', " ")));
            lines.push(format!("- **Problem**: {}", finding.description));
            lines.push("- **Evidence**:".to_string());
            for slug in &finding.affected_slugs {
                lines.push(format!("  - `{}`", slug));
            }
            if !finding.evidence.is_empty() {
                lines.push("- **Verified metrics**:".to_string());
                for ev in &finding.evidence {
                    lines.push(format!("  - `{}`: {} = {}", ev.slug, ev.metric, ev.value));
                }
            }
            lines.push(format!("- **Fix**: {}", finding.suggested_fix));
            lines.push("- **Estimated Effort**: small".to_string());
        }
    }

    if !p1.is_empty() {
        lines.push(String::new());
        lines.push("## P1 — Content Fixes (PageSeeds Can Handle)".to_string());
        for (i, finding) in p1.iter().enumerate() {
            lines.push(String::new());
            lines.push(format!("### P1.{}: {}", i + 1, finding.issue_type.replace('_', " ")));
            lines.push(format!("- **Problem**: {}", finding.description));
            lines.push("- **Affected Pages**:".to_string());
            for slug in &finding.affected_slugs {
                lines.push(format!("  - `{}`", slug));
            }
            lines.push(format!("- **Fix Action**: {}", finding.suggested_fix));
        }
    }

    if !p2.is_empty() {
        lines.push(String::new());
        lines.push("## P2 — Structural Changes (Architecture)".to_string());
        for (i, finding) in p2.iter().enumerate() {
            lines.push(String::new());
            lines.push(format!("### P2.{}: {}", i + 1, finding.issue_type.replace('_', " ")));
            lines.push(format!("- **Problem**: {}", finding.description));
            lines.push("- **Affected Pages**:".to_string());
            for slug in &finding.affected_slugs {
                lines.push(format!("  - `{}`", slug));
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
    all_issues.extend(p1.iter().map(|f| ("P1", &f.issue_type, f.affected_slugs.len())));
    all_issues.extend(p2.iter().map(|f| ("P2", &f.issue_type, f.affected_slugs.len())));

    for (priority, issue_type, count) in all_issues {
        lines.push(format!(
            "| {} | {} | {} | {} | open |",
            issue_type.replace('_', " "),
            priority,
            if priority == "P0" { "Code" } else if priority == "P1" { "Content" } else { "Structural" },
            count
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
        trimmed.contains("P0") || trimmed.contains("P1") || trimmed.contains("P2");
    if !has_priority_section {
        return Err("output is missing priority sections (P0/P1/P2)");
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
        let content = "# SEO Feature Specification\n\nGenerated: 2024-01-01 00:00 UTC\nTriggered by: test (task-id)\n\n## Executive Summary\nThis is a comprehensive summary of the most critical issues and their business impact. We have identified several problems that require immediate attention from the development team.\n\n## P0 — Code Changes Required (Developer)\n\n### Problem: Template rendering failure\n**Evidence**: Multiple pages show generic titles.\n**Root Cause**: Layout component overrides page titles.\n**Fix**: Edit layout.tsx to respect page-level title metadata.\n**Estimated Effort**: small\n\n## P1 — Content Fixes (PageSeeds Can Handle)\n\n### Problem: Thin content\n**Affected Pages**: /blog/post-1, /blog/post-2\n**Fix Action**: Expand articles to minimum 500 words.\n\n## P2 — Structural Changes (Architecture)\n\n### Problem: Orphaned pages\n**Affected Pages**: /old-page-1\n**Migration Plan**: Add internal links from related articles.\n\n## Issue Matrix\n| Issue | Priority | Type | Count | Status |\n|-------|----------|------|-------|--------|\n| Template failure | P0 | Code | 5 | Open |\n";
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

// ═══════════════════════════════════════════════════════════════════════════════
// Live smoke test — requires Kimi bridge running in ACP mode
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::engine::tools::{feature_spec_tools, InvestigationContext};

    /// Full end-to-end prototype: create a temp project with known SEO issues,
    /// populate the DB, and run the feature-spec agent with real tools.
    /// Uses a simplified prompt to avoid model confusion with long histories.
    #[ignore = "requires live Kimi bridge at localhost:8080"]
    #[tokio::test]
    async fn test_feature_spec_prototype_e2e() {
        // ── 1. Set up temp project ─────────────────────────────────────────────
        let tmp = std::env::temp_dir().join("pageseeds-test-prototype");
        let content_dir = tmp.join("content");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&content_dir).unwrap();

        // Create MDX files with deliberate issues for the agent to discover
        std::fs::write(
            content_dir.join("001_about_us.mdx"),
            "---\ntitle: \"About Us\"\ndescription: \"Learn about our coffee roastery\"\npublished_date: \"2024-01-15\"\n---\n\nWe are a specialty coffee roastery based in Auckland, New Zealand.\n",
        ).unwrap();
        std::fs::write(
            content_dir.join("002_coffee_beans.mdx"),
            "---\ntitle: \"Coffee Beans\"\ndescription: \"Our selection of premium coffee beans\"\npublished_date: \"2024-01-20\"\n---\n\nWe source the finest arabica beans from Ethiopia, Colombia, and Brazil.\n",
        ).unwrap();
        std::fs::write(
            content_dir.join("003_brewing_guides.mdx"),
            "---\ntitle: \"Brewing Guides\"\ndescription: \"How to brew the perfect cup\"\npublished_date: \"2024-01-25\"\n---\n\nLearn how to brew pour over, espresso, and french press coffee at home.\n",
        ).unwrap();
        // Empty body → orphan / low-word-count issue
        std::fs::write(
            content_dir.join("004_empty_file.mdx"),
            "---\ntitle: \"Empty File\"\ndescription: \"This file has no body content\"\npublished_date: \"2024-02-01\"\n---\n",
        ).unwrap();
        // Temporal URL pattern
        std::fs::write(
            content_dir.join("005_best_coffee_2024.mdx"),
            "---\ntitle: \"Best Coffee 2024\"\ndescription: \"Top coffee picks for 2024\"\npublished_date: \"2024-02-10\"\n---\n\nHere are our top coffee picks for the year 2024.\n",
        ).unwrap();

        // ── 2. Set up temp DB ──────────────────────────────────────────────────
        let db_path = tmp.join("test.db");
        std::env::set_var("PAGESEEDS_DB_PATH", &db_path);
        let conn = crate::db::init(&db_path).expect("Failed to init DB");

        let project_id = "proj-test-e2e";
        conn.execute(
            "INSERT INTO projects (id, name, path, content_dir, active) VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![project_id, "Test Project", tmp.to_str().unwrap(), content_dir.to_str().unwrap()],
        ).unwrap();

        let articles = vec![
            (1, "About Us", "about_us", "001_about_us.mdx", 50, "2024-01-15"),
            (2, "Coffee Beans", "coffee_beans", "002_coffee_beans.mdx", 55, "2024-01-20"),
            (3, "Brewing Guides", "brewing_guides", "003_brewing_guides.mdx", 60, "2024-01-25"),
            (4, "Empty File", "empty_file", "004_empty_file.mdx", 0, "2024-02-01"),
            (5, "Best Coffee 2024", "best_coffee_2024", "005_best_coffee_2024.mdx", 45, "2024-02-10"),
        ];
        for (id, title, slug, file, wc, date) in articles {
            conn.execute(
                "INSERT INTO articles (id, title, url_slug, file, word_count, published_date, status, project_id) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'published', ?7)",
                rusqlite::params![id, title, slug, file, wc, date, project_id],
            ).unwrap();
        }
        conn.execute(
            "INSERT INTO articles_meta (project_id, next_article_id) VALUES (?1, 6)",
            [project_id],
        ).unwrap();
        drop(conn);

        // ── 3. Run the agent with a simplified prompt ──────────────────────────
        let ctx = InvestigationContext {
            project_id: project_id.to_string(),
            project_path: tmp.to_string_lossy().to_string(),
            db_path: db_path.to_string_lossy().to_string(),
        };

        let preamble = "You are an SEO auditor. Use the available tools to investigate the project, \
            then return a JSON object with an executive_summary and a list of findings. \
            Each finding must have priority (P0/P1/P2), issue_type, description, \
            affected_slugs, evidence_tool_calls, suggested_fix, and confidence (0-1). \
            Only report issues you can verify with tool evidence.";

        let prompt = format!(
            "Investigate this coffee blog project and report SEO issues. \
            Project has 5 articles in {}. \
            Call article_index first to see all articles, then read suspicious ones. \
            Look for: empty files, temporal URLs (with years), very short articles. \
            Return findings as JSON.",
            content_dir.display()
        );

        println!("\n🚀 Starting simplified feature-spec prototype...");
        println!("   Project: {}", tmp.display());
        println!("   DB:      {}", db_path.display());

        let backend = crate::rig::provider::resolve_backend("kimi", None, None, Some("bridge"))
            .await
            .expect("Failed to resolve backend");

        let response = match &backend {
            crate::rig::provider::LlmBackend::KimiBridge { base_url, model } => {
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::HeaderName::from_static("x-kimi-backend"),
                    reqwest::header::HeaderValue::from_static("acp"),
                );
                let client = rig::providers::openai::Client::builder()
                    .base_url(base_url)
                    .api_key("dummy")
                    .http_headers(headers)
                    .build()
                    .expect("Failed to build client");

                let agent = client
                    .completions_api()
                    .agent(model)
                    .preamble(preamble)
                    .tools(feature_spec_tools(ctx))
                    .default_max_turns(5)
                    .build();

                agent.prompt(&prompt).await
                    .expect("Agent prompt failed")
            }
            other => panic!("Expected KimiBridge, got: {:?}", other),
        };

        println!("\n📄 Raw agent response (first 2000 chars):\n{}\n", &response[..response.len().min(2000)]);

        let json_str = crate::engine::text::extract_json_string(&response)
            .unwrap_or_else(|| response.clone());

        let parsed: FeatureSpecAgentOutput = serde_json::from_str(&json_str)
            .expect(&format!("Failed to parse JSON: {}", &json_str[..json_str.len().min(500)]));

        println!("\n✅ Parsed {} findings:\n", parsed.findings.len());
        println!("Executive Summary: {}\n", parsed.executive_summary);
        for f in &parsed.findings {
            println!(
                "  [{}] {} — {} (confidence: {:.0}%)",
                f.priority, f.issue_type, f.description, f.confidence * 100.0
            );
        }

        assert!(!parsed.findings.is_empty(), "Agent should find at least one issue");

        // Cleanup
        let _ = std::fs::remove_dir_all(&tmp);
    }
}

#[cfg(test)]
mod bridge_smoke_tests {
    use rig::client::CompletionClient;
    use rig::completion::Prompt;
    use rig::tool::Tool;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    /// Minimal tool to verify the bridge can execute tool calls.
    #[derive(Debug, Clone)]
    struct EchoTool;

    #[derive(Debug, Serialize, Deserialize, JsonSchema)]
    struct EchoArgs {
        message: String,
    }

    #[derive(Debug, Serialize, JsonSchema)]
    struct EchoOutput {
        echo: String,
    }

    impl Tool for EchoTool {
        const NAME: &'static str = "echo";
        type Error = std::convert::Infallible;
        type Args = EchoArgs;
        type Output = EchoOutput;

        async fn definition(&self, _prompt: String) -> rig::completion::ToolDefinition {
            rig::completion::ToolDefinition {
                name: Self::NAME.to_string(),
                description: "Echoes the input message back. Use this to verify tool calling works.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" }
                    },
                    "required": ["message"]
                }),
            }
        }

        async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
            Ok(EchoOutput {
                echo: format!("Echo: {}", args.message),
            })
        }
    }

    /// Live smoke test: verify Kimi bridge in ACP mode can execute tool calls.
    #[ignore = "requires live Kimi bridge at localhost:8080 in ACP mode"]
    #[tokio::test]
    async fn test_kimi_acp_tool_call() {
        let backend = crate::rig::provider::resolve_backend("kimi", None, None, Some("bridge"))
            .await
            .expect("Failed to resolve backend");

        let response = match &backend {
            crate::rig::provider::LlmBackend::KimiBridge { base_url, model } => {
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::HeaderName::from_static("x-kimi-backend"),
                    reqwest::header::HeaderValue::from_static("acp"),
                );
                let client = rig::providers::openai::Client::builder()
                    .base_url(base_url)
                    .api_key("dummy")
                    .http_headers(headers)
                    .build()
                    .expect("Failed to build client");

                let tools: Vec<Box<dyn rig::tool::ToolDyn>> = vec![Box::new(EchoTool)];
                let agent = client
                    .completions_api()
                    .agent(model)
                    .preamble(
                        "You have access to tools. When a tool result is already present \
                         in the conversation, answer the user directly using that result. \
                         Do NOT call the same tool again.",
                    )
                    .tools(tools)
                    .default_max_turns(5)
                    .build();

                agent.prompt("Call the echo tool with message 'hello from acp' and tell me what it returned.").await
                    .expect("Agent prompt failed")
            }
            other => panic!("Expected KimiBridge, got: {:?}", other),
        };

        println!("Agent response: {}", response);
        assert!(
            response.to_lowercase().contains("echo") || response.to_lowercase().contains("hello"),
            "Response should mention the echo result: {}",
            response
        );
    }
}
