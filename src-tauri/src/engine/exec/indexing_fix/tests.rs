//! Tests for the indexing fix pipeline (moved from the monolithic
//! `indexing_fix.rs` during the per-step module split).

use super::*;
use crate::models::task::{
    AgentPolicy, FollowUpPolicy, Priority, TaskArtifact, TaskReviewSurface, TaskRunPolicy,
    TaskStatus,
};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn unique_temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
}

fn dummy_task(description: &str, artifacts: Vec<TaskArtifact>) -> Task {
    Task {
        id: "fix-task-1".to_string(),
        task_type: "fix_indexing".to_string(),
        phase: "implementation".to_string(),
        status: TaskStatus::InProgress,
        priority: Priority::Medium,
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        agent_policy: AgentPolicy::Required,
        title: Some("Fix indexing".to_string()),
        description: Some(description.to_string()),
        project_id: "proj-1".to_string(),
        depends_on: vec![],
        artifacts,
        run: crate::models::task::TaskRun::default(),
        not_before: None,
        created_at: "2024-01-01T00:00:00Z".to_string(),
        updated_at: "2024-01-01T00:00:00Z".to_string(),
    }
}

const SAMPLE_MDX: &str = "---\ntitle: Old Title\ndescription: Old meta description here.\ndate: 2024-01-01\n---\n\n# Old Heading\n\nFirst paragraph of the article.\n\n## Section\n\nMore text.\n";

fn write_sample_project(dir: &Path) -> PathBuf {
    // locator auto-discovers `content/` when it contains markdown
    let content_dir = dir.join("content");
    std::fs::create_dir_all(&content_dir).unwrap();
    let file = content_dir.join("test-article.mdx");
    std::fs::write(&file, SAMPLE_MDX).unwrap();
    file
}

fn plan_artifact(plan: &IndexingFixPlan) -> TaskArtifact {
    TaskArtifact {
        key: "indexing_fix_plan".to_string(),
        path: None,
        artifact_type: Some("json".to_string()),
        source: Some("indexing_fix_generate".to_string()),
        content: Some(serde_json::to_string_pretty(plan).unwrap()),
    }
}

// ─── Description parsing (Fix 2) ────────────────────────────────────────

#[test]
fn parse_campaign_rewrite_description() {
    let desc = "URL: https://example.com/blog/test-article\n\
                Recommended action: rewrite_title_h1\n\
                Reason: Shares H2s with sibling\n\
                Parent campaign: camp-1\n\
                Suggested title: Better Title\n\
                Suggested H1: Better H1";
    let parsed = parse_fix_task_description(desc);
    assert_eq!(parsed.url, "https://example.com/blog/test-article");
    assert_eq!(parsed.recommended_action.as_deref(), Some("rewrite_title_h1"));
    assert_eq!(parsed.reason.as_deref(), Some("Shares H2s with sibling"));
    assert_eq!(parsed.suggested_title.as_deref(), Some("Better Title"));
    assert_eq!(parsed.suggested_h1.as_deref(), Some("Better H1"));
    assert_eq!(parsed.issue, None);
}

#[test]
fn parse_diagnostics_description() {
    let desc = "URL: https://example.com/blog/test-article\n\
                Issue: not_indexed_crawled\n\
                Action: improve content\n\
                Verdict: crawled_but_not_indexed";
    let parsed = parse_fix_task_description(desc);
    assert_eq!(parsed.url, "https://example.com/blog/test-article");
    assert_eq!(parsed.issue.as_deref(), Some("not_indexed_crawled"));
    assert_eq!(parsed.action.as_deref(), Some("improve content"));
    assert_eq!(parsed.verdict.as_deref(), Some("crawled_but_not_indexed"));
    assert_eq!(parsed.recommended_action, None);
}

#[test]
fn parse_description_matches_any_line_not_fixed_index() {
    // "Recommended action:" on line 1 must not be misread as "Issue: ".
    let desc = "URL: https://example.com/blog/a\nRecommended action: rewrite_title_h1";
    let parsed = parse_fix_task_description(desc);
    assert_eq!(parsed.issue, None);
    assert_eq!(parsed.recommended_action.as_deref(), Some("rewrite_title_h1"));
}

#[test]
fn context_step_emits_suggestions() {
    let dir = unique_temp_dir("ifix_ctx");
    write_sample_project(&dir);
    let task = dummy_task(
        "URL: https://example.com/blog/test-article\n\
         Recommended action: rewrite_title_h1\n\
         Reason: overlap\n\
         Suggested title: Better Title\n\
         Suggested H1: Better H1",
        vec![],
    );
    let result = exec_indexing_fix_context(&task, dir.to_str().unwrap());
    assert!(result.success, "context step failed: {}", result.message);
    let ctx: IndexingFixContext = serde_json::from_str(&result.output.unwrap()).unwrap();
    assert_eq!(ctx.recommended_action.as_deref(), Some("rewrite_title_h1"));
    assert_eq!(ctx.suggested_title.as_deref(), Some("Better Title"));
    assert_eq!(ctx.suggested_h1.as_deref(), Some("Better H1"));
    assert_eq!(ctx.title.as_deref(), Some("Old Title"));
    assert!(ctx.exists);
}

