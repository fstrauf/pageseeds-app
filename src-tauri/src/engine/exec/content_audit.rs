use crate::engine::exec::utils::{parse_frontmatter, read_source_file};
use sha2::{Digest, Sha256};

/// Content audit execution module.
///
/// Covers:
///   - exec_content_audit   (21-check deterministic article quality audit)
///   - audit_one_article    (per-article check logic)
use crate::engine::project_paths::ProjectPaths;

/// Native Rust replacement for `pageseeds automation seo content-audit`.
///
/// Runs 17 deterministic checks per article (keyword in title/H1/meta, word count,
/// internal links, temporal URLs, page bloat, literal template variables, title
/// token duplication, etc.), scores each article, and writes content_audit.json to
/// automation/content_audit.json. No LLM or external API needed.
pub fn exec_content_audit(
    task: &crate::models::task::Task,
    project_path: &str,
) -> crate::engine::workflows::StepResult {
    use regex::Regex;

    let paths = ProjectPaths::from_path(project_path);

    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(conn) => conn,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to open app database: {}", e),
                output: None,
            }
        }
    };

    let articles = match crate::content::article_index::list_articles(&db, &task.project_id) {
        Ok(a) => a,
        Err(e) => {
            return crate::engine::workflows::StepResult {
                success: false,
                message: format!("Failed to load articles from DB: {}", e),
                output: None,
            }
        }
    };

    // Only audit published/live articles (skip drafts)
    let to_audit: Vec<&crate::models::article::Article> = articles
        .iter()
        .filter(|a| matches!(a.status.to_lowercase().as_str(), "published" | "live" | ""))
        .collect();

    let num_prefix_re = Regex::new(r"^\d+[_\-]+").unwrap();

    // Pre-compile regexes once instead of inside every audit_one_article call.
    // Previously each article re-compiled 4 regexes — for 500 articles that's
    // 2000 regex compilations, each allocating significant temporary memory.
    let code_block_re = Regex::new(r"(?s)```.*?```").unwrap();
    let link_extract_re = Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").unwrap();
    // Detect malformed links like `[text]/blog/slug` (missing parentheses)
    let malformed_link_re = Regex::new(r"\][ \t]*(/blog/[^)\s]*)").unwrap();
    // Detect temporal URLs: month names, years, seasonal, relative time patterns
    let temporal_url_re = Regex::new(
        r"(?i)(-jan-?|-feb-?|-mar-?|-apr-?|-may-?|-jun-?|-jul-?|-aug-?|-sep-?|-oct-?|-nov-?|-dec-?|-\d{4}-|spring|summer|autumn|fall|winter|today|tomorrow|yesterday|this-week|next-week|last-week|this-month|next-month|last-month|this-year|next-year|last-year|now|current)"
    ).unwrap();

    let mut results: Vec<serde_json::Value> = to_audit
        .iter()
        .map(|article| {
            audit_one_article(
                article,
                &paths.repo_root,
                &num_prefix_re,
                &code_block_re,
                &link_extract_re,
                &malformed_link_re,
                &temporal_url_re,
            )
        })
        .collect();

    // Sort: worst first (highest priority_score, lowest health_score)
    results.sort_by(|a, b| {
        let pa = a["priority_score"].as_f64().unwrap_or(0.0);
        let pb = b["priority_score"].as_f64().unwrap_or(0.0);
        pb.partial_cmp(&pa).unwrap_or(std::cmp::Ordering::Equal)
    });

    let good = results
        .iter()
        .filter(|r| r["health"].as_str() == Some("good"))
        .count();
    let needs = results
        .iter()
        .filter(|r| r["health"].as_str() == Some("needs_improvement"))
        .count();
    let poor = results
        .iter()
        .filter(|r| r["health"].as_str() == Some("poor"))
        .count();

    // Compute exact duplicate groups from md5_body_hash
    let mut hash_groups: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
    for (idx, article) in results.iter().enumerate() {
        if let Some(hash) = article["md5_body_hash"].as_str() {
            hash_groups.entry(hash.to_string()).or_default().push(idx);
        }
    }
    let duplicate_groups: Vec<serde_json::Value> = hash_groups
        .values()
        .filter(|g| g.len() > 1)
        .map(|g| {
            let articles: Vec<serde_json::Value> = g
                .iter()
                .map(|&idx| {
                    serde_json::json!({
                        "id": results[idx]["id"],
                        "title": results[idx]["title"],
                        "url_slug": results[idx]["url_slug"],
                        "file": results[idx]["file"],
                    })
                })
                .collect();
            serde_json::json!({
                "hash": results[g[0]]["md5_body_hash"],
                "article_count": g.len(),
                "articles": articles,
            })
        })
        .collect();

    let now_iso = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let output_doc = serde_json::json!({
        "generated_at": now_iso,
        "total_audited": results.len(),
        "health_summary": { "good": good, "needs_improvement": needs, "poor": poor },
        "duplicate_groups": duplicate_groups,
        "duplicate_articles": duplicate_groups.iter().map(|g| g["article_count"].as_u64().unwrap_or(0)).sum::<u64>(),
        "articles": results,
    });

    let out_path = paths.automation_dir.join("content_audit.json");
    if let Err(e) =
        crate::engine::exec::common::write_json(&out_path, &output_doc, "content_audit.json")
    {
        return e;
    }

    // Update content_hash in articles table for each audited article
    for result in &results {
        if let (Some(id), Some(hash)) = (result["id"].as_i64(), result["md5_body_hash"].as_str()) {
            let _ = db.execute(
                "UPDATE articles SET content_hash = ?1 WHERE id = ?2 AND project_id = ?3",
                rusqlite::params![hash, id, &task.project_id],
            );
        }
    }

    crate::engine::workflows::StepResult {
        success: true,
        message: format!(
            "Content audit: {} articles — {} good, {} needs work, {} poor",
            good + needs + poor,
            good,
            needs,
            poor
        ),
        output: Some(
            serde_json::to_string_pretty(&serde_json::json!({
                "total": good + needs + poor,
                "good": good, "needs_improvement": needs, "poor": poor,
                "output_path": out_path.display().to_string(),
            }))
            .unwrap_or_default(),
        ),
    }
}

