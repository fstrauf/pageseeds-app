use super::*;
use crate::engine::project_paths::ProjectPaths;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn test_dir() -> String {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        std::env::temp_dir()
            .join(format!("can_audit_test_{}_{}", std::process::id(), n))
            .to_string_lossy()
            .to_string()
    }

    fn setup_project(path: &str) {
        let _ = std::fs::remove_dir_all(path);
        let auto_dir = Path::new(path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();
        let content_dir = Path::new(path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        let articles = serde_json::json!({
            "articles": [
                {
                    "id": 1,
                    "url_slug": "best-stocks-csp",
                    "title": "Best Stocks for Cash-Secured Puts",
                    "target_keyword": "cash secured puts",
                    "file": "content/001_best_stocks_csp.mdx",
                    "gsc": { "impressions": 45000.0, "clicks": 120.0, "ctr": 0.0027, "avg_position": 5.5 }
                },
                {
                    "id": 2,
                    "url_slug": "csp-strategy-explained",
                    "title": "Cash-Secured Puts Strategy Explained",
                    "target_keyword": "cash secured puts",
                    "file": "content/002_csp_strategy.mdx",
                    "gsc": { "impressions": 1200.0, "clicks": 5.0, "ctr": 0.0042, "avg_position": 8.2 }
                },
                {
                    "id": 3,
                    "url_slug": "covered-calls-guide",
                    "title": "Covered Calls Complete Guide",
                    "target_keyword": "covered calls",
                    "file": "content/003_covered_calls.mdx",
                    "gsc": { "impressions": 8000.0, "clicks": 30.0, "ctr": 0.0038, "avg_position": 6.1 }
                },
                {
                    "id": 4,
                    "url_slug": "csp-beginners-guide",
                    "title": "Cash-Secured Puts for Beginners",
                    "target_keyword": "cash secured puts",
                    "file": "content/004_csp_beginners.mdx",
                    "gsc": { "impressions": 500.0, "clicks": 2.0, "ctr": 0.004, "avg_position": 12.0 }
                }
            ]
        });
        std::fs::write(
            auto_dir.join("articles.json"),
            serde_json::to_string_pretty(&articles).unwrap(),
        )
        .unwrap();

        let mdx1 = r#"---
title: "Best Stocks for Cash-Secured Puts"
date: "2024-01-01"
---

# Best Stocks for Cash-Secured Puts

This article covers the best stocks for cash secured puts strategy in 2024.

## Criteria

We look for stable blue chip stocks with weekly options.
"#;
        std::fs::write(content_dir.join("001_best_stocks_csp.mdx"), mdx1).unwrap();

        let mdx2 = r#"---
title: "Cash-Secured Puts Strategy Explained"
date: "2024-01-02"
---

# Cash-Secured Puts Strategy Explained

This article covers the cash secured puts strategy for beginners looking for the best stocks.

## How It Works

You sell put options while holding cash to buy the stock if assigned.
"#;
        std::fs::write(content_dir.join("002_csp_strategy.mdx"), mdx2).unwrap();

        let mdx3 = r#"---
title: "Covered Calls Complete Guide"
date: "2024-01-03"
---

# Covered Calls Complete Guide

This guide covers covered calls strategy for income generation.

## Basics

You sell call options against stock you already own.
"#;
        std::fs::write(content_dir.join("003_covered_calls.mdx"), mdx3).unwrap();

        let mdx4 = r#"---
title: "Cash-Secured Puts for Beginners"
date: "2024-01-04"
---

# Cash-Secured Puts for Beginners

Learn the basics of cash secured puts and how to find the best stocks for this income strategy.

## Introduction

Cash secured puts are a great way to generate income.
"#;
        std::fs::write(content_dir.join("004_csp_beginners.mdx"), mdx4).unwrap();
    }

    fn cleanup(path: &str) {
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn test_cosine_similarity_range() {
        let a = TfIdfVector {
            weights: [("apple".to_string(), 1.0), ("banana".to_string(), 1.0)]
                .into_iter()
                .collect(),
            norm: (2.0f64).sqrt(),
        };
        let b = TfIdfVector {
            weights: [("apple".to_string(), 1.0), ("banana".to_string(), 1.0)]
                .into_iter()
                .collect(),
            norm: (2.0f64).sqrt(),
        };
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = TfIdfVector {
            weights: [("cherry".to_string(), 1.0)].into_iter().collect(),
            norm: 1.0,
        };
        assert!(cosine_similarity(&a, &c).abs() < 0.001);
    }

    #[test]
    fn test_exec_can_build_context() {
        let path = test_dir();
        setup_project(&path);
        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "cannibalization_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test Cannibalization Audit".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_can_build_context(&task, &path);
        assert!(result.success, "build_context failed: {}", result.message);

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();

        // Compact summary shape
        assert_eq!(output["summary"]["total_articles"].as_i64().unwrap(), 4);
        assert!(output["summary"]["total_impressions"].as_f64().unwrap() > 0.0);
        assert_eq!(output["summary"]["candidate_clusters"].as_i64().unwrap(), 1);
        assert!(output["summary"]["hub_gaps"].as_i64().unwrap() >= 1);

        // Artifact paths
        assert!(output["artifact_paths"]["context"]
            .as_str()
            .unwrap()
            .contains("cannibalization_audit_context.json"));
        assert!(output["artifact_paths"]["clusters"]
            .as_str()
            .unwrap()
            .contains("cannibalization_clusters.json"));

        // Full artifacts should still be written to disk
        let auto_dir = Path::new(&path).join(".github").join("automation");
        assert!(auto_dir.join("cannibalization_audit_context.json").exists());
        assert!(auto_dir.join("cannibalization_clusters.json").exists());
        assert!(auto_dir.join("hub_gaps.json").exists());
        // Verify clusters artifact has the expected content
        let clusters_content =
            std::fs::read_to_string(auto_dir.join("cannibalization_clusters.json")).unwrap();
        let clusters_doc: serde_json::Value = serde_json::from_str(&clusters_content).unwrap();
        let clusters = clusters_doc["clusters"].as_array().unwrap();
        assert!(!clusters.is_empty());
        let csp_cluster = clusters.iter().find(|c| {
            c["theme"]
                .as_str()
                .unwrap_or("")
                .contains("cash secured puts")
        });
        assert!(csp_cluster.is_some());
        assert_eq!(csp_cluster.unwrap()["pages"].as_array().unwrap().len(), 3);

        cleanup(&path);
    }

    #[test]
    fn test_missing_gsc_data_graceful() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        let auto_dir = Path::new(&path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();
        let content_dir = Path::new(&path).join("content");
        std::fs::create_dir_all(&content_dir).unwrap();

        // Articles with NO gsc data
        let articles = serde_json::json!({
            "articles": [
                {
                    "id": 1,
                    "url_slug": "article-one",
                    "title": "Article One",
                    "target_keyword": "keyword one",
                    "file": "content/article_one.mdx"
                },
                {
                    "id": 2,
                    "url_slug": "article-two",
                    "title": "Article Two",
                    "target_keyword": "keyword one",
                    "file": "content/article_two.mdx"
                }
            ]
        });
        std::fs::write(
            auto_dir.join("articles.json"),
            serde_json::to_string_pretty(&articles).unwrap(),
        )
        .unwrap();

        let mdx = r#"---
title: "Article"
---

# Article

Some content here.
"#;
        std::fs::write(content_dir.join("article_one.mdx"), mdx).unwrap();
        std::fs::write(content_dir.join("article_two.mdx"), mdx).unwrap();

        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "cannibalization_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_can_build_context(&task, &path);
        assert!(
            result.success,
            "Should succeed even with missing GSC data: {}",
            result.message
        );

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        // Articles without GSC data are included in clustering but contribute zero impressions.
        assert_eq!(output["summary"]["total_articles"].as_i64().unwrap(), 2);
        assert_eq!(
            output["summary"]["total_impressions"].as_f64().unwrap(),
            0.0
        );
        assert_eq!(output["summary"]["candidate_clusters"].as_i64().unwrap(), 1);

        cleanup(&path);
    }

    #[test]
    fn test_hub_gap_detection() {
        let records = vec![
            ArticleRecord {
                id: 1,
                url_slug: "best-stocks-csp".to_string(),
                title: "Best Stocks for CSP".to_string(),
                h1: "Best Stocks for CSP".to_string(),
                target_keyword: "cash secured puts".to_string(),
                first_200_words: "...".to_string(),
                file: "a.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 10000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "2024-01-01".to_string(),
                word_count: 100,
                page_type: None,
            },
            ArticleRecord {
                id: 2,
                url_slug: "csp-strategy".to_string(),
                title: "CSP Strategy".to_string(),
                h1: "CSP Strategy".to_string(),
                target_keyword: "cash secured puts".to_string(),
                first_200_words: "...".to_string(),
                file: "b.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 5000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "2024-01-02".to_string(),
                word_count: 100,
                page_type: None,
            },
            ArticleRecord {
                id: 3,
                url_slug: "csp-beginners".to_string(),
                title: "CSP Beginners".to_string(),
                h1: "CSP Beginners".to_string(),
                target_keyword: "cash secured puts".to_string(),
                first_200_words: "...".to_string(),
                file: "c.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 3000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "2024-01-03".to_string(),
                word_count: 100,
                page_type: None,
            },
            ArticleRecord {
                id: 4,
                url_slug: "hub/cash-secured-puts".to_string(),
                title: "Hub CSP".to_string(),
                h1: "Hub CSP".to_string(),
                target_keyword: "cash secured puts".to_string(),
                first_200_words: "...".to_string(),
                file: "d.mdx".to_string(),
                gsc: serde_json::json!({"impressions": 20000.0}),
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "2024-01-04".to_string(),
                word_count: 100,
                page_type: None,
            },
        ];

        let clusters = build_clusters(
            &records,
            &[
                (0, 1, 0.5),
                (1, 2, 0.5),
                (0, 2, 0.5),
                (0, 3, 0.5),
                (1, 3, 0.5),
                (2, 3, 0.5),
            ],
            None,
            "",
        );
        let gaps = detect_hub_gaps(&records, &clusters, None, "");

        // Cluster includes hub page (id 4), so no gap should be reported
        assert!(
            gaps.is_empty(),
            "Should not report hub gap when hub exists in cluster"
        );
    }

    #[test]
    fn test_compute_query_overlap_with_db_data() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();

        let project_id = "proj-overlap";

        // Insert required project row (FK constraint)
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode) VALUES (?1, ?2, ?3, 1, 'workspace')",
            rusqlite::params![project_id, "Test", "/tmp"],
        ).unwrap();

        // Insert query metrics for 3 articles
        // Article 1: queries A, B, C
        crate::db::set_ctr_query_metrics(
            &conn,
            project_id,
            1,
            "/a",
            &[
                ("query a".to_string(), 100.0, 1.0, 0.01, 5.0, None),
                ("query b".to_string(), 80.0, 1.0, 0.01, 6.0, None),
                ("query c".to_string(), 60.0, 1.0, 0.01, 7.0, None),
            ],
            Some("2026-01-01"),
            Some("2026-03-31"),
        )
        .unwrap();

        // Article 2: queries B, C, D
        crate::db::set_ctr_query_metrics(
            &conn,
            project_id,
            2,
            "/b",
            &[
                ("query b".to_string(), 90.0, 1.0, 0.01, 4.0, None),
                ("query c".to_string(), 70.0, 1.0, 0.01, 5.0, None),
                ("query d".to_string(), 50.0, 1.0, 0.01, 8.0, None),
            ],
            Some("2026-01-01"),
            Some("2026-03-31"),
        )
        .unwrap();

        // Article 3: queries C, D, E (no overlap with article 1 except C)
        crate::db::set_ctr_query_metrics(
            &conn,
            project_id,
            3,
            "/c",
            &[
                ("query c".to_string(), 85.0, 1.0, 0.01, 3.0, None),
                ("query d".to_string(), 65.0, 1.0, 0.01, 6.0, None),
                ("query e".to_string(), 45.0, 1.0, 0.01, 9.0, None),
            ],
            Some("2026-01-01"),
            Some("2026-03-31"),
        )
        .unwrap();

        let records = vec![
            ArticleRecord {
                id: 1,
                url_slug: "a".to_string(),
                title: "A".to_string(),
                h1: "A".to_string(),
                target_keyword: "kw".to_string(),
                first_200_words: "".to_string(),
                file: "a.mdx".to_string(),
                gsc: serde_json::Value::Null,
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 0,
                page_type: None,
            },
            ArticleRecord {
                id: 2,
                url_slug: "b".to_string(),
                title: "B".to_string(),
                h1: "B".to_string(),
                target_keyword: "kw".to_string(),
                first_200_words: "".to_string(),
                file: "b.mdx".to_string(),
                gsc: serde_json::Value::Null,
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 0,
                page_type: None,
            },
            ArticleRecord {
                id: 3,
                url_slug: "c".to_string(),
                title: "C".to_string(),
                h1: "C".to_string(),
                target_keyword: "kw".to_string(),
                first_200_words: "".to_string(),
                file: "c.mdx".to_string(),
                gsc: serde_json::Value::Null,
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 0,
                page_type: None,
            },
        ];

        let indices = vec![0, 1, 2];
        let (count, top) = compute_query_overlap(Some(&conn), project_id, &records, &indices);

        // Pairwise overlaps: (A,B)=B,C; (A,C)=C; (B,C)=C,D
        // Union = B, C, D = 3 queries
        assert_eq!(count, 3, "Should find 3 shared queries (B, C, D)");
        assert_eq!(top.len(), 3);
    }

    #[test]
    fn test_compute_query_overlap_fallback_to_proxy() {
        let records = vec![
            ArticleRecord {
                id: 1,
                url_slug: "a".to_string(),
                title: "A".to_string(),
                h1: "A".to_string(),
                target_keyword: "cash secured puts".to_string(),
                first_200_words: "".to_string(),
                file: "a.mdx".to_string(),
                gsc: serde_json::Value::Null,
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 0,
                page_type: None,
            },
            ArticleRecord {
                id: 2,
                url_slug: "b".to_string(),
                title: "B".to_string(),
                h1: "B".to_string(),
                target_keyword: "cash secured puts".to_string(),
                first_200_words: "".to_string(),
                file: "b.mdx".to_string(),
                gsc: serde_json::Value::Null,
                tokens: vec![],
                incoming_links: 0,
                outgoing_links: 0,
                published_date: "".to_string(),
                word_count: 0,
                page_type: None,
            },
        ];

        let indices = vec![0, 1];
        // No DB connection — should fall back to target_keyword proxy
        let (count, top) = compute_query_overlap(None, "proj", &records, &indices);

        assert_eq!(count, 1, "Proxy should find 1 distinct target_keyword");
        assert_eq!(top[0], "cash secured puts");
    }

    #[test]
    fn test_can_select_candidates_produces_merge_candidates() {
        let path = test_dir();
        setup_project(&path);
        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "cannibalization_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let build_result = exec_can_build_context(&task, &path);
        assert!(build_result.success);

        let select_result = exec_can_select_candidates(&task, &path);
        assert!(
            select_result.success,
            "select_candidates failed: {}",
            select_result.message
        );

        let auto_dir = Path::new(&path).join(".github").join("automation");
        assert!(auto_dir.join("cannibalization_candidates.json").exists());

        let candidates_doc: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(auto_dir.join("cannibalization_candidates.json")).unwrap(),
        )
        .unwrap();
        let candidates = candidates_doc["candidates"].as_array().unwrap();
        assert!(
            !candidates.is_empty(),
            "Should produce at least one candidate (fixture has 3 pages with exact keyword 'cash secured puts')"
        );

        // Candidates are evidence-lane merge sets: exact-keyword groups and/or high-sim pairs
        for c in candidates {
            let ctype = c["candidate_type"].as_str().unwrap();
            assert!(
                ctype == "merge_candidate" || ctype == "exact_keyword_dupe",
                "unexpected candidate_type: {}",
                ctype
            );
            assert!(
                c["pages"].as_array().unwrap().len() <= 4,
                "candidate page cap is 4"
            );
            assert!(c["total_impressions"].as_f64().unwrap() >= 0.0);
        }

        cleanup(&path);
    }

    fn audit_task() -> Task {
        Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "cannibalization_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    #[test]
    fn test_build_context_excludes_redirect_sources() {
        let path = test_dir();
        setup_project(&path);
        // csp-strategy-explained was merged away into best-stocks-csp.
        let auto_dir = Path::new(&path).join(".github").join("automation");
        std::fs::write(
            auto_dir.join("redirects.csv"),
            "source,destination,status\n/blog/csp-strategy-explained,/blog/best-stocks-csp,301\n",
        )
        .unwrap();

        let result = exec_can_build_context(&audit_task(), &path);
        assert!(result.success, "build_context failed: {}", result.message);

        let output: serde_json::Value =
            serde_json::from_str(result.output.as_deref().unwrap()).unwrap();
        assert_eq!(output["summary"]["total_articles"].as_i64().unwrap(), 3);

        let ctx =
            std::fs::read_to_string(auto_dir.join("cannibalization_audit_context.json")).unwrap();
        assert!(
            !ctx.contains("csp-strategy-explained"),
            "redirect source must not appear in fingerprint records"
        );
        assert!(ctx.contains("best-stocks-csp"), "keeper stays in the audit");

        cleanup(&path);
    }

    fn write_clusters_file(path: &str, pages: serde_json::Value) {
        let auto_dir = Path::new(path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();
        let doc = serde_json::json!({
            "generated_at": "2024-01-01T00:00:00Z",
            "clusters": [{
                "cluster_id": "c1",
                "theme": "cash secured puts",
                "pages": pages,
                "top_shared_queries": [],
                "shared_query_count": 0,
            }],
        });
        std::fs::write(
            auto_dir.join("cannibalization_clusters.json"),
            serde_json::to_string_pretty(&doc).unwrap(),
        )
        .unwrap();
    }

    fn cluster_page(id: i64, slug: &str, keyword: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "url": format!("/blog/{}", slug),
            "title": slug,
            "h1": slug,
            "target_keyword": keyword,
            "impressions": 100.0,
            "clicks": 5.0,
            "avg_position": 8.0,
            "word_count": 800,
            "incoming_internal_links": 1,
            "outgoing_internal_links": 1,
            "published_date": "2024-01-01",
            "first_200_words": "cash secured puts income strategy",
        })
    }

    fn read_candidates(path: &str) -> Vec<serde_json::Value> {
        let auto_dir = Path::new(path).join(".github").join("automation");
        let doc: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(auto_dir.join("cannibalization_candidates.json")).unwrap(),
        )
        .unwrap();
        doc["candidates"].as_array().unwrap().clone()
    }

    #[test]
    fn test_select_candidates_drops_distinct_keyword_grab_bag() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        // Soft cluster whose pages all have distinct target keywords must NOT
        // become a whole-theme merge candidate (fail-closed shortlist).
        write_clusters_file(
            &path,
            serde_json::json!([
                cluster_page(1, "csp-guide", "cash secured puts"),
                cluster_page(2, "best-csp-stocks", "best stocks for cash secured puts"),
                cluster_page(3, "csp-income", "csp income strategy"),
            ]),
        );

        let result = exec_can_select_candidates(&audit_task(), &path);
        assert!(result.success, "select_candidates failed: {}", result.message);
        assert!(
            result.message.contains("0 merge candidates")
                || result.message.to_lowercase().contains("no cannibalization"),
            "empty shortlist should mention no evidence: {}",
            result.message
        );

        let candidates = read_candidates(&path);
        assert_eq!(
            candidates.len(),
            0,
            "distinct-keyword soft clusters must not invent grab-bag merge candidates"
        );

        cleanup(&path);
    }

    #[test]
    fn test_select_candidates_still_splits_when_every_group_has_two_pages() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        write_clusters_file(
            &path,
            serde_json::json!([
                cluster_page(1, "csp-a", "cash secured puts"),
                cluster_page(2, "csp-b", "cash secured puts"),
                cluster_page(3, "cc-a", "covered calls"),
                cluster_page(4, "cc-b", "covered calls"),
            ]),
        );

        let result = exec_can_select_candidates(&audit_task(), &path);
        assert!(result.success, "select_candidates failed: {}", result.message);

        let candidates = read_candidates(&path);
        assert_eq!(candidates.len(), 2, "balanced groups must still split");
        assert!(candidates
            .iter()
            .all(|c| c["page_count"].as_i64().unwrap() == 2));

        cleanup(&path);
    }

    #[test]
    fn test_select_candidates_emits_high_similarity_pairs() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        let auto_dir = Path::new(&path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();

        // Soft cluster with distinct keywords alone would yield 0 candidates.
        write_clusters_file(
            &path,
            serde_json::json!([
                cluster_page(1, "cold-brew-guide", "cold brew coffee"),
                cluster_page(2, "portable-brewer", "portable coffee maker"),
            ]),
        );

        // Context with a high-sim pair (above PAIR_CANDIDATE_SIMILARITY_THRESHOLD 0.45)
        // and a low-sim pair that must be ignored.
        let context = serde_json::json!({
            "generated_at": "2024-01-01T00:00:00Z",
            "articles": [
                {
                    "id": 1,
                    "url_slug": "cold-brew-guide",
                    "title": "Cold Brew Guide",
                    "h1": "Cold Brew Guide",
                    "target_keyword": "cold brew coffee",
                    "first_200_words": "how to make cold brew coffee at home",
                    "gsc": { "impressions": 1000.0, "clicks": 10.0, "avg_position": 5.0 },
                    "incoming_internal_links": 1,
                    "outgoing_internal_links": 1,
                    "published_date": "2024-01-01",
                    "word_count": 800
                },
                {
                    "id": 2,
                    "url_slug": "portable-brewer",
                    "title": "Portable Coffee Maker",
                    "h1": "Portable Coffee Maker",
                    "target_keyword": "portable coffee maker",
                    "first_200_words": "best portable coffee makers for travel",
                    "gsc": { "impressions": 500.0, "clicks": 5.0, "avg_position": 8.0 },
                    "incoming_internal_links": 0,
                    "outgoing_internal_links": 1,
                    "published_date": "2024-02-01",
                    "word_count": 600
                },
                {
                    "id": 3,
                    "url_slug": "moka-pot",
                    "title": "Moka Pot Guide",
                    "h1": "Moka Pot Guide",
                    "target_keyword": "moka pot",
                    "first_200_words": "using a moka pot on the stove",
                    "gsc": { "impressions": 200.0, "clicks": 2.0, "avg_position": 12.0 },
                    "incoming_internal_links": 0,
                    "outgoing_internal_links": 0,
                    "published_date": "2024-03-01",
                    "word_count": 400
                }
            ],
            "similarity_pairs": [
                {
                    "article_a_id": 1,
                    "article_b_id": 2,
                    "article_a_title": "Cold Brew Guide",
                    "article_b_title": "Portable Coffee Maker",
                    "similarity": 0.62
                },
                {
                    "article_a_id": 1,
                    "article_b_id": 3,
                    "article_a_title": "Cold Brew Guide",
                    "article_b_title": "Moka Pot Guide",
                    "similarity": 0.18
                }
            ]
        });
        std::fs::write(
            auto_dir.join("cannibalization_audit_context.json"),
            serde_json::to_string_pretty(&context).unwrap(),
        )
        .unwrap();

        let result = exec_can_select_candidates(&audit_task(), &path);
        assert!(result.success, "select_candidates failed: {}", result.message);

        let candidates = read_candidates(&path);
        assert_eq!(
            candidates.len(),
            1,
            "only the high-sim pair (≥0.45) should emit a candidate"
        );
        assert_eq!(candidates[0]["page_count"].as_i64().unwrap(), 2);
        assert_eq!(
            candidates[0]["candidate_type"].as_str().unwrap(),
            "merge_candidate"
        );
        let pair_sim = candidates[0]["pair_similarity"].as_f64().unwrap();
        assert!(
            (pair_sim - 0.62).abs() < 0.001,
            "pair_similarity should be preserved"
        );

        cleanup(&path);
    }

    #[test]
    fn test_select_candidates_empty_shortlist_succeeds() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        // Two-page soft cluster, distinct keywords, no context pairs → empty shortlist.
        write_clusters_file(
            &path,
            serde_json::json!([
                cluster_page(1, "alpha-page", "alpha keyword unique"),
                cluster_page(2, "beta-page", "beta keyword unique"),
            ]),
        );

        let result = exec_can_select_candidates(&audit_task(), &path);
        assert!(result.success, "empty shortlist must still succeed");
        let candidates = read_candidates(&path);
        assert!(candidates.is_empty());

        cleanup(&path);
    }

    #[test]
    fn test_select_candidates_caps_keyword_group_at_four() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        write_clusters_file(
            &path,
            serde_json::json!([
                cluster_page(1, "kw-a", "shared keyword"),
                cluster_page(2, "kw-b", "shared keyword"),
                cluster_page(3, "kw-c", "shared keyword"),
                cluster_page(4, "kw-d", "shared keyword"),
                cluster_page(5, "kw-e", "shared keyword"),
                cluster_page(6, "kw-f", "shared keyword"),
            ]),
        );

        let result = exec_can_select_candidates(&audit_task(), &path);
        assert!(result.success, "select_candidates failed: {}", result.message);

        let candidates = read_candidates(&path);
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0]["page_count"].as_i64().unwrap(),
            4,
            "exact-keyword groups cap at 4 pages"
        );

        cleanup(&path);
    }

    #[test]
    fn test_can_reduce_strategy_merges_batch_outputs() {
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        let auto_dir = Path::new(&path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();

        // Write fake batch outputs
        let batch_doc = serde_json::json!({
            "batch_outputs": [
                {
                    "candidate_id": "test_0",
                    "success": true,
                    "message": "ok",
                    "merge_recommendation": {
                        "cluster_theme": "cash secured puts",
                        "keep_url": "/blog/best-stocks-csp",
                        "redirect_urls": ["/blog/csp-strategy-explained"],
                        "merge_instructions": "Merge content",
                        "reason": "Higher impressions",
                        "confidence": "high"
                    }
                },
                {
                    "candidate_id": "test_1",
                    "success": true,
                    "message": "ok",
                    "merge_recommendation": {
                        "no_action": true,
                        "reason": "Topical overlap only"
                    }
                },
                {
                    "candidate_id": "test_2",
                    "success": false,
                    "message": "Agent error"
                }
            ]
        });
        std::fs::write(
            auto_dir.join("cannibalization_batch_outputs.json"),
            serde_json::to_string_pretty(&batch_doc).unwrap(),
        )
        .unwrap();

        // Write minimal hub gaps
        let hub_doc = serde_json::json!({
            "hub_gaps": [
                {
                    "theme": "cash secured puts",
                    "suggested_url": "/hub/cash-secured-puts",
                    "suggested_title": "Cash Secured Puts: Complete Guide",
                    "spoke_pages": [{"id": 1, "url": "/blog/a", "title": "A"}],
                    "reason": "No hub exists"
                }
            ]
        });
        std::fs::write(
            auto_dir.join("hub_gaps.json"),
            serde_json::to_string_pretty(&hub_doc).unwrap(),
        )
        .unwrap();

        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "cannibalization_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_can_reduce_strategy(&task, &path);
        assert!(result.success, "reduce_strategy failed: {}", result.message);

        let strategy_path = auto_dir.join("cannibalization_strategy.json");
        assert!(strategy_path.exists());

        let strategy: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&strategy_path).unwrap()).unwrap();

        // Should include the one valid merge recommendation (test_0)
        let merges = strategy["merge_recommendations"].as_array().unwrap();
        assert_eq!(merges.len(), 1);
        assert_eq!(
            merges[0]["keep_url"].as_str().unwrap(),
            "/blog/best-stocks-csp"
        );
        assert_eq!(merges[0]["confidence"].as_str().unwrap(), "high");

        // Should include hub from deterministic data
        let hubs = strategy["hub_recommendations"].as_array().unwrap();
        assert_eq!(hubs.len(), 1);

        // Should record the failed candidate as a risk
        let risks = strategy["risks"].as_array().unwrap();
        assert!(risks.iter().any(|r| r.as_str().unwrap().contains("test_2")));

        cleanup(&path);
    }

    #[test]
    fn test_can_reduce_strategy_normalizes_non_canonical_urls() {
        // The reducer must canonicalize merge URLs even when a legacy or
        // hand-edited batch output carries non-resolvable slugs (underscores,
        // mixed case, blog/ prefix, trailing slash). Downstream 301 redirects
        // and GSC joins require canonical /blog/<hyphenated-slug> paths, and the
        // agent must never be able to introduce a malformed URL into the artifact.
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        let auto_dir = Path::new(&path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();

        let batch_doc = serde_json::json!({
            "batch_outputs": [{
                "candidate_id": "test_0",
                "success": true,
                "message": "ok",
                "merge_recommendation": {
                    "cluster_theme": "covered calls",
                    "keep_url": "/blog/best_stocks_csp",
                    "redirect_urls": ["/blog/Cash_Secured_Puts_Strategy", "blog/rolling_covered_calls/"],
                    "reason": "Higher impressions",
                    "confidence": "high"
                }
            }]
        });
        std::fs::write(
            auto_dir.join("cannibalization_batch_outputs.json"),
            serde_json::to_string_pretty(&batch_doc).unwrap(),
        )
        .unwrap();
        std::fs::write(
            auto_dir.join("hub_gaps.json"),
            serde_json::json!({ "hub_gaps": [] }).to_string(),
        )
        .unwrap();

        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "cannibalization_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_can_reduce_strategy(&task, &path);
        assert!(result.success, "{}", result.message);

        let strategy: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(auto_dir.join("cannibalization_strategy.json")).unwrap(),
        )
        .unwrap();
        let merges = strategy["merge_recommendations"].as_array().unwrap();
        assert_eq!(merges.len(), 1);
        // Underscored + mixed-case keeper → canonical hyphenated.
        assert_eq!(
            merges[0]["keep_url"].as_str().unwrap(),
            "/blog/best-stocks-csp"
        );
        // Each redirect normalized: underscores→hyphens, lowercased,
        // blog/ prefix and trailing slash handled.
        let redirs: Vec<&str> = merges[0]["redirect_urls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(
            redirs,
            vec![
                "/blog/cash-secured-puts-strategy",
                "/blog/rolling-covered-calls"
            ]
        );

        cleanup(&path);
    }

    #[test]
    fn test_merge_prompt_budget_and_trim() {
        let skill = "# Skill\n\nSome instructions here.".to_string();
        let candidate = serde_json::json!({
            "candidate_id": "test",
            "pages": [
                {
                    "id": 1,
                    "title": "Page 1",
                    "excerpt": "word ".repeat(100)
                },
                {
                    "id": 2,
                    "title": "Page 2",
                    "excerpt": "word ".repeat(100)
                }
            ]
        });

        let (full_prompt, full_bytes) = build_merge_prompt(&skill, &candidate);
        let (trimmed_prompt, trimmed_bytes) = build_merge_prompt_trimmed(&skill, &candidate);

        // Trimmed prompt should be smaller because excerpts are removed
        assert!(
            trimmed_bytes < full_bytes,
            "Trimmed prompt should be smaller: {} < {}",
            trimmed_bytes,
            full_bytes
        );
        assert!(!trimmed_prompt.contains("excerpt"));
        assert!(full_prompt.contains("excerpt"));
    }

    #[test]
    fn test_can_analyze_fails_loudly_on_stale_url_based_skill() {
        // Mutates process-global env — serialize against other env-mutating tests.
        let _env_guard = crate::test_support::ENV_LOCK.lock().unwrap();
        // A stale project-level skill copy using the old keep_url/redirect_urls
        // contract must fail the analyze step with an actionable message instead
        // of silently zeroing every merge recommendation downstream.
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        let auto_dir = Path::new(&path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();
        let skill_dir = Path::new(&path)
            .join(".github")
            .join("skills")
            .join("cannibalization-strategy");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join("SKILL.md"),
            "# Cannibalization Strategy Skill\n\nReturn keep_url and redirect_urls.\n",
        )
        .unwrap();

        let candidates_doc = serde_json::json!({
            "candidates": [{
                "candidate_id": "test_0",
                "theme": "cash secured puts",
                "pages": [{ "id": 1, "url": "/blog/a", "title": "A" }]
            }]
        });
        std::fs::write(
            auto_dir.join("cannibalization_candidates.json"),
            serde_json::to_string_pretty(&candidates_doc).unwrap(),
        )
        .unwrap();

        // Redirect the audit-artifact DB lookup to a throwaway path so the
        // JSON fallback is used and no live app state is touched.
        let old_db = std::env::var("PAGESEEDS_DB_PATH").ok();
        std::env::set_var(
            "PAGESEEDS_DB_PATH",
            Path::new(&path).join("test.db").to_string_lossy().as_ref(),
        );

        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "cannibalization_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_can_analyze_candidates(&task, &path, "mock");

        match old_db {
            Some(v) => std::env::set_var("PAGESEEDS_DB_PATH", v),
            None => std::env::remove_var("PAGESEEDS_DB_PATH"),
        }

        assert!(!result.success, "stale skill must fail the analyze step");
        assert!(
            result.message.contains("keep_id/redirect_ids output contract"),
            "unexpected failure message: {}",
            result.message
        );
        assert!(result.message.contains(".github/skills/cannibalization-strategy/SKILL.md"));

        cleanup(&path);
    }

    #[test]
    fn test_can_reduce_strategy_surfaces_guard_degraded_recommendations() {
        // Guard-degraded no_action recommendations (keep_id/redirect_ids not in
        // the candidate page set) must surface as a risk, not vanish silently.
        // The count comes from the typed `guard_degraded_count` field written by
        // the analyze step — not from sniffing reason prose, so a genuine
        // model-authored no_action reason that happens to mention "keep_id"
        // must NOT be counted.
        let path = test_dir();
        let _ = std::fs::remove_dir_all(&path);
        let auto_dir = Path::new(&path).join(".github").join("automation");
        std::fs::create_dir_all(&auto_dir).unwrap();

        let batch_doc = serde_json::json!({
            "guard_degraded_count": 1,
            "batch_outputs": [
                {
                    "candidate_id": "test_0",
                    "success": true,
                    "message": "ok",
                    "merge_recommendation": {
                        "no_action": true,
                        "reason": "Model returned keep_id=0 which is not in the candidate page set; cannot resolve a canonical keeper URL."
                    }
                },
                {
                    "candidate_id": "test_1",
                    "success": true,
                    "message": "ok",
                    "merge_recommendation": {
                        "no_action": true,
                        "reason": "Neither page is a strong keeper; keep_id selection would be arbitrary, topical overlap only."
                    }
                }
            ]
        });
        std::fs::write(
            auto_dir.join("cannibalization_batch_outputs.json"),
            serde_json::to_string_pretty(&batch_doc).unwrap(),
        )
        .unwrap();
        std::fs::write(
            auto_dir.join("hub_gaps.json"),
            serde_json::json!({ "hub_gaps": [] }).to_string(),
        )
        .unwrap();

        let task = Task {
            id: "task-test".to_string(),
            project_id: "proj-test".to_string(),
            task_type: "cannibalization_audit".to_string(),
            phase: "investigation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Test".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![],
            run: crate::models::task::TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };

        let result = exec_can_reduce_strategy(&task, &path);
        assert!(result.success, "{}", result.message);
        assert!(
            result.message.contains("1 recommendation(s) discarded by id-resolution guard"),
            "unexpected message: {}",
            result.message
        );

        let strategy: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(auto_dir.join("cannibalization_strategy.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            strategy["merge_recommendations"].as_array().unwrap().len(),
            0
        );
        let risks = strategy["risks"].as_array().unwrap();
        assert!(
            risks.iter().any(|r| r
                .as_str()
                .unwrap()
                .contains("1 recommendation(s) discarded: model returned keep_id/redirect_ids not in the candidate page set")),
            "guard-degradation risk missing: {:?}",
            risks
        );

        cleanup(&path);
    }
