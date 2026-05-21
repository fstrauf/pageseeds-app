use crate::engine::exec::ctr_audit::rendered::extract_json_ld_schema_types_with_faq_count;
/// Site title template detection and fix workflow for CTR recovery.
///
/// Detects repeated brand/title suffix patterns across rendered pages and
/// produces a framework-aware fix plan for the target repository.
use crate::engine::workflows::StepResult;
use crate::models::ctr::{CtrRenderedPageAudit, CtrTemplateDetectionResult, CtrTemplatePageDetail};
use crate::models::task::Task;

/// Minimum number of pages sharing a pattern to qualify as site-wide.
const SITE_TEMPLATE_THRESHOLD: usize = 2;

/// Run deterministic detection of repeated title template patterns.
///
/// 1. Loads all rendered audits for the project.
/// 2. Groups pages by the suffix appended to their source title.
/// 3. For groups crossing the threshold, detects framework files.
/// 4. Returns structured detection results as JSON.
pub(crate) fn exec_ctr_template_detect(
    task: &Task,
    project_path: &str,
    conn: &rusqlite::Connection,
) -> StepResult {
    let audits = match crate::db::list_ctr_rendered_audits(conn, &task.project_id) {
        Ok(a) => a,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to load rendered audits: {}", e),
                output: None,
            };
        }
    };

    if audits.is_empty() {
        return StepResult {
            success: true,
            message: "No rendered audits available — run ctr_audit first".to_string(),
            output: Some("[]".to_string()),
        };
    }

    let results = detect_template_patterns(project_path, &audits);

    if results.is_empty() {
        return StepResult {
            success: true,
            message: "No site-wide title template patterns detected".to_string(),
            output: Some("[]".to_string()),
        };
    }

    let output = serde_json::to_string_pretty(&results).unwrap_or_default();
    StepResult {
        success: true,
        message: format!(
            "Detected {} site-wide title template pattern(s)",
            results.len()
        ),
        output: Some(output),
    }
}

// ─── Detection Core ───────────────────────────────────────────────────────────

