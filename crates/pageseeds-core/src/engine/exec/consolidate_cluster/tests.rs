use std::path::{Path, PathBuf};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::merge_patch::{
    ContentMergePatch, ExtractedExample, ExtractedFaq, ExtractedHeading, ExtractedTable,
    MergePreflightReport, MergeValidationReport, RedirectRule, SectionInventory,
};
use crate::models::task::Task;

use super::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::draft_patch::assemble_merge_prompt;
    use super::extract_sections::{
        cap_keeper_outline, merge_batch_byte_budget, pack_redirect_batches, MAX_PAGES_PER_BATCH,
    };
    use super::*;

    #[test]
    fn test_extract_headings() {
        let body = r#"# Title

## Section One
Some text here.

### Subsection
More text.

## Section Two
Final text.
"#;
        let headings = extract_headings(body);
        assert_eq!(headings.len(), 2);
        assert_eq!(headings[0].text, "Section One");
        assert_eq!(headings[0].level, 2);
        assert_eq!(headings[1].text, "Section Two");
    }

    #[test]
    fn test_extract_tables() {
        let body = r#"# Title

| Col A | Col B |
|-------|-------|
| 1     | 2     |

Some text.
"#;
        let tables = extract_tables(body);
        assert_eq!(tables.len(), 1);
        assert!(tables[0].markdown.contains("Col A"));
    }

    #[test]
    fn test_extract_examples() {
        let body = r#"# Title

```python
print("hello")
```

Some text.
"#;
        let examples = extract_examples(body);
        assert_eq!(examples.len(), 1);
        assert_eq!(examples[0].language.as_deref(), Some("python"));
        assert!(examples[0].code.contains("hello"));
    }

    #[test]
    fn test_extract_faqs() {
        let body = r#"# Title

Q: What is this?
A: It is a test.

Q: Why?
A: Because.
"#;
        let faqs = extract_faqs(body);
        assert_eq!(faqs.len(), 2);
        assert_eq!(faqs[0].question, "What is this?");
        assert_eq!(faqs[0].answer, "It is a test.");
    }

    #[test]
    fn test_merge_preflight_report_roundtrip() {
        let report = MergePreflightReport {
            keeper_file_exists: true,
            keeper_is_indexable: true,
            redirect_files_exist: vec!["/blog/a".to_string()],
            redirect_files_missing: vec![],
            redirect_cycles_detected: vec![],
            can_proceed: true,
            notes: vec!["ok".to_string()],
        };
        let json = serde_json::to_string(&report).unwrap();
        let decoded: MergePreflightReport = serde_json::from_str(&json).unwrap();
        assert!(decoded.can_proceed);
    }

    // ─── Temp project helpers ────────────────────────────────────────────────

    struct TempProject(PathBuf);

    impl TempProject {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "pageseeds_merge_test_{}_{}_{}",
                name,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(dir.join("content")).unwrap();
            TempProject(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn write_content_file(&self, filename: &str, content: &str) -> PathBuf {
            let path = self.0.join("content").join(filename);
            std::fs::write(&path, content).unwrap();
            path
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // ─── find_file_by_slug ───────────────────────────────────────────────────

    #[test]
    fn test_find_file_by_slug_exact_match_wins() {
        let project = TempProject::new("slug_exact");
        project.write_content_file("001_cash_secured_puts.mdx", "---\ntitle: A\n---\n\nBody A\n");
        project.write_content_file(
            "002_cash_secured_puts_best.mdx",
            "---\ntitle: B\n---\n\nBody B\n",
        );

        let found = find_file_by_slug(project.path().to_str().unwrap(), "cash_secured_puts")
            .unwrap()
            .expect("should resolve");
        assert_eq!(found.file_stem().unwrap(), "001_cash_secured_puts");

        let found = find_file_by_slug(project.path().to_str().unwrap(), "cash-secured-puts-best")
            .unwrap()
            .expect("should resolve");
        assert_eq!(found.file_stem().unwrap(), "002_cash_secured_puts_best");
    }

    #[test]
    fn test_find_file_by_slug_ambiguous_fails_loudly() {
        let project = TempProject::new("slug_ambiguous");
        // Both stems normalize to "my-post" — resolution must not guess.
        project.write_content_file("001_my_post.mdx", "---\ntitle: A\n---\n\nBody A\n");
        project.write_content_file("002-my-post.mdx", "---\ntitle: B\n---\n\nBody B\n");

        let result = find_file_by_slug(project.path().to_str().unwrap(), "my-post");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Ambiguous"));
    }

    #[test]
    fn test_find_file_by_slug_frontmatter_fallback() {
        let project = TempProject::new("slug_frontmatter");
        project.write_content_file(
            "zzz_random_name.mdx",
            "---\ntitle: X\nurl_slug: unique-fm-slug\n---\n\nBody\n",
        );

        let found = find_file_by_slug(project.path().to_str().unwrap(), "unique_fm_slug")
            .unwrap()
            .expect("frontmatter url_slug should resolve");
        assert_eq!(found.file_stem().unwrap(), "zzz_random_name");

        let missing = find_file_by_slug(project.path().to_str().unwrap(), "no-such-slug").unwrap();
        assert!(missing.is_none());
    }

    // ─── merge_apply_patch ───────────────────────────────────────────────────

    #[test]
    fn test_apply_patch_invalid_mdx_restores_original() {
        let project = TempProject::new("apply_invalid");
        let original = "---\ntitle: Keeper\n---\n\nFirst body paragraph.\n";
        let keeper = project.write_content_file("001_keeper.mdx", original);

        // Removing the opening frontmatter delimiter makes the MDX invalid.
        let patch = serde_json::json!({
            "keeper_file": keeper.to_string_lossy(),
            "additions": [],
            "transitions": [{"find": "---\n", "replace": ""}],
            "notes": [],
        });

        let result = exec_merge_apply_patch(
            &Task::default(),
            project.path().to_str().unwrap(),
            &patch.to_string(),
        );

        assert!(!result.success, "invalid patch must fail: {}", result.message);
        let on_disk = std::fs::read_to_string(&keeper).unwrap();
        assert_eq!(on_disk, original, "keeper must be byte-identical after failure");
        assert!(
            !keeper.with_extension("mdx.snapshot").exists(),
            "snapshot must be cleaned up"
        );
    }

    #[test]
    fn test_apply_patch_replaces_first_occurrence_only() {
        let project = TempProject::new("apply_first_occurrence");
        let original = "---\ntitle: Keeper\n---\n\nalpha beta first.\n\nalpha beta second.\n";
        let keeper = project.write_content_file("001_keeper.mdx", original);

        let patch = serde_json::json!({
            "keeper_file": keeper.to_string_lossy(),
            "additions": [],
            "transitions": [{"find": "alpha beta", "replace": "gamma delta"}],
            "notes": [],
        });

        let result = exec_merge_apply_patch(
            &Task::default(),
            project.path().to_str().unwrap(),
            &patch.to_string(),
        );

        assert!(result.success, "patch should apply: {}", result.message);
        let on_disk = std::fs::read_to_string(&keeper).unwrap();
        assert!(on_disk.contains("gamma delta first."));
        assert!(
            on_disk.contains("alpha beta second."),
            "second occurrence must be untouched"
        );
        assert!(!keeper.with_extension("mdx.snapshot").exists());
    }

    #[test]
    fn test_apply_patch_multiple_rounds_apply_sequentially() {
        let project = TempProject::new("apply_rounds");
        let original = "---\ntitle: Keeper\n---\n\nKeeper body.\n";
        let keeper = project.write_content_file("001_keeper.mdx", original);

        let patch = serde_json::json!({
            "patches": [
                {
                    "keeper_file": keeper.to_string_lossy(),
                    "additions": [{
                        "heading": "Round One",
                        "content": "Content from round one.",
                        "position": "end",
                        "source_file": "content/002_a.mdx"
                    }],
                    "transitions": [],
                    "notes": []
                },
                {
                    "keeper_file": keeper.to_string_lossy(),
                    "additions": [{
                        "heading": "Round Two",
                        "content": "Content from round two.",
                        "position": "end",
                        "source_file": "content/003_b.mdx"
                    }],
                    "transitions": [],
                    "notes": []
                }
            ]
        });

        let result = exec_merge_apply_patch(
            &Task::default(),
            project.path().to_str().unwrap(),
            &patch.to_string(),
        );

        assert!(result.success, "rounds should apply: {}", result.message);
        let on_disk = std::fs::read_to_string(&keeper).unwrap();
        assert!(on_disk.contains("## Round One"));
        assert!(on_disk.contains("## Round Two"));
        assert!(!keeper.with_extension("mdx.snapshot").exists());
    }

    #[test]
    fn test_apply_patch_failing_later_round_leaves_original_untouched() {
        let project = TempProject::new("apply_atomic");
        let original = "---\ntitle: Keeper\n---\n\nKeeper body.\n";
        let keeper = project.write_content_file("001_keeper.mdx", original);

        // Round 1 is valid; round 2 removes the frontmatter delimiter, making
        // the accumulated result invalid. The apply is atomic: nothing from
        // round 1 may reach the disk.
        let patch = serde_json::json!({
            "patches": [
                {
                    "keeper_file": keeper.to_string_lossy(),
                    "additions": [{
                        "heading": "Round One",
                        "content": "Content from round one.",
                        "position": "end",
                        "source_file": "content/002_a.mdx"
                    }],
                    "transitions": [],
                    "notes": []
                },
                {
                    "keeper_file": keeper.to_string_lossy(),
                    "additions": [],
                    "transitions": [{"find": "---\n", "replace": ""}],
                    "notes": []
                }
            ]
        });

        let result = exec_merge_apply_patch(
            &Task::default(),
            project.path().to_str().unwrap(),
            &patch.to_string(),
        );

        assert!(!result.success, "failing round must fail the apply: {}", result.message);
        let on_disk = std::fs::read_to_string(&keeper).unwrap();
        assert_eq!(on_disk, original, "keeper must be byte-identical after failure");
        assert!(
            !keeper.with_extension("mdx.snapshot").exists(),
            "snapshot must be cleaned up"
        );
    }

    // ─── redirect batching ───────────────────────────────────────────────────

    fn redirect_page(url: &str, word_count: usize, sections: Vec<MergeSection>) -> RedirectPage {
        RedirectPage {
            file: format!("content/{}.mdx", url.trim_start_matches("/blog/")),
            url: url.to_string(),
            title: url.to_string(),
            word_count,
            sections,
            tables: vec![],
            examples: vec![],
            faqs: vec![],
            truncation_note: None,
        }
    }

    #[test]
    fn test_pack_redirect_batches_splits_beyond_five_pages() {
        let pages: Vec<RedirectPage> = (0..7)
            .map(|i| redirect_page(&format!("/blog/page-{}", i), 10, vec![]))
            .collect();

        let batches = pack_redirect_batches(pages, 12_000);

        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), MAX_PAGES_PER_BATCH);
        assert_eq!(batches[1].len(), 2);
        // No page is dropped: every url appears in exactly one batch.
        let urls: Vec<&str> = batches
            .iter()
            .flatten()
            .map(|p| p.url.as_str())
            .collect();
        assert_eq!(urls.len(), 7);
        for i in 0..7 {
            assert!(urls.contains(&format!("/blog/page-{}", i).as_str()));
        }
    }

    #[test]
    fn test_pack_redirect_batches_respects_byte_budget() {
        let big_body = "x".repeat(7_000);
        let pages: Vec<RedirectPage> = (0..3)
            .map(|i| {
                redirect_page(
                    &format!("/blog/big-{}", i),
                    1000,
                    vec![MergeSection {
                        level: 2,
                        text: "Big".to_string(),
                        body: big_body.clone(),
                        covered_by_keeper: false,
                    }],
                )
            })
            .collect();

        let batches = pack_redirect_batches(pages, 12_000);

        // Two 7KB pages exceed a 12KB budget → one page per batch.
        assert_eq!(batches.len(), 3);
        assert!(batches.iter().all(|b| b.len() == 1));
    }

    #[test]
    fn test_merge_batch_byte_budget_accounts_for_overhead() {
        let target = crate::config::prompt_budget::default_prompt_budget().target;
        // No content overhead → only the fixed prompt-assembly margin (1_024)
        // is reserved; the rest of the target budget is available for batches.
        assert_eq!(merge_batch_byte_budget(0, 0, 0), target - 1_024);
        // Overhead shrinks the batch budget one-for-one.
        let budget = merge_batch_byte_budget(5_000, 2_000, 1_500);
        assert_eq!(budget, target - 5_000 - 2_000 - 1_500 - 1_024);
        // Absurd overhead hits the floor instead of underflowing to zero.
        assert_eq!(merge_batch_byte_budget(usize::MAX / 2, 0, 0), 4_000);
    }

    #[test]
    fn test_cap_keeper_outline_caps_entries_and_marks_omission() {
        let outline: Vec<OutlineHeading> = (0..150)
            .map(|i| OutlineHeading {
                level: 2,
                text: format!("Heading {}", i),
            })
            .collect();

        let capped = cap_keeper_outline(outline);

        // 100 real headings + one marker.
        assert_eq!(capped.len(), 101);
        let marker = &capped[capped.len() - 1];
        assert!(
            marker.text.contains("keeper outline truncated: 50 more heading(s) omitted"),
            "marker must record the omission: {}",
            marker.text
        );
    }

    #[test]
    fn test_cap_keeper_outline_caps_bytes() {
        let outline: Vec<OutlineHeading> = (0..50)
            .map(|i| OutlineHeading {
                level: 3,
                text: format!("{:03} {}", i, "x".repeat(200)),
            })
            .collect();

        let capped = cap_keeper_outline(outline);

        assert!(capped.len() < 51, "byte cap must drop headings");
        assert!(capped[capped.len() - 1].text.contains("keeper outline truncated"));
    }

    #[test]
    fn test_pack_redirect_batches_truncates_oversized_single_page() {
        // One page with a giant table plus a long unique section body —
        // far over any realistic per-page budget on its own.
        let mut table = String::from("| Col A | Col B |\n|-------|-------|\n");
        for i in 0..500 {
            table.push_str(&format!("| row {:04} | value {:04} |\n", i, i));
        }
        let mut page = redirect_page(
            "/blog/huge",
            5000,
            vec![MergeSection {
                level: 2,
                text: "Deep Dive".to_string(),
                body: "unique insight ".repeat(500),
                covered_by_keeper: false,
            }],
        );
        page.tables.push(MergeTable { markdown: table });

        let batches = pack_redirect_batches(vec![page], 4_000);

        assert_eq!(batches.len(), 1);
        let page = &batches[0][0];
        let bytes = serde_json::to_string(page).unwrap().len();
        assert!(
            bytes <= 4_000,
            "oversized page must be truncated to fit the budget, got {} bytes",
            bytes
        );
        // Truncation is visible: what was cut and why.
        let note = page.truncation_note.as_ref().expect("truncation_note must be set");
        assert!(note.contains("merge batch budget"), "note explains why: {}", note);
        assert!(note.contains("table row(s)"), "note records cut rows: {}", note);
        // Table keeps its header verbatim and carries a summary marker.
        assert!(page.tables[0].markdown.contains("| Col A | Col B |"));
        assert!(page.tables[0].markdown.contains("[…table truncated:"));
    }

    #[test]
    fn test_extract_sections_large_cluster_batches_within_shared_budget() {
        let project = TempProject::new("extract_large_cluster");
        project.write_content_file(
            "001_keeper.mdx",
            "---\ntitle: Keeper\n---\n\n## Overview\n\nKeeper overview text.\n",
        );
        // Six content-rich redirect pages: ~9 KB of unique body each →
        // ~54 KB of extracted redirect content, well past the old 20 KB guard.
        let body_9k = "unique analysis paragraph. ".repeat(330); // ~9 KB
        let mut redirect_urls = Vec::new();
        for i in 0..6 {
            let slug = format!("redirect-{}", i);
            let mdx = format!(
                "---\ntitle: Redirect {}\n---\n\n## Unique Data {}\n\n{}\n",
                i, i, body_9k
            );
            project.write_content_file(&format!("00{}_{}.mdx", i + 2, slug.replace('-', "_")), &mdx);
            redirect_urls.push(format!("/blog/{}", slug));
        }

        let strategy = serde_json::json!({
            "merge_recommendations": [{
                "cluster_id": "test",
                "keep_url": "/blog/keeper",
                "redirect_urls": redirect_urls
            }]
        });
        let task = Task {
            title: Some("Merge cluster: test".to_string()),
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "cannibalization_strategy".to_string(),
                path: None,
                artifact_type: None,
                source: None,
                content: Some(strategy.to_string()),
            }],
            ..Task::default()
        };

        let result = exec_merge_extract_sections(&task, project.path().to_str().unwrap());
        assert!(result.success, "extract should succeed: {}", result.message);
        let output = result.output.unwrap();

        let context: MergeContext = serde_json::from_str(&output).unwrap();
        assert_eq!(context.total_redirects, 6);
        assert!(
            context.batch_count >= 2,
            "40KB+ of redirect content must be split across batches, got {}",
            context.batch_count
        );
        assert_eq!(context.batch_count, context.batches.len());
        let extracted_bytes: usize = context
            .batches
            .iter()
            .flat_map(|b| b.redirect_pages.iter())
            .map(|p| serde_json::to_string(p).unwrap().len())
            .sum();
        assert!(
            extracted_bytes > 40_000,
            "synthetic cluster should exceed 40 KB, got {} bytes",
            extracted_bytes
        );
        // No page needed truncation: pages fit the overhead-aware budget.
        assert!(
            context
                .batches
                .iter()
                .flat_map(|b| b.redirect_pages.iter())
                .all(|p| p.truncation_note.is_none()),
            "batched pages must not be truncated"
        );

        // Every assembled round prompt stays under the shared hard budget.
        let skill = crate::engine::skills::load_skill_or_fail(project.path(), "merge-content")
            .expect("embedded merge-content skill");
        let hard = crate::config::prompt_budget::default_prompt_budget().hard;
        for batch in &context.batches {
            let round_context = serde_json::to_string(&MergeRoundContext {
                keeper_file: &context.keeper_file,
                keeper_outline: &context.keeper_outline,
                keeper_excerpt: &context.keeper_excerpt,
                total_redirects: context.total_redirects,
                batch_count: context.batch_count,
                batch_index: batch.batch_index,
                redirect_pages: &batch.redirect_pages,
            })
            .unwrap();
            let prompt = assemble_merge_prompt(&skill.content, &round_context);
            assert!(
                prompt.len() <= hard,
                "batch {} prompt ({} bytes) exceeds shared hard budget ({} bytes)",
                batch.batch_index,
                prompt.len(),
                hard
            );
        }
    }

    #[test]
    fn test_extract_sections_sends_full_unique_section_bodies() {
        let project = TempProject::new("extract_full_bodies");
        project.write_content_file(
            "001_keeper.mdx",
            "---\ntitle: Keeper\n---\n\n## Overview\n\nKeeper overview text.\n",
        );
        // Unique content sits past the old 200-char excerpt cutoff.
        let padding = "Intro paragraph text. ".repeat(20);
        let redirect_body = format!(
            "---\ntitle: Redirect\n---\n\n## Overview\n\n{}\n\n## Unique Data\n\n| Col A | Col B |\n|-------|-------|\n| 1     | 2     |\n",
            padding
        );
        project.write_content_file("002_redirect.mdx", &redirect_body);

        let strategy = serde_json::json!({
            "merge_recommendations": [{
                "cluster_id": "test",
                "keep_url": "/blog/keeper",
                "redirect_urls": ["/blog/redirect"]
            }]
        });
        let task = Task {
            title: Some("Merge cluster: test".to_string()),
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "cannibalization_strategy".to_string(),
                path: None,
                artifact_type: None,
                source: None,
                content: Some(strategy.to_string()),
            }],
            ..Task::default()
        };

        let result = exec_merge_extract_sections(&task, project.path().to_str().unwrap());

        assert!(result.success, "extract should succeed: {}", result.message);
        let output = result.output.unwrap();
        // The actual table content must reach the merge prompt, not an excerpt.
        assert!(output.contains("| Col A | Col B |"));
        // No excerpt/truncation fields from the old implementation.
        assert!(!output.contains("\"truncated\""));
        assert!(!output.contains("\"excerpt\""));
        // Single redirect page → exactly one batch.
        let doc: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(doc["batch_count"].as_u64(), Some(1));
        assert_eq!(doc["total_redirects"].as_u64(), Some(1));
    }

    // ─── depublish redirect sources ──────────────────────────────────────

    struct DepublishFixture {
        dir: PathBuf,
        conn: rusqlite::Connection,
        task: Task,
    }

    /// Temp project with a published keeper + a published redirect source,
    /// an in-memory DB with matching rows, and a merge task whose plan
    /// redirects `/blog/old-post` into `/blog/keeper`.
    fn depublish_fixture(name: &str) -> DepublishFixture {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds_depublish_{}_{}_{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("content")).unwrap();
        std::fs::create_dir_all(dir.join(".github").join("automation")).unwrap();
        std::fs::write(
            dir.join("content").join("001_keeper.mdx"),
            "---\ntitle: Keeper\nstatus: published\ndate: \"2024-01-01\"\n---\n\nKeeper body.\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("content").join("002_old_post.mdx"),
            "---\ntitle: Old\nstatus: published\ndate: \"2024-01-02\"\n---\n\nOld body.\n",
        )
        .unwrap();

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('p1', 'Test', ?1, 1, 'workspace')",
            [dir.to_str().unwrap()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles (id, title, url_slug, file, status, published_date, content_gaps_addressed, project_id)
             VALUES (1, 'Keeper', 'keeper', './content/001_keeper.mdx', 'published', '2024-01-01', '[]', 'p1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO articles (id, title, url_slug, file, status, published_date, content_gaps_addressed, project_id)
             VALUES (2, 'Old', 'old-post', './content/002_old_post.mdx', 'published', '2024-01-02', '[]', 'p1')",
            [],
        )
        .unwrap();

        let strategy = serde_json::json!({
            "merge_recommendations": [{
                "cluster_id": "test",
                "keep_url": "/blog/keeper",
                "redirect_urls": ["/blog/old-post"]
            }]
        });
        let task = Task {
            project_id: "p1".to_string(),
            title: Some("Merge cluster: test".to_string()),
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "cannibalization_strategy".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("cannibalization_audit".to_string()),
                content: Some(strategy.to_string()),
            }],
            ..Task::default()
        };

        DepublishFixture { dir, conn, task }
    }

    #[test]
    fn test_depublish_redirect_sources_marks_db_frontmatter_and_articles_json() {
        let fx = depublish_fixture("marks");

        let depublished =
            depublish_redirect_sources(&fx.task, fx.dir.to_str().unwrap(), &fx.conn).unwrap();
        assert_eq!(depublished, 1);

        // Frontmatter status updated; file stays on disk.
        let source_mdx =
            std::fs::read_to_string(fx.dir.join("content").join("002_old_post.mdx")).unwrap();
        assert!(
            source_mdx.contains("status: \"redirected\""),
            "frontmatter must be redirected: {}",
            source_mdx
        );
        assert!(source_mdx.contains("Old body."), "content must be kept");

        // SQLite row updated.
        let db_status: String = fx
            .conn
            .query_row("SELECT status FROM articles WHERE id = 2", [], |r| r.get(0))
            .unwrap();
        assert_eq!(db_status, "redirected");

        // Keeper untouched on disk and in the DB.
        let keeper_mdx =
            std::fs::read_to_string(fx.dir.join("content").join("001_keeper.mdx")).unwrap();
        assert!(keeper_mdx.contains("status: published"));
        let keeper_status: String = fx
            .conn
            .query_row("SELECT status FROM articles WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(keeper_status, "published");

        // articles.json export reflects the new state.
        crate::db::export::write_articles_to_repo(&fx.conn, "p1", &fx.dir).unwrap();
        let json = std::fs::read_to_string(
            fx.dir.join(".github").join("automation").join("articles.json"),
        )
        .unwrap();
        let doc: serde_json::Value = serde_json::from_str(&json).unwrap();
        let articles = doc["articles"].as_array().unwrap();
        let old = articles
            .iter()
            .find(|a| a["id"].as_i64() == Some(2))
            .unwrap();
        assert_eq!(old["status"].as_str(), Some("redirected"));
        let keeper = articles
            .iter()
            .find(|a| a["id"].as_i64() == Some(1))
            .unwrap();
        assert_eq!(keeper["status"].as_str(), Some("published"));

        let _ = std::fs::remove_dir_all(&fx.dir);
    }

    #[test]
    fn test_depublish_redirect_sources_fails_loudly_on_missing_file() {
        let fx = depublish_fixture("missing");
        std::fs::remove_file(fx.dir.join("content").join("002_old_post.mdx")).unwrap();

        let err = depublish_redirect_sources(&fx.task, fx.dir.to_str().unwrap(), &fx.conn)
            .unwrap_err();
        assert!(
            err.contains("old-post"),
            "error must name the failing slug: {}",
            err
        );

        let _ = std::fs::remove_dir_all(&fx.dir);
    }
}
