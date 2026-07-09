use crate::engine::exec::ctr_audit::rendered::extract_json_ld_schema_types_with_faq_count;
/// Site title template detection and fix workflow for CTR recovery.
///
/// Detects repeated brand/title suffix patterns across rendered pages and
/// produces a framework-aware fix plan for the target repository.
use crate::engine::workflows::StepResult;
use crate::models::ctr::{CtrRenderedPageAudit, CtrTemplateDetectionResult, CtrTemplatePageDetail};
use crate::models::task::Task;

/// Minimum number of pages sharing a pattern to qualify as site-wide.
use super::*;
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
        assert_eq!(results.len(), 2, "Should detect suffix group + literal variable pattern");
        let suffix_result = results
            .iter()
            .find(|r| r.detected_pattern == "{title} | Brand | Brand")
            .expect("Should find suffix group result");
        assert_eq!(suffix_result.affected_pages, 2);
        assert_eq!(suffix_result.desired_pattern, "{title} | Brand");
        let literal_result = results
            .iter()
            .find(|r| r.detected_pattern.contains("Literal template variable"))
            .expect("Should find literal variable result");
        assert_eq!(literal_result.affected_pages, 2);
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