fn detect_template_patterns(
    project_path: &str,
    audits: &[CtrRenderedPageAudit],
) -> Vec<CtrTemplateDetectionResult> {
    let mut suffix_groups: std::collections::HashMap<String, Vec<&CtrRenderedPageAudit>> =
        std::collections::HashMap::new();

    for audit in audits {
        if audit.title_issue_source != "site_template" {
            continue;
        }
        if let Some(suffix) = extract_template_suffix(&audit.source_title, &audit.rendered_title) {
            suffix_groups.entry(suffix).or_default().push(audit);
        }
    }

    let framework_files = detect_framework_files(project_path);
    let mut results = Vec::new();

    for (suffix, group) in suffix_groups {
        if group.len() < SITE_TEMPLATE_THRESHOLD {
            continue;
        }

        let detected_pattern = if suffix.starts_with('|') {
            format!("{{title}} {}", &suffix)
        } else {
            format!("{{title}} | {}", &suffix)
        };
        let desired_suffix = deduplicate_suffix(&suffix);
        let desired_pattern = format!("{{title}}{}", &desired_suffix);

        let confidence = if group.len() >= 5 && !framework_files.is_empty() {
            "high"
        } else if group.len() >= 3 || !framework_files.is_empty() {
            "medium"
        } else {
            "low"
        };

        let verification_urls: Vec<String> = group.iter().take(5).map(|a| a.url.clone()).collect();

        let pages: Vec<CtrTemplatePageDetail> = group
            .iter()
            .map(|a| CtrTemplatePageDetail {
                article_id: a.article_id,
                url: a.url.clone(),
                file: a.file.clone(),
                source_title: a.source_title.clone(),
                rendered_title: a.rendered_title.clone(),
            })
            .collect();

        results.push(CtrTemplateDetectionResult {
            detected_pattern,
            desired_pattern,
            affected_pages: group.len(),
            candidate_files: framework_files.clone(),
            confidence: confidence.to_string(),
            requires_manual_review: true,
            verification_urls,
            pages,
        });
    }

    // ─── NEW: Literal template variable detection ────────────────────────────
    // Detect pages where rendered_title contains literal template strings
    // like "| Brand |", "{Brand}", "{{title}}" — indicating unrendered variables
    let literal_var_pages: Vec<&CtrRenderedPageAudit> = audits
        .iter()
        .filter(|a| {
            let rt = a.rendered_title.to_lowercase();
            rt.contains("| brand |")
                || rt.contains("{brand}")
                || rt.contains("{{title}}")
                || rt.contains("{{brand}}")
                || rt.contains("| brandname |")
                || rt.contains("{brandname}")
        })
        .collect();

    if !literal_var_pages.is_empty() {
        let pages: Vec<CtrTemplatePageDetail> = literal_var_pages
            .iter()
            .map(|a| CtrTemplatePageDetail {
                article_id: a.article_id,
                url: a.url.clone(),
                file: a.file.clone(),
                source_title: a.source_title.clone(),
                rendered_title: a.rendered_title.clone(),
            })
            .collect();

        results.push(CtrTemplateDetectionResult {
            detected_pattern: "Literal template variable in title: e.g., '| Brand |' or '{Brand}'".to_string(),
            desired_pattern: "Dynamic title rendering: e.g., 'Article Title | Brand'".to_string(),
            affected_pages: literal_var_pages.len(),
            candidate_files: framework_files.clone(),
            confidence: if literal_var_pages.len() >= 5 { "high" } else { "medium" }.to_string(),
            requires_manual_review: true,
            verification_urls: literal_var_pages.iter().take(5).map(|a| a.url.clone()).collect(),
            pages,
        });
    }

    // ─── NEW: Missing dynamic title detection ────────────────────────────────
    // Detect pages where rendered_title is just the brand name (no dynamic content)
    // This indicates a fallback bug where the page title wasn't set
    let brand_only_pages: Vec<&CtrRenderedPageAudit> = audits
        .iter()
        .filter(|a| {
            let rt = a.rendered_title.trim();
            let st = a.source_title.trim();
            // Title equals brand name — either source_title is empty/brand-only
            // or rendered_title is just a short static string with no article content
            (!st.is_empty() && rt == st && st.len() <= 30)
                || (rt.len() <= 30 && !rt.contains('|') && !rt.contains('-'))
        })
        .collect();

    if !brand_only_pages.is_empty() {
        let pages: Vec<CtrTemplatePageDetail> = brand_only_pages
            .iter()
            .map(|a| CtrTemplatePageDetail {
                article_id: a.article_id,
                url: a.url.clone(),
                file: a.file.clone(),
                source_title: a.source_title.clone(),
                rendered_title: a.rendered_title.clone(),
            })
            .collect();

        results.push(CtrTemplateDetectionResult {
            detected_pattern: "Missing dynamic title: page shows only brand/static name".to_string(),
            desired_pattern: "Each page should have a unique, descriptive title".to_string(),
            affected_pages: brand_only_pages.len(),
            candidate_files: framework_files.clone(),
            confidence: if brand_only_pages.len() >= 5 { "high" } else { "medium" }.to_string(),
            requires_manual_review: true,
            verification_urls: brand_only_pages.iter().take(5).map(|a| a.url.clone()).collect(),
            pages,
        });
    }

    // Sort by affected page count descending
    results.sort_by(|a, b| b.affected_pages.cmp(&a.affected_pages));
    results
}

/// Extract the suffix appended to the source title in the rendered title.
fn extract_template_suffix(source_title: &str, rendered_title: &str) -> Option<String> {
    let source = source_title.trim();
    let rendered = rendered_title.trim();

    if source.is_empty() || rendered.is_empty() {
        return None;
    }

    // Case 1: rendered title starts with source title
    if let Some(rest) = rendered.strip_prefix(source) {
        let suffix = rest.trim();
        if !suffix.is_empty() {
            return Some(suffix.to_string());
        }
    }

    // Case 2: source title is a substring somewhere in rendered title
    // Try to find it and extract what comes after
    if let Some(pos) = rendered.to_lowercase().find(&source.to_lowercase()) {
        let after = &rendered[pos + source.len()..];
        let suffix = after.trim();
        if !suffix.is_empty() {
            return Some(suffix.to_string());
        }
    }

    None
}

