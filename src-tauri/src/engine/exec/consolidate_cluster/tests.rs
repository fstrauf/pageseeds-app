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
    use super::extract_sections::{pack_redirect_batches, MAX_PAGES_PER_BATCH};
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
        }
    }

    #[test]
    fn test_pack_redirect_batches_splits_beyond_five_pages() {
        let pages: Vec<RedirectPage> = (0..7)
            .map(|i| redirect_page(&format!("/blog/page-{}", i), 10, vec![]))
            .collect();

        let batches = pack_redirect_batches(pages);

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

        let batches = pack_redirect_batches(pages);

        // Two 7KB pages exceed the 12KB budget → one page per batch.
        assert_eq!(batches.len(), 3);
        assert!(batches.iter().all(|b| b.len() == 1));
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
}
