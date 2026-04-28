/// Rendered SERP audit — fetch live HTML, extract metadata, compare with source files.
///
/// Detects whether CTR issues belong to source content or target-repo rendering code.

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::ctr::{CtrRenderedPageAudit, CtrSnippetMarkup};
use crate::models::task::Task;
use scraper::{Html, Selector};
use std::collections::HashSet;

/// Run the rendered SERP audit for all articles in a project.
///
/// 1. Reads articles.json
/// 2. For each article, fetches the live page HTML
/// 3. Extracts rendered title, meta, canonical, H1, JSON-LD schema, snippet markup
/// 4. Compares with source-file values
/// 5. Classifies issue source
/// 6. Stores results in ctr_rendered_page_audits table
pub(crate) fn exec_ctr_rendered_serp_audit(
    task: &Task,
    project_path: &str,
    conn: &rusqlite::Connection,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    // Resolve site_url from manifest.json
    let site_url: String = match resolve_site_url(&paths.automation_dir) {
        Some(u) => u,
        None => {
            return StepResult {
                success: false,
                message: "No site_url in manifest.json — skipping rendered audit".to_string(),
                output: None,
            };
        }
    };

    let base_url = normalize_base_url(&site_url);

    // Read articles.json
    let articles_path = paths.automation_dir.join("articles.json");
    let doc: serde_json::Value = match crate::engine::exec::common::read_json(&articles_path, "articles.json") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let empty = vec![];
    let articles = doc["articles"].as_array().unwrap_or(&empty);

    let mut audited = 0usize;
    let mut failed = 0usize;

    for article in articles.iter() {
        let id = article["id"].as_i64().unwrap_or(0);
        let url_slug = article["url_slug"].as_str().unwrap_or("");
        let file_ref = article["file"].as_str().unwrap_or("");
        let source_title = article["title"].as_str().unwrap_or("").to_string();
        let source_description = article["meta_description"]
            .as_str()
            .or_else(|| article["description"].as_str())
            .unwrap_or("")
            .to_string();

        if url_slug.is_empty() || file_ref.is_empty() {
            continue;
        }

        let page_url = format!("{}{}", base_url, url_slug);

        // Fetch and audit in a blocking thread with local tokio runtime
        let page_url_clone = page_url.clone();
        let result = std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async move {
                fetch_and_audit_page(&page_url_clone).await
            })
        }).join();

        let (rendered_title, rendered_desc, canonical, h1, schema_types, has_faq, faq_count, snippet, _fetch_error) = match result {
            Ok(Ok(data)) => data,
            Ok(Err(e)) => {
                log::warn!("[ctr_rendered_audit] Failed to fetch {}: {}", page_url, e);
                failed += 1;
                continue;
            }
            Err(_) => {
                log::warn!("[ctr_rendered_audit] Fetch thread panicked for {}", page_url);
                failed += 1;
                continue;
            }
        };

        // Classify issue source
        let title_issue_source = classify_title_issue(&source_title, &rendered_title);
        let schema_issue = has_source_faq(&file_ref, project_path) && !has_faq;

        let mut issues = Vec::new();
        if rendered_title.len() > crate::engine::exec::audit_health::TITLE_MAX_LEN {
            issues.push("rendered_title_too_long".to_string());
        }
        if is_brand_duplicated(&rendered_title) {
            issues.push("brand_duplicate".to_string());
        }
        if schema_issue {
            issues.push("missing_rendered_faq_page".to_string());
        }
        if !snippet.has_question_h2 && !snippet.has_ordered_list && !snippet.has_table {
            issues.push("snippet_markup_missing".to_string());
        }

        let audit = CtrRenderedPageAudit {
            article_id: id,
            url: page_url,
            file: file_ref.to_string(),
            source_title: source_title.clone(),
            rendered_title: rendered_title.clone(),
            rendered_title_length: rendered_title.len(),
            title_issue_source: title_issue_source.to_string(),
            source_description: source_description.clone(),
            rendered_description: rendered_desc,
            canonical_url: canonical,
            rendered_h1: h1,
            schema_types,
            has_rendered_faq_page: has_faq,
            rendered_faq_question_count: faq_count,
            snippet_markup: snippet,
            issues,
            checked_at: chrono::Utc::now().to_rfc3339(),
        };

        if let Err(e) = crate::db::set_ctr_rendered_audit(conn, &task.project_id, &audit) {
            log::warn!("[ctr_rendered_audit] Failed to store audit for article {}: {}", id, e);
        }

        audited += 1;
    }

    let summary = serde_json::json!({
        "audited": audited,
        "failed": failed,
        "site_url": site_url,
    });

    StepResult {
        success: true,
        message: format!("Rendered SERP audit: {} pages audited, {} failed", audited, failed),
        output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn resolve_site_url(automation_dir: &std::path::Path) -> Option<String> {
    let manifest_path = automation_dir.join("manifest.json");
    std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
        .and_then(|v| v.get("gsc_site").or_else(|| v.get("url")).and_then(|u| u.as_str()).map(String::from))
}

fn normalize_base_url(site_url: &str) -> String {
    let mut url = if site_url.starts_with("sc-domain:") {
        format!("https://{}/", &site_url["sc-domain:".len()..])
    } else if !site_url.ends_with('/') {
        format!("{}/", site_url)
    } else {
        site_url.to_string()
    };
    // Ensure trailing slash
    if !url.ends_with('/') {
        url.push('/');
    }
    url
}

async fn fetch_and_audit_page(
    page_url: &str,
) -> Result<(String, Option<String>, Option<String>, Option<String>, Vec<String>, bool, usize, CtrSnippetMarkup, Option<String>), crate::error::Error> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (compatible; PageSeeds/1.0)")
        .build()
        .map_err(crate::error::Error::Http)?;

    let response = client.get(page_url).send().await.map_err(crate::error::Error::Http)?;
    if !response.status().is_success() {
        return Err(crate::error::Error::Other(format!(
            "HTTP {}",
            response.status()
        )));
    }

    let html = response.text().await.map_err(crate::error::Error::Http)?;
    let document = Html::parse_document(&html);

    let rendered_title = extract_title(&document).unwrap_or_default();
    let rendered_desc = extract_meta_description(&document);
    let canonical = extract_canonical_url(&document, page_url);
    let h1 = extract_h1(&document);
    let (schema_types, faq_count) = extract_json_ld_schema_types_with_faq_count(&document);
    let has_faq = schema_types.iter().any(|t| t == "FAQPage");
    let snippet = extract_snippet_markup(&document);

    Ok((rendered_title, rendered_desc, canonical, h1, schema_types, has_faq, faq_count, snippet, None))
}