/// Remove duplicate segments from a suffix.
///
/// Splits by `|` and removes exact duplicate segments (case-insensitive).
fn deduplicate_suffix(suffix: &str) -> String {
    let segments: Vec<&str> = suffix.split('|').collect();
    let mut seen: Vec<String> = Vec::new();

    for seg in &segments {
        let trimmed = seg.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if !seen.iter().any(|s| s.to_lowercase() == lower) {
            seen.push(trimmed.to_string());
        }
    }

    if seen.is_empty() {
        return format!(" | {}", suffix);
    }

    format!(" | {}", seen.join(" | "))
}

// ─── Framework Detection ──────────────────────────────────────────────────────

fn detect_framework_files(project_path: &str) -> Vec<String> {
    let root = std::path::Path::new(project_path);
    if !root.is_dir() {
        return Vec::new();
    }

    let known_names: std::collections::HashSet<&str> = [
        "layout.tsx",
        "layout.jsx",
        "layout.js",
        "layout.astro",
        "layout.vue",
        "_app.tsx",
        "_app.jsx",
        "_app.js",
        "_document.tsx",
        "_document.jsx",
        "_document.js",
        "page.tsx",
        "page.jsx",
        "page.js",
        "gatsby-config.js",
        "gatsby-config.ts",
        "gatsby-config.mjs",
        "next.config.js",
        "next.config.ts",
        "next.config.mjs",
        "astro.config.mjs",
        "astro.config.ts",
        "astro.config.js",
    ]
    .iter()
    .cloned()
    .collect();

    let mut candidates = Vec::new();

    let walker = walkdir::WalkDir::new(root).max_depth(5).follow_links(false);

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        let path_str = path.to_string_lossy();

        // Skip dependency/build directories
        if path_str.contains("node_modules")
            || path_str.contains(".git")
            || path_str.contains("target")
            || path_str.contains("dist")
            || path_str.contains("build")
            || path_str.contains(".next")
            || path_str.contains(".astro")
        {
            continue;
        }

        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        if known_names.contains(file_name) {
            if let Ok(relative) = path.strip_prefix(root) {
                candidates.push(relative.to_string_lossy().to_string());
            }
        }
    }

    // Content search for metadata-related files — limited to specific directories
    let content_patterns = [
        "generateMetadata",
        "Helmet",
        "SEO",
        "<title>",
        "document.title",
    ];
    let search_dirs = ["src", "app", "pages", "components", "layouts"];
    for dir in &search_dirs {
        let full_dir = root.join(dir);
        if full_dir.is_dir() {
            candidates.extend(search_dir_for_patterns(&full_dir, root, &content_patterns));
        }
    }

    candidates.sort();
    candidates.dedup();
    candidates.truncate(10);
    candidates
}

fn search_dir_for_patterns(
    dir: &std::path::Path,
    root: &std::path::Path,
    patterns: &[&str],
) -> Vec<String> {
    let mut matches = Vec::new();
    let extensions = ["tsx", "jsx", "js", "ts", "astro", "vue", "svelte"];

    let walker = walkdir::WalkDir::new(dir).max_depth(3).follow_links(false);

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let path_str = path.to_string_lossy();
        if path_str.contains("node_modules")
            || path_str.contains(".git")
            || path_str.contains("target")
            || path_str.contains("dist")
            || path_str.contains("build")
        {
            continue;
        }

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !extensions.contains(&ext) {
            continue;
        }

        // Limit file size to avoid reading huge files
        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.len() > 50_000 {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if patterns.iter().any(|p| content.contains(p)) {
            if let Ok(relative) = path.strip_prefix(root) {
                matches.push(relative.to_string_lossy().to_string());
            }
        }
    }

    matches
}

