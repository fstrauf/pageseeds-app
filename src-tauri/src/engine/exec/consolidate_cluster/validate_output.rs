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
// Step 7: Validate Output
// ═══════════════════════════════════════════════════════════════════════════════

/// Validate the merged keeper and redirect map.
pub(crate) fn exec_merge_validate_output(task: &Task, project_path: &str) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);

    let plan_json = load_plan_from_task_or_file(task, project_path);
    let plan: serde_json::Value = match serde_json::from_str(&plan_json) {
        Ok(v) => v,
        Err(e) => {
            return StepResult {
                success: false,
                message: format!("Invalid merge plan JSON: {}", e),
                output: None,
            };
        }
    };

    let keep_url = plan["keep_url"].as_str().unwrap_or("");
    let keeper_slug = keep_url
        .trim_start_matches("/blog/")
        .trim_start_matches('/');
    let keeper_file = find_file_by_slug(project_path, keeper_slug);

    let mut issues: Vec<String> = Vec::new();

    let keeper_valid = keeper_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|content| {
            let validation = crate::content::cleaner::validate_mdx_structure(&content);
            if let Err(e) = &validation {
                issues.push(format!("keeper: {}", e));
            }
            validation.is_ok()
        })
        .unwrap_or_else(|| {
            issues.push("Keeper file not found".to_string());
            false
        });

    let word_count = keeper_file
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|c| crate::content::ops::count_words(&c))
        .unwrap_or(0);

    let csv_path = paths.automation_dir.join("redirects.csv");
    let has_redirect_map = csv_path.exists();
    if !has_redirect_map {
        issues.push("No redirects.csv found".to_string());
    }

    // Assert no remaining inbound links point at redirected slugs — the
    // merge_rewrite_inbound_links step must have rewritten them all.
    let redirect_sources: std::collections::HashSet<String> = plan["redirect_urls"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(crate::content::slug::normalize_url_slug)
                .collect()
        })
        .unwrap_or_default();
    if !redirect_sources.is_empty() {
        if let Some(content_dir) =
            crate::content::locator::resolve(&paths.repo_root, None).selected
        {
            for file in crate::content::locator::collect_markdown_files(&content_dir) {
                let Ok(content) = std::fs::read_to_string(&file) else {
                    continue;
                };
                let file_name = file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                for (_anchor, raw_href, slug_written) in
                    crate::content::linking::extract_blog_link_hrefs(&content)
                {
                    let normalized = crate::content::slug::normalize_url_slug(&slug_written);
                    if redirect_sources.contains(&normalized) {
                        issues.push(format!(
                            "{}: link '{}' still points to redirected slug '{}'",
                            file_name, raw_href, normalized
                        ));
                    }
                }
            }
        }
    }

    let report = MergeValidationReport {
        keeper_valid,
        keeper_word_count: word_count,
        redirect_map_path: Some(csv_path.to_string_lossy().to_string()),
        issues: issues.clone(),
    };

    let all_ok = keeper_valid && has_redirect_map && issues.is_empty();

    StepResult {
        success: all_ok,
        message: if all_ok {
            "Merge validation passed".to_string()
        } else {
            format!("Merge validation found {} issues", issues.len())
        },
        output: Some(serde_json::to_string_pretty(&report).unwrap_or_default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-validate-output-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(dir.join("content").join("blog")).unwrap();
        std::fs::create_dir_all(dir.join(".github").join("automation")).unwrap();
        dir
    }

    fn merge_task() -> Task {
        let now = chrono::Utc::now().to_rfc3339();
        Task {
            id: "task-1".to_string(),
            project_id: "p1".to_string(),
            task_type: "consolidate_cluster".to_string(),
            phase: "implementation".to_string(),
            status: crate::models::task::TaskStatus::InProgress,
            priority: crate::models::task::Priority::Medium,
            run_policy: crate::models::task::TaskRunPolicy::UserEnqueue,
            review_surface: crate::models::task::TaskReviewSurface::None,
            follow_up_policy: crate::models::task::FollowUpPolicy::None,
            agent_policy: crate::models::task::AgentPolicy::None,
            title: Some("Merge cluster: cluster-1".to_string()),
            description: None,
            depends_on: vec![],
            artifacts: vec![crate::models::task::TaskArtifact {
                key: "cannibalization_strategy".to_string(),
                path: None,
                artifact_type: Some("json".to_string()),
                source: Some("cannibalization_audit".to_string()),
                content: Some(
                    serde_json::json!({
                        "merge_recommendations": [{
                            "cluster_id": "cluster-1",
                            "keep_url": "/blog/hub-coffee",
                            "redirect_urls": ["/blog/old-post"]
                        }]
                    })
                    .to_string(),
                ),
            }],
            run: crate::models::task::TaskRun::default(),
            created_at: now.clone(),
            updated_at: now,
            not_before: None,
        }
    }

    fn write_keeper_and_redirect_map(dir: &Path) {
        let content_dir = dir.join("content").join("blog");
        std::fs::write(
            content_dir.join("hub-coffee.mdx"),
            "---\ntitle: Hub\n---\n\n# Hub\n\nKeeper body text.\n",
        )
        .unwrap();
        std::fs::write(
            dir.join(".github")
                .join("automation")
                .join("redirects.csv"),
            "source,destination,status\n/blog/old-post,/blog/hub-coffee,301\n",
        )
        .unwrap();
    }

    #[test]
    fn flags_remaining_links_to_redirected_slugs() {
        let dir = temp_project();
        write_keeper_and_redirect_map(&dir);
        std::fs::write(
            dir.join("content").join("blog").join("1_post.mdx"),
            "---\ntitle: P\n---\n\nBody [old](/blog/12_old_post) link.\n",
        )
        .unwrap();

        let task = merge_task();
        let result = exec_merge_validate_output(&task, dir.to_str().unwrap());

        assert!(!result.success, "leftover link must fail validation");
        let output = result.output.unwrap();
        assert!(
            output.contains("1_post.mdx") && output.contains("old-post"),
            "issue lists file and slug: {}",
            output
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn passes_when_no_links_to_redirected_slugs_remain() {
        let dir = temp_project();
        write_keeper_and_redirect_map(&dir);
        std::fs::write(
            dir.join("content").join("blog").join("1_post.mdx"),
            "---\ntitle: P\n---\n\nBody [hub](/blog/hub-coffee) link.\n",
        )
        .unwrap();

        let task = merge_task();
        let result = exec_merge_validate_output(&task, dir.to_str().unwrap());

        assert!(result.success, "validation failed unexpectedly: {}", result.message);

        let _ = std::fs::remove_dir_all(&dir);
    }
}