fn extract_title(document: &Html) -> Option<String> {
    let selector = Selector::parse("title").ok()?;
    let value = document.select(&selector).next()?.text().collect::<String>();
    let cleaned = value.trim().to_string();
    if cleaned.is_empty() { None } else { Some(cleaned) }
}

fn extract_meta_description(document: &Html) -> Option<String> {
    let selector = Selector::parse("meta[name='description']").ok()?;
    let element = document.select(&selector).next()?;
    let value = element.value().attr("content")?.trim();
    if value.is_empty() { None } else { Some(value.to_string()) }
}

fn extract_canonical_url(document: &Html, fallback: &str) -> Option<String> {
    let selector = Selector::parse("link[rel='canonical']").ok()?;
    let href = document.select(&selector).next()?.value().attr("href")?;
    if href.is_empty() {
        Some(fallback.to_string())
    } else {
        Some(href.to_string())
    }
}

fn extract_h1(document: &Html) -> Option<String> {
    let selector = Selector::parse("h1").ok()?;
    let value = document.select(&selector).next()?.text().collect::<String>();
    let cleaned = value.trim().to_string();
    if cleaned.is_empty() { None } else { Some(cleaned) }
}

pub(crate) fn extract_json_ld_schema_types_with_faq_count(document: &Html) -> (Vec<String>, usize) {
    let selector = match Selector::parse("script[type='application/ld+json']") {
        Ok(s) => s,
        Err(_) => return (Vec::new(), 0),
    };

    let mut types = HashSet::new();
    let mut faq_count = 0usize;

    for element in document.select(&selector) {
        let text = element.text().collect::<String>();
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            // Handle @graph arrays
            if let Some(graph) = json.get("@graph").and_then(|g| g.as_array()) {
                for item in graph {
                    if let Some(t) = item.get("@type").and_then(|t| t.as_str()) {
                        types.insert(t.to_string());
                        if t == "FAQPage" {
                            faq_count += count_faq_questions(item);
                        }
                    }
                }
            }
            // Handle direct @type (string or array)
            if let Some(t) = json.get("@type") {
                if let Some(s) = t.as_str() {
                    types.insert(s.to_string());
                    if s == "FAQPage" {
                        faq_count += count_faq_questions(&json);
                    }
                } else if let Some(arr) = t.as_array() {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            types.insert(s.to_string());
                        }
                    }
                    // If the root is an array containing FAQPage, count questions from root
                    if arr.iter().any(|v| v.as_str() == Some("FAQPage")) {
                        faq_count += count_faq_questions(&json);
                    }
                }
            }
        }
    }
    (types.into_iter().collect(), faq_count)
}