// ─── Verification ─────────────────────────────────────────────────────────────

/// Verify that sample pages no longer contain the duplicate title pattern.
pub(crate) fn exec_ctr_template_verify_render(
    task: &Task,
    _project_path: &str,
    _conn: &rusqlite::Connection,
) -> StepResult {
    // Load the detection artifact from the task
    let detection_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "ctr_template_detection")
        .and_then(|a| a.content.clone())
        .unwrap_or_default();

    if detection_json.is_empty() {
        return StepResult {
            success: false,
            message: "No ctr_template_detection artifact found".to_string(),
            output: None,
        };
    }

    let results: Vec<CtrTemplateDetectionResult> = match serde_json::from_str(&detection_json) {
        Ok(r) => r,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to parse detection artifact: {}", e),
                output: None,
            };
        }
    };

    let mut verified = 0usize;
    let mut failed = 0usize;
    let mut details = Vec::new();

    for result in &results {
        for url in &result.verification_urls {
            let current_title = match fetch_current_title(url) {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("[ctr_template_verify] Failed to fetch {}: {}", url, e);
                    failed += 1;
                    details.push(serde_json::json!({
                        "url": url,
                        "status": "fetch_failed",
                        "error": e
                    }));
                    continue;
                }
            };

            // Check if the detected pattern still applies
            let still_broken = result.detected_pattern.replace("{title}", "").trim()
                == current_title.trim()
                || is_brand_duplicated(&current_title);

            if still_broken {
                failed += 1;
                details.push(serde_json::json!({
                    "url": url,
                    "status": "failed",
                    "rendered_title": current_title,
                    "reason": "duplicate brand still present"
                }));
            } else {
                verified += 1;
                details.push(serde_json::json!({
                    "url": url,
                    "status": "verified",
                    "rendered_title": current_title
                }));
            }
        }
    }

    let summary = serde_json::json!({
        "verified": verified,
        "failed": failed,
        "details": details,
    });

    let all_pass = failed == 0 && verified > 0;
    StepResult {
        success: all_pass,
        message: format!(
            "Template verification: {} passed, {} failed",
            verified, failed
        ),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
    }
}

fn fetch_current_title(url: &str) -> Result<String, crate::error::Error> {
    let url = url.to_string();
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| crate::error::Error::Other(format!("Failed to create runtime: {}", e)))?;

    rt.block_on(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("Mozilla/5.0 (compatible; PageSeeds/1.0)")
            .build()
            .map_err(crate::error::Error::Http)?;

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(crate::error::Error::Http)?;
        if !response.status().is_success() {
            return Err(crate::error::Error::Other(format!(
                "HTTP {}",
                response.status()
            )));
        }

        let html = response.text().await.map_err(crate::error::Error::Http)?;
        let document = scraper::Html::parse_document(&html);

        let selector = scraper::Selector::parse("title").map_err(|_| {
            crate::error::Error::Other("Failed to parse title selector".to_string())
        })?;

        let title = document
            .select(&selector)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        Ok(title)
    })
}

fn is_brand_duplicated(title: &str) -> bool {
    let words: Vec<&str> = title.split_whitespace().collect();
    if words.len() < 4 {
        return false;
    }
    let mut counts = std::collections::HashMap::new();
    for word in words {
        let lower = word
            .to_lowercase()
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_string();
        if !lower.is_empty() {
            *counts.entry(lower).or_insert(0) += 1;
        }
    }
    counts.values().any(|&c| c >= 3)
}

// ─── Schema Renderer Detection ────────────────────────────────────────────────

