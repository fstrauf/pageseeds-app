//! Shared builders for single-article `ctr_context` artifacts.
//!
//! Both audit-spawned `fix_ctr_article` tasks and CLI/operator standalone
//! spawn use this module so the context shape has a single writer:
//!
//! ```json
//! { "total_articles": 1, "articles": [ <article record> ] }
//! ```

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::models::article::Article;
use crate::models::task::TaskArtifact;

/// Wrap a single article record into the standard `ctr_context` document shape.
pub(crate) fn wrap_single_article_ctr_context(
    article_context: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({
        "total_articles": 1,
        "articles": [article_context],
    })
}

/// Build a `ctr_context` [`TaskArtifact`] from a single article record.
///
/// This is the **single writer** of the artifact shape used by audit spawn
/// and standalone/CLI spawn.
pub(crate) fn ctr_context_artifact_from_article(
    article_context: serde_json::Value,
    source: &str,
) -> Result<TaskArtifact, String> {
    let single_context = wrap_single_article_ctr_context(article_context);
    let context_str = serde_json::to_string(&single_context)
        .map_err(|e| format!("Failed to serialize ctr_context: {e}"))?;
    Ok(TaskArtifact {
        key: "ctr_context".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some(source.to_string()),
        content: Some(context_str),
    })
}

/// Build a full single-article `ctr_context` document for standalone/operator spawn.
///
/// Preference order:
/// 1. Slice the matching article from the latest project `ctr_audit_context`
///    (DB artifact or `automation/ctr_audit_context.json` fallback), then tag
///    `detection_reasons` with `"operator_requested"`.
/// 2. Otherwise build a fresh record from the MDX file + article metadata
///    (best-effort GSC from `articles.json`), always including
///    `"operator_requested"` in `detection_reasons`.
pub(crate) fn build_standalone_ctr_context(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    article: &Article,
) -> Result<serde_json::Value, String> {
    if let Some(mut record) =
        try_slice_from_latest_audit(conn, project_id, project_path, article)
    {
        ensure_operator_requested(&mut record);
        return Ok(wrap_single_article_ctr_context(record));
    }

    let record = build_article_record_from_file(conn, project_id, project_path, article);
    Ok(wrap_single_article_ctr_context(record))
}

/// Ensure `detection_reasons` contains `"operator_requested"`.
fn ensure_operator_requested(record: &mut serde_json::Value) {
    let reasons = record
        .as_object_mut()
        .map(|obj| {
            obj.entry("detection_reasons")
                .or_insert_with(|| serde_json::json!([]))
        });
    if let Some(reasons) = reasons {
        let has = reasons
            .as_array()
            .map(|arr| {
                arr.iter()
                    .any(|r| r.as_str() == Some("operator_requested"))
            })
            .unwrap_or(false);
        if !has {
            if let Some(arr) = reasons.as_array_mut() {
                arr.push(serde_json::Value::String(
                    "operator_requested".to_string(),
                ));
            } else {
                *reasons = serde_json::json!(["operator_requested"]);
            }
        }
    }
}

/// Try to find a matching article record in the latest project CTR audit context.
fn try_slice_from_latest_audit(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    article: &Article,
) -> Option<serde_json::Value> {
    let paths = ProjectPaths::from_path(project_path);

    let context_doc: serde_json::Value =
        crate::db::content_audit::get_latest_audit_artifact(conn, project_id, "ctr_audit_context")
            .ok()
            .flatten()
            .or_else(|| {
                let fallback = paths.automation_dir.join("ctr_audit_context.json");
                std::fs::read_to_string(&fallback)
                    .ok()
                    .and_then(|s| serde_json::from_str(&s).ok())
            })?;

    let articles = context_doc.get("articles")?.as_array()?;
    let slug_norm = crate::content::slug::normalize_url_slug(&article.url_slug);

    articles
        .iter()
        .find(|a| {
            let id_match = a.get("id").and_then(|v| v.as_i64()) == Some(article.id);
            let slug = a.get("url_slug").and_then(|v| v.as_str()).unwrap_or("");
            let slug_match = slug == article.url_slug
                || crate::content::slug::normalize_url_slug(slug) == slug_norm;
            id_match || slug_match
        })
        .cloned()
}