fn count_faq_questions(json: &serde_json::Value) -> usize {
    json.get("mainEntity")
        .and_then(|m| m.as_array())
        .map(|arr| arr.len())
        .unwrap_or(0)
}

fn extract_snippet_markup(document: &Html) -> CtrSnippetMarkup {
    let has_question_h2 = document
        .select(&Selector::parse("h2").unwrap())
        .any(|el| {
            let text = el.text().collect::<String>().to_lowercase();
            text.ends_with('?')
                || text.starts_with("what ")
                || text.starts_with("how ")
                || text.starts_with("why ")
                || text.starts_with("when ")
                || text.starts_with("where ")
                || text.starts_with("who ")
                || text.starts_with("which ")
                || text.starts_with("can ")
                || text.starts_with("does ")
                || text.starts_with("is ")
                || text.starts_with("are ")
        });

    let has_ordered_list = document.select(&Selector::parse("ol").unwrap()).next().is_some();
    let has_table = document.select(&Selector::parse("table").unwrap()).next().is_some();

    CtrSnippetMarkup {
        has_question_h2,
        has_ordered_list,
        has_table,
    }
}

fn classify_title_issue(source_title: &str, rendered_title: &str) -> &'static str {
    if rendered_title.is_empty() {
        return "unknown";
    }
    if source_title != rendered_title {
        // If the rendered title contains duplication patterns not in source, it's likely template
        if is_brand_duplicated(rendered_title) && !is_brand_duplicated(source_title) {
            return "site_template";
        }
        return "site_template";
    }
    if source_title.len() > crate::engine::exec::audit_health::TITLE_MAX_LEN {
        return "content_file";
    }
    "content_file"
}

fn is_brand_duplicated(title: &str) -> bool {
    // Simple heuristic: if a word appears 3+ times, it's likely duplicated brand
    let words: Vec<&str> = title.split_whitespace().collect();
    if words.len() < 4 {
        return false;
    }
    let mut counts = std::collections::HashMap::new();
    for word in words {
        let lower = word.to_lowercase().trim_matches(|c: char| !c.is_alphanumeric()).to_string();
        if !lower.is_empty() {
            *counts.entry(lower).or_insert(0) += 1;
        }
    }
    counts.values().any(|&c| c >= 3)
}

fn has_source_faq(file_ref: &str, project_path: &str) -> bool {
    let repo_root = std::path::Path::new(project_path);
    if let Some(full_path) = crate::engine::exec::audit_health::resolve_content_file(repo_root, file_ref) {
        if let Ok(content) = std::fs::read_to_string(&full_path) {
            return crate::engine::exec::audit_health::has_faq_schema(&content);
        }
    }
    false
}