/// Detect articles where source has FAQ content but rendered HTML lacks FAQPage JSON-LD.
pub(crate) fn exec_ctr_schema_detect(
    task: &Task,
    project_path: &str,
    conn: &rusqlite::Connection,
) -> StepResult {
    let audits = match crate::db::list_ctr_rendered_audits(conn, &task.project_id) {
        Ok(a) => a,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to load rendered audits: {}", e),
                output: None,
            };
        }
    };

    let mut affected = Vec::new();
    for audit in &audits {
        if audit.has_rendered_faq_page || audit.rendered_faq_question_count > 0 {
            continue;
        }
        // Check if source file has FAQ content
        let repo_root = std::path::Path::new(project_path);
        if let Some(full_path) =
            crate::engine::exec::audit_health::resolve_content_file(repo_root, &audit.file)
        {
            if let Ok(content) = std::fs::read_to_string(&full_path) {
                let has_source_faq = crate::engine::exec::audit_health::has_faq_schema(&content);
                if has_source_faq {
                    affected.push(serde_json::json!({
                        "article_id": audit.article_id,
                        "url": audit.url,
                        "file": audit.file,
                        "source_has_faq": true,
                        "rendered_has_faq": false,
                        "rendered_faq_question_count": audit.rendered_faq_question_count,
                    }));
                }
            }
        }
    }

    let output = serde_json::to_string_pretty(&affected).unwrap_or_default();
    StepResult {
        success: true,
        message: format!(
            "Schema renderer detection: {} article(s) with source FAQ but no rendered FAQPage JSON-LD",
            affected.len()
        ),
        output: Some(output),
    }
}

/// Verify that sample pages now contain FAQPage JSON-LD with 3–5 questions.
pub(crate) fn exec_ctr_schema_verify_render(
    task: &Task,
    _project_path: &str,
    _conn: &rusqlite::Connection,
) -> StepResult {
    let detection_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "ctr_schema_detection")
        .and_then(|a| a.content.clone())
        .unwrap_or_default();

    if detection_json.is_empty() {
        return StepResult {
            success: false,
            message: "No ctr_schema_detection artifact found".to_string(),
            output: None,
        };
    }

    let affected: Vec<serde_json::Value> = match serde_json::from_str(&detection_json) {
        Ok(r) => r,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Failed to parse detection artifact: {}", e),
                output: None,
            };
        }
    };

    let mut verified = 0usize;
    let mut failed = 0usize;
    let mut details = Vec::new();

    for item in &affected {
        let url = item["url"].as_str().unwrap_or("");
        if url.is_empty() {
            continue;
        }

        let (has_faq, question_count) = match fetch_current_faq_state(url) {
            Ok(state) => state,
            Err(e) => {
                log::warn!("[ctr_schema_verify] Failed to fetch {}: {}", url, e);
                failed += 1;
                details.push(serde_json::json!({
                    "url": url,
                    "status": "fetch_failed",
                    "error": e.to_string()
                }));
                continue;
            }
        };

        if has_faq && question_count >= 3 {
            verified += 1;
            details.push(serde_json::json!({
                "url": url,
                "status": "verified",
                "faq_question_count": question_count
            }));
        } else {
            failed += 1;
            details.push(serde_json::json!({
                "url": url,
                "status": "failed",
                "has_faq": has_faq,
                "faq_question_count": question_count,
                "reason": if has_faq { "too few questions" } else { "no FAQPage schema" }
            }));
        }
    }

    let summary = serde_json::json!({
        "verified": verified,
        "failed": failed,
        "details": details,
    });

    let all_pass = failed == 0 && verified > 0;
    StepResult {
        success: all_pass,
        message: format!(
            "Schema renderer verification: {} passed, {} failed",
            verified, failed
        ),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
    }
}

fn fetch_current_faq_state(url: &str) -> Result<(bool, usize), crate::error::Error> {
    let url = url.to_string();
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| crate::error::Error::Other(format!("Failed to create runtime: {}", e)))?;

    rt.block_on(async move {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("Mozilla/5.0 (compatible; PageSeeds/1.0)")
            .build()
            .map_err(crate::error::Error::Http)?;

        let response = client
            .get(&url)
            .send()
            .await
            .map_err(crate::error::Error::Http)?;
        if !response.status().is_success() {
            return Err(crate::error::Error::Other(format!(
                "HTTP {}",
                response.status()
            )));
        }

        let html = response.text().await.map_err(crate::error::Error::Http)?;
        let document = scraper::Html::parse_document(&html);

        let (_, faq_count) = extract_json_ld_schema_types_with_faq_count(&document);
        Ok((faq_count > 0, faq_count))
    })
}