// ─── Apply step (deterministic write) ────────────────────────────────────

#[test]
fn apply_step_edits_file() {
    let dir = unique_temp_dir("ifix_apply");
    let file = write_sample_project(&dir);
    let plan = IndexingFixPlan {
        diagnosis: "overlap".to_string(),
        changes: IndexingFixChanges {
            title: Some("Better Title".to_string()),
            h1: Some("Better H1".to_string()),
            description: Some("A sharper meta description.".to_string()),
            intro: None,
            frontmatter: None,
        },
    };
    let task = dummy_task(
        "URL: https://example.com/blog/test-article\nRecommended action: rewrite_title_h1",
        vec![plan_artifact(&plan)],
    );

    let result = exec_indexing_fix_apply(&task, dir.to_str().unwrap(), None);
    assert!(result.success, "apply failed: {}", result.message);

    let updated = std::fs::read_to_string(&file).unwrap();
    assert!(updated.contains("Better Title"), "title not updated:\n{}", updated);
    assert!(updated.contains("# Better H1"), "H1 not updated:\n{}", updated);
    assert!(
        updated.contains("A sharper meta description."),
        "description not updated:\n{}",
        updated
    );
    // No leftover snapshot
    assert!(!file.with_extension("mdx.snapshot").exists());
}

#[test]
fn apply_step_fails_loudly_when_plan_has_no_changes() {
    let dir = unique_temp_dir("ifix_noop");
    write_sample_project(&dir);
    let plan = IndexingFixPlan {
        diagnosis: "none".to_string(),
        changes: IndexingFixChanges::default(),
    };
    let task = dummy_task(
        "URL: https://example.com/blog/test-article",
        vec![plan_artifact(&plan)],
    );

    let result = exec_indexing_fix_apply(&task, dir.to_str().unwrap(), None);
    assert!(
        !result.success,
        "apply must fail loudly when the plan contains no changes"
    );
    assert!(result.message.contains("no changes"));
}

#[test]
fn apply_step_fails_loudly_when_no_effective_change() {
    let dir = unique_temp_dir("ifix_noop2");
    // Fixture with quoted frontmatter values so replacing title with the
    // identical value produces byte-identical content.
    let content_dir = dir.join("content");
    std::fs::create_dir_all(&content_dir).unwrap();
    let file = content_dir.join("test-article.mdx");
    std::fs::write(
        &file,
        "---\ntitle: \"Old Title\"\ndescription: \"Old meta description here.\"\ndate: 2024-01-01\n---\n\n# Old Heading\n\nFirst paragraph of the article.\n",
    )
    .unwrap();
    let plan = IndexingFixPlan {
        diagnosis: "none".to_string(),
        changes: IndexingFixChanges {
            title: Some("Old Title".to_string()), // identical to current
            h1: None,
            description: None,
            intro: None,
            frontmatter: None,
        },
    };
    let task = dummy_task(
        "URL: https://example.com/blog/test-article",
        vec![plan_artifact(&plan)],
    );

    let result = exec_indexing_fix_apply(&task, dir.to_str().unwrap(), None);
    assert!(
        !result.success,
        "apply must fail loudly when the plan changes nothing"
    );
    assert!(result.message.contains("no effective change"));
}

// ─── Verify step ─────────────────────────────────────────────────────────

#[test]
fn verify_step_fails_when_file_unchanged() {
    let dir = unique_temp_dir("ifix_verify_fail");
    write_sample_project(&dir);
    // Plan demands a new title, but the file was never edited.
    let plan = IndexingFixPlan {
        diagnosis: "overlap".to_string(),
        changes: IndexingFixChanges {
            title: Some("Better Title".to_string()),
            h1: Some("Better H1".to_string()),
            description: None,
            intro: None,
            frontmatter: None,
        },
    };
    let task = dummy_task(
        "URL: https://example.com/blog/test-article",
        vec![plan_artifact(&plan)],
    );

    let result = exec_indexing_fix_verify(&task, dir.to_str().unwrap());
    assert!(
        !result.success,
        "verify must fail loudly when the planned change never landed"
    );
    assert!(result.message.contains("verification FAILED"));
}

#[test]
fn verify_step_passes_after_apply() {
    let dir = unique_temp_dir("ifix_verify_ok");
    write_sample_project(&dir);
    let plan = IndexingFixPlan {
        diagnosis: "overlap".to_string(),
        changes: IndexingFixChanges {
            title: Some("Better Title".to_string()),
            h1: Some("Better H1".to_string()),
            description: Some("A sharper meta description.".to_string()),
            intro: None,
            frontmatter: None,
        },
    };
    let task = dummy_task(
        "URL: https://example.com/blog/test-article",
        vec![plan_artifact(&plan)],
    );

    let applied = exec_indexing_fix_apply(&task, dir.to_str().unwrap(), None);
    assert!(applied.success, "apply failed: {}", applied.message);

    let verified = exec_indexing_fix_verify(&task, dir.to_str().unwrap());
    assert!(verified.success, "verify failed: {}", verified.message);
}