/// Build one article context record from the live MDX file + best-effort GSC.
///
/// Shape mirrors `context.rs` (~lines 279–309).
fn build_article_record_from_file(
    conn: &Connection,
    project_id: &str,
    project_path: &str,
    article: &Article,
) -> serde_json::Value {
    let file_ref = &article.file;
    let target_keyword = article.target_keyword.clone().unwrap_or_default();
    let url_slug = &article.url_slug;

    let (current_title, meta_description, first_paragraph, h1, has_faq_schema, file_found) =
        crate::engine::exec::audit_health::read_article_excerpt(project_path, file_ref);

    let repo_root = std::path::Path::new(project_path);
    let file_content =
        crate::engine::exec::audit_health::resolve_content_file(repo_root, file_ref)
            .and_then(|p| std::fs::read_to_string(&p).ok())
            .unwrap_or_default();
    let has_frontmatter_faq =
        crate::engine::exec::audit_health::has_frontmatter_faq(&file_content);
    let faq_question_count =
        crate::engine::exec::audit_health::frontmatter_faq_count(&file_content);

    let content_hash = crate::engine::exec::audit_health::compute_content_hash(
        &current_title,
        &meta_description,
        &first_paragraph,
        has_faq_schema,
    );

    let health = crate::engine::exec::audit_health::check_article_health(
        &current_title,
        &meta_description,
        &first_paragraph,
        &target_keyword,
        has_faq_schema,
        file_found,
    );

    // Best-effort GSC from articles.json
    let (impressions, clicks, ctr, avg_position) =
        load_gsc_for_article(project_path, article.id, url_slug);

    let target_ctr = super::context::target_ctr_for_position(avg_position);
    let clicks_lost = impressions * (target_ctr - ctr).max(0.0);

    let mut detection_reasons = vec!["operator_requested".to_string()];
    if !health.all_ok() {
        detection_reasons.push("format_violation".to_string());
    }
    if super::context::ctr_underperforms(ctr, target_ctr) {
        detection_reasons.push("ctr_underperformance".to_string());
    }

    let rendered_audit = crate::db::get_ctr_rendered_audit(conn, project_id, article.id)
        .ok()
        .flatten();

    let rendered_json = match rendered_audit {
        Some(a) => serde_json::json!({
            "rendered_title": a.rendered_title,
            "rendered_title_length": a.rendered_title_length,
            "title_issue_source": a.title_issue_source,
            "rendered_description": a.rendered_description,
            "rendered_h1": a.rendered_h1,
            "schema_types": a.schema_types,
            "has_rendered_faq_page": a.has_rendered_faq_page,
            "snippet_markup": a.snippet_markup,
            "issues": a.issues,
            "checked_at": a.checked_at,
        }),
        None => serde_json::Value::Null,
    };

    // Prefer live file title; fall back to SQLite article title when file missing.
    let title = if current_title.is_empty() {
        article.title.clone()
    } else {
        current_title
    };

    serde_json::json!({
        "id": article.id,
        "url_slug": url_slug,
        "title": title,
        "target_keyword": target_keyword,
        "meta_description": meta_description,
        "first_paragraph": first_paragraph,
        "h1": h1,
        "file": file_ref,
        "content_hash": content_hash,
        "gsc": {
            "impressions": impressions,
            "clicks": clicks,
            "ctr": ctr,
            "avg_position": avg_position,
        },
        "clicks_lost": clicks_lost,
        "target_ctr": target_ctr,
        "detection_reasons": detection_reasons,
        "issues_detected": {
            "file_not_found": !health.file_found,
            "title_too_long": !health.title_ok,
            "meta_too_short": !health.meta_ok,
            "snippet_suboptimal": !health.snippet_ok,
            "missing_faq_schema": !health.faq_ok,
        },
        "has_frontmatter_faq": has_frontmatter_faq,
        "faq_question_count": faq_question_count,
        "top_queries": serde_json::Value::Null,
        "rendered_audit": rendered_json,
    })
}

