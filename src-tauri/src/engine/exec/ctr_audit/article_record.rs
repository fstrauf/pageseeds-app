//! Single pure builder for the per-article CTR context record shape.
//!
//! Both the audit loop (`context.rs`) and standalone/operator fallback
//! (`standalone_context.rs`) must call [`build_ctr_article_record`] so the
//! JSON shape has one writer. Admission filtering, GSC loading, MDX reads,
//! and `operator_requested` tagging stay in the outer orchestration.

use crate::engine::exec::audit_health::ArticleHealth;
use crate::models::ctr::CtrRenderedPageAudit;

/// Inputs for the shared single-article CTR context record.
///
/// Callers assemble metrics, health, detection reasons, and optional rendered
/// audit; this struct only carries them into the pure JSON builder.
pub(crate) struct CtrArticleRecordParams<'a> {
    pub id: i64,
    pub url_slug: &'a str,
    pub title: &'a str,
    pub target_keyword: &'a str,
    pub meta_description: &'a str,
    pub first_paragraph: &'a str,
    pub h1: &'a str,
    pub file: &'a str,
    pub content_hash: &'a str,
    pub impressions: f64,
    pub clicks: f64,
    pub ctr: f64,
    pub avg_position: f64,
    pub target_ctr: f64,
    pub clicks_lost: f64,
    pub detection_reasons: &'a [String],
    pub health: &'a ArticleHealth,
    pub has_frontmatter_faq: bool,
    pub faq_question_count: usize,
    pub rendered_audit: Option<&'a CtrRenderedPageAudit>,
}

/// Serialize a stored rendered-page audit into the context-record field shape.
pub(crate) fn rendered_audit_to_json(a: &CtrRenderedPageAudit) -> serde_json::Value {
    serde_json::json!({
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
    })
}

/// Build one article context record (pure JSON shape).
///
/// This is the **single writer** of the per-article CTR record used by audit
/// admission and standalone/operator spawn. Callers own detection_reasons
/// (including `operator_requested`) and any title fallbacks.
///
/// Also emits recovery-mode framing fields from #104:
/// `current_year`, `head_query` (null until query enrichment), and
/// `prompt_hint` (set for pure CTR underperformance).
pub(crate) fn build_ctr_article_record(p: CtrArticleRecordParams<'_>) -> serde_json::Value {
    let rendered_json = match p.rendered_audit {
        Some(a) => rendered_audit_to_json(a),
        None => serde_json::Value::Null,
    };

    let reason_refs: Vec<&str> = p.detection_reasons.iter().map(|s| s.as_str()).collect();
    let prompt_hint = super::context::recovery_prompt_hint(&reason_refs);
    let current_year = crate::content::year_policy::current_calendar_year();

    serde_json::json!({
        "id": p.id,
        "url_slug": p.url_slug,
        "title": p.title,
        "target_keyword": p.target_keyword,
        "meta_description": p.meta_description,
        "first_paragraph": p.first_paragraph,
        "h1": p.h1,
        "file": p.file,
        "content_hash": p.content_hash,
        "gsc": {
            "impressions": p.impressions,
            "clicks": p.clicks,
            "ctr": p.ctr,
            "avg_position": p.avg_position,
        },
        "clicks_lost": p.clicks_lost,
        "target_ctr": p.target_ctr,
        "detection_reasons": p.detection_reasons,
        "issues_detected": {
            "file_not_found": !p.health.file_found,
            "title_too_long": !p.health.title_ok,
            "meta_too_short": !p.health.meta_ok,
            "snippet_suboptimal": !p.health.snippet_ok,
            "missing_faq_schema": !p.health.faq_ok,
        },
        "has_frontmatter_faq": p.has_frontmatter_faq,
        "faq_question_count": p.faq_question_count,
        "top_queries": serde_json::Value::Null,
        "current_year": current_year,
        "head_query": serde_json::Value::Null,
        "prompt_hint": prompt_hint,
        "rendered_audit": rendered_json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_health() -> ArticleHealth {
        ArticleHealth {
            title_ok: true,
            meta_ok: true,
            snippet_ok: true,
            faq_ok: false,
            file_found: true,
            issues: vec![],
            snippet_word_count: 50,
            snippet_has_keyword_or_question: true,
        }
    }

    #[test]
    fn build_record_shape_matches_contract() {
        let health = empty_health();
        let reasons = vec!["format_violation".to_string()];
        let rec = build_ctr_article_record(CtrArticleRecordParams {
            id: 7,
            url_slug: "slug",
            title: "Title",
            target_keyword: "kw",
            meta_description: "meta",
            first_paragraph: "para",
            h1: "H1",
            file: "content/x.mdx",
            content_hash: "abc",
            impressions: 1000.0,
            clicks: 10.0,
            ctr: 0.01,
            avg_position: 5.0,
            target_ctr: 0.015,
            clicks_lost: 5.0,
            detection_reasons: &reasons,
            health: &health,
            has_frontmatter_faq: false,
            faq_question_count: 0,
            rendered_audit: None,
        });

        assert_eq!(rec["id"], 7);
        assert_eq!(rec["url_slug"], "slug");
        assert_eq!(rec["title"], "Title");
        assert_eq!(rec["gsc"]["impressions"], 1000.0);
        assert_eq!(rec["target_ctr"], 0.015);
        assert_eq!(rec["clicks_lost"], 5.0);
        assert_eq!(rec["detection_reasons"][0], "format_violation");
        assert_eq!(rec["issues_detected"]["file_not_found"], false);
        assert_eq!(rec["issues_detected"]["missing_faq_schema"], true);
        assert_eq!(rec["top_queries"], serde_json::Value::Null);
        assert_eq!(rec["rendered_audit"], serde_json::Value::Null);
        assert_eq!(rec["content_hash"], "abc");
        assert_eq!(rec["has_frontmatter_faq"], false);
        assert_eq!(rec["faq_question_count"], 0);
        assert_eq!(
            rec["current_year"].as_i64().unwrap(),
            chrono::Datelike::year(&chrono::Utc::now()) as i64
        );
        assert!(rec["head_query"].is_null());
        // format_violation alone → no pure-underperformance prompt_hint
        assert!(rec["prompt_hint"].is_null());
    }

    #[test]
    fn build_record_sets_prompt_hint_for_pure_underperformance() {
        let health = empty_health();
        let reasons = vec!["ctr_underperformance".to_string()];
        let rec = build_ctr_article_record(CtrArticleRecordParams {
            id: 1,
            url_slug: "slug",
            title: "Title",
            target_keyword: "kw",
            meta_description: "meta",
            first_paragraph: "para",
            h1: "H1",
            file: "content/x.mdx",
            content_hash: "abc",
            impressions: 1000.0,
            clicks: 10.0,
            ctr: 0.01,
            avg_position: 5.0,
            target_ctr: 0.015,
            clicks_lost: 5.0,
            detection_reasons: &reasons,
            health: &health,
            has_frontmatter_faq: false,
            faq_question_count: 0,
            rendered_audit: None,
        });
        let hint = rec["prompt_hint"]
            .as_str()
            .expect("prompt_hint for pure underperformance");
        assert!(!hint.is_empty());
    }
}