// ─── Task Spawner ─────────────────────────────────────────────────────────────

/// Spawn a `fix_ctr_site_template` task if rendered audits show repeated patterns.
pub(crate) fn create_ctr_site_template_task(
    conn: &rusqlite::Connection,
    parent_task: &Task,
    project_path: &str,
) -> Option<String> {
    let audits = match crate::db::list_ctr_rendered_audits(conn, &parent_task.project_id) {
        Ok(a) => a,
        Err(e) => {
            log::warn!("[ctr_template] Failed to load rendered audits: {}", e);
            return None;
        }
    };

    let results = detect_template_patterns(project_path, &audits);
    if results.is_empty() {
        return None;
    }

    let detection_json = match serde_json::to_string_pretty(&results) {
        Ok(j) => j,
        Err(e) => {
            log::warn!("[ctr_template] Failed to serialize detection: {}", e);
            return None;
        }
    };

    let artifact = crate::models::task::TaskArtifact {
        key: "ctr_template_detection".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some("ctr_template_detect".to_string()),
        content: Some(detection_json),
    };

    let idempotency_key = format!(
        "ctr_fix:site_template:{}:{}",
        parent_task.project_id, parent_task.id
    );

    let first_result = &results[0];
    let title = format!(
        "Fix site title template: {} ({} pages)",
        first_result.detected_pattern, first_result.affected_pages
    );

    let spec = crate::engine::spawner::TaskSpec {
        project_id: parent_task.project_id.clone(),
        task_type: "fix_ctr_site_template".to_string(),
        title: Some(title),
        description: Some(format!(
            "Detected repeated title template pattern affecting {} page(s). \
             Pattern: {} → Desired: {}",
            first_result.affected_pages,
            first_result.detected_pattern,
            first_result.desired_pattern
        )),
        priority: crate::models::task::Priority::High,
        run_policy: Some(crate::models::task::TaskRunPolicy::UserEnqueue),
        agent_policy: crate::models::task::AgentPolicy::Optional,
        depends_on: vec![parent_task.id.clone()],
        artifacts: vec![artifact],
        idempotency_key: Some(idempotency_key),
        ..Default::default()
    };

    match crate::engine::spawner::TaskSpawner::spawn(conn, spec) {
        Ok(task) => {
            log::info!(
                "[ctr_template] Created site template fix task {} (pattern: {})",
                task.id,
                first_result.detected_pattern
            );
            Some(task.id)
        }
        Err(e) => {
            log::warn!(
                "[ctr_template] Failed to create site template fix task: {}",
                e
            );
            None
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_template_suffix_simple() {
        assert_eq!(
            extract_template_suffix("Best Stocks", "Best Stocks | Brand"),
            Some("| Brand".to_string())
        );
    }

    #[test]
    fn test_extract_template_suffix_no_match() {
        assert_eq!(extract_template_suffix("Foo", "Bar | Baz"), None);
    }

    #[test]
    fn test_extract_template_suffix_empty_source() {
        assert_eq!(extract_template_suffix("", "Some Title"), None);
    }

    #[test]
    fn test_deduplicate_suffix_exact_duplicates() {
        let suffix = "Days to Expiry | Days to Expiry";
        let result = deduplicate_suffix(suffix);
        assert_eq!(result, " | Days to Expiry");
    }

    #[test]
    fn test_deduplicate_suffix_no_duplicates() {
        let suffix = "Brand | Tagline";
        let result = deduplicate_suffix(suffix);
        assert_eq!(result, " | Brand | Tagline");
    }

    #[test]
    fn test_is_brand_duplicated_true() {
        let title = "Best Stocks | Days to Expiry | Days to Expiry | Days to Expiry";
        assert!(is_brand_duplicated(title));
    }

    #[test]
    fn test_is_brand_duplicated_false() {
        let title = "Best Stocks | Days to Expiry";
        assert!(!is_brand_duplicated(title));
    }

    #[test]
    fn test_detect_template_patterns_groups_correctly() {
        let audits = vec![
            CtrRenderedPageAudit {
                article_id: 1,
                url: "https://example.com/a".to_string(),
                file: "content/001_a.mdx".to_string(),
                source_title: "Article A".to_string(),
                rendered_title: "Article A | Brand | Brand".to_string(),
                rendered_title_length: 25,
                title_issue_source: "site_template".to_string(),
                source_description: "".to_string(),
                rendered_description: None,
                canonical_url: None,
                rendered_h1: None,
                schema_types: vec![],
                has_rendered_faq_page: false,
                rendered_faq_question_count: 0,
                snippet_markup: Default::default(),
                issues: vec!["brand_duplicate".to_string()],
                checked_at: chrono::Utc::now().to_rfc3339(),
            },
            CtrRenderedPageAudit {
                article_id: 2,
                url: "https://example.com/b".to_string(),
                file: "content/002_b.mdx".to_string(),
                source_title: "Article B".to_string(),
                rendered_title: "Article B | Brand | Brand".to_string(),
                rendered_title_length: 25,
                title_issue_source: "site_template".to_string(),
                source_description: "".to_string(),
                rendered_description: None,
                canonical_url: None,
                rendered_h1: None,
                schema_types: vec![],
                has_rendered_faq_page: false,
                rendered_faq_question_count: 0,
                snippet_markup: Default::default(),
                issues: vec!["brand_duplicate".to_string()],
                checked_at: chrono::Utc::now().to_rfc3339(),
            },
            CtrRenderedPageAudit {
                article_id: 3,
                url: "https://example.com/c".to_string(),
                file: "content/003_c.mdx".to_string(),
                source_title: "Article C".to_string(),
                rendered_title: "Article C | Different".to_string(),
                rendered_title_length: 21,
                title_issue_source: "site_template".to_string(),
                source_description: "".to_string(),
                rendered_description: None,
                canonical_url: None,
                rendered_h1: None,
                schema_types: vec![],
                has_rendered_faq_page: false,
                rendered_faq_question_count: 0,
                snippet_markup: Default::default(),
                issues: vec!["rendered_title_too_long".to_string()],
                checked_at: chrono::Utc::now().to_rfc3339(),
            },
        ];

        let results = detect_template_patterns("/tmp", &audits);
        assert_eq!(results.len(), 1, "Should detect exactly 1 template pattern");
        assert_eq!(results[0].affected_pages, 2);
        assert_eq!(results[0].detected_pattern, "{title} | Brand | Brand");
        assert_eq!(results[0].desired_pattern, "{title} | Brand");
    }

    #[test]
    fn test_detect_template_patterns_ignores_content_file_issues() {
        let audits = vec![CtrRenderedPageAudit {
            article_id: 1,
            url: "https://example.com/a".to_string(),
            file: "content/001_a.mdx".to_string(),
            source_title: "Article A".to_string(),
            rendered_title: "Article A | Brand | Brand".to_string(),
            rendered_title_length: 25,
            title_issue_source: "content_file".to_string(),
            source_description: "".to_string(),
            rendered_description: None,
            canonical_url: None,
            rendered_h1: None,
            schema_types: vec![],
            has_rendered_faq_page: false,
            rendered_faq_question_count: 0,
            snippet_markup: Default::default(),
            issues: vec![],
            checked_at: chrono::Utc::now().to_rfc3339(),
        }];

        let results = detect_template_patterns("/tmp", &audits);
        assert!(
            results.is_empty(),
            "Should not detect patterns for content_file issues"
        );
    }
}