/// Best-effort GSC metrics from `articles.json` (by id, then slug).
fn load_gsc_for_article(
    project_path: &str,
    article_id: i64,
    url_slug: &str,
) -> (f64, f64, f64, f64) {
    let paths = ProjectPaths::from_path(project_path);
    let project_articles = crate::engine::exec::common::load_project_articles(&paths);

    let json_article = project_articles
        .articles
        .iter()
        .find(|a| a.get("id").and_then(|v| v.as_i64()) == Some(article_id))
        .or_else(|| project_articles.by_slug.get(url_slug));

    match json_article {
        Some(a) => {
            let gsc = &a["gsc"];
            (
                gsc["impressions"].as_f64().unwrap_or(0.0),
                gsc["clicks"].as_f64().unwrap_or(0.0),
                gsc["ctr"].as_f64().unwrap_or(0.0),
                gsc["avg_position"].as_f64().unwrap_or(0.0),
            )
        }
        None => (0.0, 0.0, 0.0, 0.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use super::super::tests::{cleanup, setup_project, test_db, test_dir};

    fn sample_article() -> Article {
        // Matches setup_project fixture (id=1, test-article).
        Article {
            id: 1,
            title: "Test Article | Brand | Brand -- Tagline".to_string(),
            url_slug: "test-article".to_string(),
            file: "content/001_test_article.mdx".to_string(),
            target_keyword: Some("test article".to_string()),
            keyword_difficulty: None,
            target_volume: 0,
            published_date: None,
            word_count: 100,
            status: "published".to_string(),
            review_status: None,
            review_started_at: None,
            last_reviewed_at: None,
            review_count: 0,
            content_gaps_addressed: vec![],
            estimated_traffic_monthly: None,
            project_id: "proj-test".to_string(),
            quality_score: None,
            quality_grade: None,
            quality_rated_at: None,
            publishing_ready: None,
            quality_breakdown: None,
            page_type: None,
            content_hash: None,
            last_edited_at: None,
        }
    }

    #[test]
    fn wrap_shape_has_total_articles_one() {
        let article = serde_json::json!({
            "id": 42,
            "url_slug": "x",
            "file": "content/x.mdx",
        });
        let doc = wrap_single_article_ctr_context(article);
        assert_eq!(doc["total_articles"], 1);
        assert_eq!(doc["articles"].as_array().unwrap().len(), 1);
        assert_eq!(doc["articles"][0]["id"], 42);
    }

    #[test]
    fn artifact_key_is_ctr_context() {
        let article = serde_json::json!({"id": 1, "file": "a.mdx"});
        let art = ctr_context_artifact_from_article(article, "test").unwrap();
        assert_eq!(art.key, "ctr_context");
        assert_eq!(art.artifact_type.as_deref(), Some("json"));
        assert_eq!(art.source.as_deref(), Some("test"));
        let doc: serde_json::Value =
            serde_json::from_str(art.content.as_deref().unwrap()).unwrap();
        assert_eq!(doc["total_articles"], 1);
    }

    #[test]
    fn standalone_build_from_file_has_operator_requested_and_real_file() {
        let path = test_dir();
        setup_project(&path);
        let conn = test_db();
        // Project row not required for file-based build (rendered audit is best-effort).
        let article = sample_article();
        let doc = build_standalone_ctr_context(&conn, "proj-test", &path, &article).unwrap();

        assert_eq!(doc["total_articles"], 1);
        let rec = &doc["articles"][0];
        assert_eq!(rec["id"], 1);
        assert_eq!(rec["url_slug"], "test-article");
        assert_eq!(rec["file"], "content/001_test_article.mdx");
        assert!(!rec["title"].as_str().unwrap_or("").is_empty());
        // Meta comes from MDX frontmatter ("A short desc")
        assert_eq!(rec["meta_description"], "A short desc");

        let reasons = rec["detection_reasons"].as_array().unwrap();
        assert!(
            reasons
                .iter()
                .any(|r| r.as_str() == Some("operator_requested")),
            "detection_reasons must include operator_requested: {reasons:?}"
        );

        // GSC from articles.json fixture
        assert_eq!(rec["gsc"]["impressions"], 10000.0);

        cleanup(&path);
    }

    #[test]
    fn standalone_slices_from_latest_audit_when_present() {
        let path = test_dir();
        setup_project(&path);
        let conn = test_db();
        // audit_artifacts FK requires a projects row
        super::super::tests::insert_test_project(&conn, &path);

        // Seed an audit context with a rich record for article 1.
        let audit = serde_json::json!({
            "total_articles": 1,
            "articles": [{
                "id": 1,
                "url_slug": "test-article",
                "file": "content/001_test_article.mdx",
                "title": "From Audit",
                "meta_description": "audit meta",
                "detection_reasons": ["format_violation"],
                "issues_detected": {
                    "file_not_found": false,
                    "title_too_long": true,
                    "meta_too_short": false,
                    "snippet_suboptimal": false,
                    "missing_faq_schema": false
                },
                "gsc": { "impressions": 999.0, "clicks": 1.0, "ctr": 0.001, "avg_position": 5.0 }
            }]
        });
        db::content_audit::save_audit_artifact(
            &conn,
            "proj-test",
            "ctr_audit_context",
            "2026-01-01T00:00:00Z",
            &audit.to_string(),
        )
        .unwrap();

        let article = sample_article();
        let doc = build_standalone_ctr_context(&conn, "proj-test", &path, &article).unwrap();
        let rec = &doc["articles"][0];
        assert_eq!(rec["title"], "From Audit");
        assert_eq!(rec["gsc"]["impressions"], 999.0);

        let reasons = rec["detection_reasons"].as_array().unwrap();
        assert!(reasons.iter().any(|r| r.as_str() == Some("format_violation")));
        assert!(reasons.iter().any(|r| r.as_str() == Some("operator_requested")));

        cleanup(&path);
    }
}
