use crate::engine::exec::utils::{parse_frontmatter, read_source_file};
/// Content audit execution module.
///
/// Covers:
///   - exec_content_audit   (13-check deterministic article quality audit)
///   - audit_one_article    (per-article check logic)
use crate::engine::project_paths::ProjectPaths;

/// Native Rust replacement for `pageseeds automation seo content-audit`.
///
/// Runs 13 deterministic checks per article (keyword in title/H1/meta, word count,
/// internal links, etc.), scores each article, and writes content_audit.json to
/// automation/content_audit.json. No LLM or external API needed.
pub(crate) fn exec_content_audit(
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
    let link_syntax_re = Regex::new(r"\[([^\]]+)\]\([^)]+\)").unwrap();
    let md_syntax_re = Regex::new(r"[#*_`>|]").unwrap();
    let link_extract_re = Regex::new(r"\[([^\]]*)\]\(([^)]+)\)").unwrap();

    let mut results: Vec<serde_json::Value> = to_audit
        .iter()
        .map(|article| {
            audit_one_article(
                article,
                &paths.repo_root,
                &num_prefix_re,
                &code_block_re,
                &link_syntax_re,
                &md_syntax_re,
                &link_extract_re,
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

    let now_iso = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let output_doc = serde_json::json!({
        "generated_at": now_iso,
        "total_audited": results.len(),
        "health_summary": { "good": good, "needs_improvement": needs, "poor": poor },
        "articles": results,
    });

    let out_path = paths.automation_dir.join("content_audit.json");
    if let Err(e) =
        crate::engine::exec::common::write_json(&out_path, &output_doc, "content_audit.json")
    {
        return e;
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
    link_syntax_re: &regex::Regex,
    md_syntax_re: &regex::Regex,
    link_extract_re: &regex::Regex,
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

    let content_to_analyze = crate::engine::exec::quality_rater::ContentToAnalyze {
        content: &full_content,
        target_keyword: if keyword.is_empty() {
            "podcast"
        } else {
            &keyword
        },
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

    // Word count (strip markdown syntax) — reuse pre-compiled regexes
    let plain = {
        let no_code = code_block_re.replace_all(&body, " ");
        let no_links = link_syntax_re.replace_all(&no_code, "$1");
        let no_md = md_syntax_re.replace_all(&no_links, " ");
        no_md.into_owned()
    };
    let actual_word_count = plain.split_whitespace().count();

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
        "gsc_data":             check_pass(Some(!gsc.is_null()), "GSC data synced"),
        "source_file_found":    check_pass(Some(source.is_some()), "Source file readable"),
        "frontmatter_complete": check_pass(Some(frontmatter_complete), "Frontmatter has title, date, and description"),
        "readability":          check_val(readability.as_ref().map(|_| flesch_score >= 30.0), serde_json::json!(format!("{:.1}", flesch_score)), "Flesch Reading Ease ≥ 30"),
        "passive_voice":        check_val(readability.as_ref().map(|_| passive_voice_pct <= 20.0), serde_json::json!(format!("{:.1}%", passive_voice_pct)), "Passive voice ≤ 20%"),
    });

    // ─── Scoring ─────────────────────────────────────────────────────────────
    let weights = [
        ("broken_links", 30i64),
        ("source_file_found", 20),
        ("title_keyword", 10),
        ("h1_keyword", 10),
        ("meta_desc_keyword", 10),
        ("keyword_first_para", 8),
        ("keyword_density", 8),
        ("meta_desc_present", 7),
        ("frontmatter_complete", 6),
        ("meta_desc_length", 5),
        ("word_count", 5),
        ("h2_structure", 3),
        ("internal_links", 3),
        ("gsc_data", 1),
        ("readability", 8),
        ("passive_voice", 5),
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

    let critical_issues = ["broken_links", "source_file_found", "title_keyword"]
        .iter()
        .filter(|k| checks[*k]["pass"].as_bool() == Some(false))
        .count();
    let high_issues = [
        "meta_desc_keyword",
        "keyword_first_para",
        "keyword_density",
        "h1_keyword",
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
    })
}