/// Run all deterministic checks on one article, return an audit record Value.
    pub(crate) fn audit_one_article(
    article: &crate::models::article::Article,
    repo_root: &std::path::Path,
    num_prefix_re: &regex::Regex,
    code_block_re: &regex::Regex,
    link_extract_re: &regex::Regex,
    malformed_link_re: &regex::Regex,
    temporal_url_re: &regex::Regex,
) -> serde_json::Value {
    let keyword = article
        .target_keyword
        .as_deref()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    let title = article.title.trim().to_string();
    let file_ref = article.file.trim().to_string();
    let gsc = serde_json::Value::Null; // TODO: load from sidecar metadata once Phase 2 is implemented
    let published_date = article.published_date.as_deref().unwrap_or("").to_string();
    let status = article.status.to_lowercase();

    // Read source file
    let source = read_source_file(repo_root, &file_ref);
    let (fm, body) = parse_frontmatter(source.as_deref().unwrap_or(""));

    // NEW: Run comprehensive quality rating
    let meta_title = fm.get("title").map(String::as_str).or(Some(title.as_str()));
    let meta_description = fm.get("description").map(String::as_str);
    let full_content = format!("# {}\n\n{}", meta_title.unwrap_or(""), body);

    let fallback_keyword = if keyword.is_empty() {
        // Fall back to a normalized version of the title so quality rater
        // still has something meaningful to check keyword placement against
        Some(title.to_lowercase().replace(|c: char| !c.is_alphanumeric() && c != ' ', " "))
    } else {
        None
    };

    let content_to_analyze = crate::engine::exec::quality_rater::ContentToAnalyze {
        content: &full_content,
        target_keyword: if let Some(ref fk) = fallback_keyword { fk.as_str() } else { &keyword },
        meta_title,
        meta_description,
    };

    let quality_rating = crate::engine::exec::quality_rater::rate_content(&content_to_analyze);

    // NEW: Run readability analysis
    let cleaned_body = crate::content::readability::clean_mdx_for_readability(&body);
    let readability = crate::content::readability::analyze_readability(&cleaned_body).ok();
    let flesch_score = readability
        .as_ref()
        .map(|r| r.flesch_reading_ease)
        .unwrap_or(0.0);
    let passive_voice_pct = readability
        .as_ref()
        .map(|r| r.passive_voice_percentage)
        .unwrap_or(0.0);

    let meta_description = fm
        .get("description")
        .map(String::as_str)
        .unwrap_or("")
        .trim()
        .to_string();

    // Parse headings + structure
    let h1 = body
        .lines()
        .find(|l| l.trim_start().starts_with("# ") && !l.trim_start().starts_with("## "))
        .map(|l| l.trim_start_matches('#').trim().to_string())
        .unwrap_or_default();
    let h2_count = body
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("## ") && !t.starts_with("### ")
        })
        .count();

    let actual_word_count = crate::content::ops::count_words(&body);

    // Keyword density — avoid full body.to_lowercase() by searching case-insensitively
    let kw_count = if keyword.is_empty() {
        0
    } else {
        let kw_lower = keyword.as_str();
        body.to_lowercase().matches(kw_lower).count()
    };
    let kw_density = if actual_word_count > 0 && !keyword.is_empty() {
        kw_count as f64 / actual_word_count as f64 * 100.0
    } else {
        0.0
    };

    // First paragraph (first non-empty, non-heading line)
    let first_para = body
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with("---"))
        .unwrap_or("")
        .to_lowercase();

    // Links — count internal links and broken links without collecting all into a Vec
    let mut internal_link_count = 0usize;
    let mut broken_links = Vec::new();
    for c in link_extract_re.captures_iter(&body) {
        let href = c.get(2).map(|m| m.as_str()).unwrap_or("");
        if !href.starts_with("http") {
            internal_link_count += 1;
        }
        if href.contains("TODO") || href.trim() == "" || href.trim() == "#" {
            let text = c.get(1).map(|m| m.as_str()).unwrap_or("");
            broken_links.push(serde_json::json!({ "text": text, "href": href }));
        }
    }

    // Malformed links — detect `[text]/blog/slug` (missing parentheses around URL)
    let mut malformed_links = Vec::new();
    for c in malformed_link_re.captures_iter(&body) {
        let href = c.get(1).map(|m| m.as_str()).unwrap_or("");
        malformed_links.push(serde_json::json!({ "href": href, "issue": "missing parentheses around URL" }));
    }

    // ─── NEW CHECKS (must be before checks JSON object) ──────────────────────

    // 1. Temporal URL — detect month/year/seasonal/relative-time patterns in slug
    let slug_lower = article.url_slug.to_lowercase();
    let temporal_url = temporal_url_re.is_match(&slug_lower);

    // 2. Page bloat proxy — file size, image/table/code block counts
    let file_size = source.as_ref().map(|s| s.len()).unwrap_or(0);
    let image_count = body.matches("![").count();
    let table_count = body.lines().filter(|l| l.trim_start().starts_with('|')).count();
    let code_block_count = code_block_re.find_iter(&body).count();
    let is_bloated = file_size > 500_000 || image_count > 20 || table_count > 30 || code_block_count > 10;

    // 3. Literal template variable — detect unrendered template variables in title
    let literal_template_variable = title.contains("| Brand |")
        || title.contains("{Brand}")
        || title.contains("{{title}}")
        || title.contains("{{brand}}")
        || title.contains("| BrandName |")
        || title.contains("{BrandName}");

    // 4. Title token duplication — any token appears ≥2 times in title (brand dup, stuffing)
    let title_tokens: Vec<String> = title
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty() && t.len() > 2)
        .map(String::from)
        .collect();
    let mut token_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for token in &title_tokens {
        *token_counts.entry(token.clone()).or_insert(0) += 1;
    }
    let max_token_count = token_counts.values().copied().max().unwrap_or(0);
    let title_token_duplication = max_token_count >= 2;

    // Compute body hash for exact duplicate detection
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    let md5_body_hash = format!("{:x}", hasher.finalize());

    // ─── Checks ──────────────────────────────────────────────────────────────
    let check_pass = |pass: Option<bool>, label: &str| -> serde_json::Value {
        serde_json::json!({ "pass": pass, "label": label })
    };
    let check_val =
        |pass: Option<bool>, value: serde_json::Value, label: &str| -> serde_json::Value {
            serde_json::json!({ "pass": pass, "value": value, "label": label })
        };

    let kw_opt = if keyword.is_empty() {
        None
    } else {
        Some(keyword.clone())
    };

    // Frontmatter completeness check
    let frontmatter_complete = source.is_some() && {
        let has_title = fm.get("title").map(|s| !s.is_empty()).unwrap_or(false);
        let has_date = fm.get("date").map(|s| !s.is_empty()).unwrap_or(false)
            || fm
                .get("publishedDate")
                .map(|s| !s.is_empty())
                .unwrap_or(false)
            || fm
                .get("published_date")
                .map(|s| !s.is_empty())
                .unwrap_or(false);
        let has_desc = fm
            .get("description")
            .map(|s| !s.is_empty())
            .unwrap_or(false)
            || fm
                .get("metaDescription")
                .map(|s| !s.is_empty())
                .unwrap_or(false)
            || fm
                .get("meta_description")
                .map(|s| !s.is_empty())
                .unwrap_or(false);
        has_title && has_date && has_desc
    };

    let checks = serde_json::json!({
        "title_keyword":        check_pass(kw_opt.as_ref().map(|kw| title.to_lowercase().contains(kw.as_str())), "Title contains keyword"),
        "h1_keyword":           check_pass(kw_opt.as_ref().map(|kw| h1.to_lowercase().contains(kw.as_str())), "H1 contains keyword"),
        "meta_desc_present":    check_pass(Some(!meta_description.is_empty()), "Meta description present"),
        "meta_desc_keyword":    check_pass(kw_opt.as_ref().map(|kw| meta_description.to_lowercase().contains(kw.as_str())), "Meta description contains keyword"),
        "meta_desc_length":     check_val(Some(meta_description.len() >= 50 && meta_description.len() <= 155), serde_json::json!(meta_description.len()), "Meta description length 50–155 chars"),
        "keyword_first_para":   check_pass(kw_opt.as_ref().map(|kw| first_para.contains(kw.as_str())), "Keyword in first paragraph"),
        "word_count":           check_val(Some(actual_word_count >= 800), serde_json::json!(actual_word_count), "Word count ≥ 800"),
        "keyword_density":      check_val(kw_opt.as_ref().map(|_| kw_density >= 0.2 && kw_density <= 0.8), serde_json::json!(format!("{:.2}%", kw_density)), "Keyword density 0.2–0.8%"),
        "h2_structure":         check_val(Some(h2_count >= 2), serde_json::json!(h2_count), "Has ≥2 H2 headings"),
        "internal_links":       check_val(Some(internal_link_count >= 3), serde_json::json!(internal_link_count), "Has ≥3 internal links"),
        "broken_links":         serde_json::json!({ "pass": broken_links.is_empty(), "value": broken_links.len(), "issues": broken_links, "label": "No broken/placeholder links" }),
        "malformed_links":      serde_json::json!({ "pass": malformed_links.is_empty(), "value": malformed_links.len(), "issues": malformed_links, "label": "No malformed markdown links (missing parentheses around URL)" }),
        "gsc_data":             check_pass(Some(!gsc.is_null()), "GSC data synced"),
        "source_file_found":    check_pass(Some(source.is_some()), "Source file readable"),
        "frontmatter_complete": check_pass(Some(frontmatter_complete), "Frontmatter has title, date, and description"),
        "readability":          check_val(readability.as_ref().map(|_| flesch_score >= 30.0), serde_json::json!(format!("{:.1}", flesch_score)), "Flesch Reading Ease ≥ 30"),
        "passive_voice":        check_val(readability.as_ref().map(|_| passive_voice_pct <= 20.0), serde_json::json!(format!("{:.1}%", passive_voice_pct)), "Passive voice ≤ 20%"),
        // NEW: SEO audit checks
        "temporal_url":         check_pass(Some(!temporal_url), "URL does not contain temporal patterns (month, year, seasonal, relative time)"),
        "page_bloat_proxy":     check_val(Some(!is_bloated), serde_json::json!({ "file_size": file_size, "image_count": image_count, "table_count": table_count, "code_block_count": code_block_count }), "Page is not bloated (file size ≤ 500KB, images ≤ 20, tables ≤ 30, code blocks ≤ 10)"),
        "literal_template_variable": check_pass(Some(!literal_template_variable), "Title does not contain literal template variables"),
        "title_token_duplication": check_val(Some(!title_token_duplication), serde_json::json!(max_token_count), "No token appears ≥2 times in title"),
    });

    // ─── Scoring ─────────────────────────────────────────────────────────────
    let weights = [
        ("broken_links", 30i64),
        ("malformed_links", 25),
        ("source_file_found", 20),
        ("literal_template_variable", 15),
        ("title_keyword", 10),
        ("h1_keyword", 10),
        ("meta_desc_keyword", 10),
        ("title_token_duplication", 10),
        ("keyword_first_para", 8),
        ("keyword_density", 8),
        ("readability", 8),
        ("temporal_url", 8),
        ("meta_desc_present", 7),
        ("frontmatter_complete", 6),
        ("meta_desc_length", 5),
        ("word_count", 5),
        ("passive_voice", 5),
        ("page_bloat_proxy", 5),
        ("h2_structure", 3),
        ("internal_links", 3),
        ("gsc_data", 1),
    ];
    let penalty: i64 = weights
        .iter()
        .map(|(k, w)| {
            if checks[k]["pass"].as_bool() == Some(false) {
                *w
            } else {
                0
            }
        })
        .sum();
    let health_score = (100 - penalty).max(0);

    let health = if health_score >= 85 {
        "good"
    } else if health_score >= 60 {
        "needs_improvement"
    } else {
        "poor"
    };

    let critical_issues = [
        "broken_links",
        "source_file_found",
        "title_keyword",
        "literal_template_variable",
    ]
    .iter()
    .filter(|k| checks[*k]["pass"].as_bool() == Some(false))
    .count();
    let high_issues = [
        "meta_desc_keyword",
        "keyword_first_para",
        "keyword_density",
        "h1_keyword",
        "title_token_duplication",
        "temporal_url",
    ]
    .iter()
    .filter(|k| checks[*k]["pass"].as_bool() == Some(false))
    .count();

    // GSC priority boost for old articles with no/low impressions
    let gsc_boost: i64 = if gsc.is_null() {
        if let Ok(pub_date) = chrono::NaiveDate::parse_from_str(&published_date, "%Y-%m-%d") {
            let age = (chrono::Utc::now().date_naive() - pub_date).num_days();
            if age > 60 {
                15
            } else {
                0
            }
        } else {
            0
        }
    } else {
        let impressions = gsc["impressions"].as_f64().unwrap_or(0.0) as i64;
        if impressions == 0 {
            10
        } else if impressions < 50 {
            5
        } else {
            0
        }
    };

    let priority_score = penalty + gsc_boost;
    let checks_passed = weights
        .iter()
        .filter(|(k, _)| checks[*k]["pass"].as_bool() == Some(true))
        .count();
    let checks_failed = weights
        .iter()
        .filter(|(k, _)| checks[*k]["pass"].as_bool() == Some(false))
        .count();

    // (new checks moved to before the checks JSON object)
    let _ = num_prefix_re; // used by caller for slug normalization

    serde_json::json!({
        "id": article.id,
        "title": title,
        "url_slug": &article.url_slug,
        "file": file_ref,
        "target_keyword": keyword,
        "status": status,
        "published_date": published_date,
        "word_count": actual_word_count,
        "gsc": gsc,
        "health_score": health_score,
        "health": health,
        "priority_score": priority_score,
        "critical_issues": critical_issues,
        "high_issues": high_issues,
        "checks": checks,
        "checks_passed": checks_passed,
        "checks_failed": checks_failed,
        "checks_total": weights.len(),
        // NEW: Quality rating data
        "quality_score": quality_rating.overall_score,
        "quality_grade": quality_rating.grade,
        "publishing_ready": quality_rating.publishing_ready,
        "quality_breakdown": quality_rating.category_scores,
        "quality_critical": quality_rating.critical_issues,
        "quality_warnings": quality_rating.warnings,
        "quality_suggestions": quality_rating.suggestions,
        // NEW: Readability data
        "readability": readability.map(|r| serde_json::json!({
            "flesch_reading_ease": r.flesch_reading_ease,
            "flesch_kincaid_grade": r.flesch_kincaid_grade,
            "smog_index": r.smog_index,
            "coleman_liau_index": r.coleman_liau_index,
            "automated_readability_index": r.automated_readability_index,
            "passive_voice_percentage": r.passive_voice_percentage,
            "sentence_variety_score": r.sentence_variety_score,
            "avg_sentence_length": r.avg_sentence_length,
            "cliche_count": r.cliche_count,
            "filter_word_percentage": r.filter_word_percentage,
        })),
        // NEW: SEO audit checks (also in checks object for scoring)
        "md5_body_hash": md5_body_hash,
        "temporal_url": temporal_url,
        "page_bloat_proxy": is_bloated,
        "literal_template_variable": literal_template_variable,
        "title_token_duplication": title_token_duplication,
        "title_token_max_count": max_token_count,
        "bloat_metrics": {
            "file_size": file_size,
            "image_count": image_count,
            "table_count": table_count,
            "code_block_count": code_block_count,
        },
    })
}
