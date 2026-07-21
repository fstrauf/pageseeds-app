use std::collections::HashSet;
use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::models::task::Task;

use super::*;
// ═══════════════════════════════════════════════════════════════════════════════
// Step 7: Rewrite Inbound Links
// ═══════════════════════════════════════════════════════════════════════════════

/// Rewrite every `/blog/` link that points at a redirected (merged-away) slug
/// to the keeper URL, across all MDX files in the content dir.
///
/// Runs between `merge_generate_redirects` and `merge_validate_output` and
/// implements the "Update internal links" part of the consolidation contract
/// (docs/BUSINESS_PROCESSES.md). Redirect rules come from the approved merge
/// plan: every `redirect_urls` entry → `keep_url`.
///
/// Deterministic: the rewrite mapping (source slug → destination) is fully
/// given by the plan — no judgment involved.
pub(crate) fn exec_merge_rewrite_inbound_links(task: &Task, project_path: &str) -> StepResult {
    let plan_json = load_plan_from_task_or_file(task, project_path);
    let plan: serde_json::Value = match serde_json::from_str(&plan_json) {
        Ok(v) => v,
        Err(e) => {
            return StepResult::fail(format!("Invalid merge plan JSON: {}", e));
        }
    };

    let keep_url = plan["keep_url"].as_str().unwrap_or("");
    let redirect_urls: Vec<String> = plan["redirect_urls"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|v| v.as_str().map(|s| s.to_string()))
        .collect();

    if keep_url.is_empty() || redirect_urls.is_empty() {
        return StepResult {
            success: true,
            message: "No redirect rules in merge plan — nothing to rewrite".to_string(),
            output: None,
        };
    }

    let destination = crate::content::slug::format_blog_link(keep_url);
    let source_slugs: HashSet<String> = redirect_urls
        .iter()
        .map(|s| crate::content::slug::normalize_url_slug(s))
        .collect();

    let content_dir = match crate::content::locator::resolve(Path::new(project_path), None).selected
    {
        Some(d) => d,
        None => {
            return StepResult::fail("Could not locate content directory".to_string())
        }
    };

    match rewrite_links_to_redirected_slugs(&content_dir, &source_slugs, &destination) {
        Ok((total, files)) => {
            let summary = serde_json::json!({
                "destination": destination,
                "source_slugs": source_slugs.iter().collect::<Vec<_>>(),
                "total_rewrites": total,
                "files": files,
            });
            StepResult {
                success: true,
                message: if total > 0 {
                    format!(
                        "Rewrote {} inbound link(s) to {} across {} file(s)",
                        total,
                        destination,
                        files.len()
                    )
                } else {
                    "No inbound links to redirected slugs found — nothing to rewrite".to_string()
                },
                output: Some(serde_json::to_string_pretty(&summary).unwrap_or_default()),
            }
        }
        Err(e) => StepResult::fail(e),
    }
}

/// Core rewrite loop, split out for unit testing.
///
/// Returns `(total_rewrites, per-file summaries)`. Counts distinct rewritten
/// hrefs per file (every occurrence of each href is replaced).
fn rewrite_links_to_redirected_slugs(
    content_dir: &Path,
    source_slugs: &HashSet<String>,
    destination: &str,
) -> Result<(usize, Vec<serde_json::Value>), String> {
    let matches = crate::content::linking::find_links_to_slugs(content_dir, source_slugs);

    // Group matched hrefs into per-file repair maps, preserving traversal
    // order (matches for one file are consecutive).
    let mut per_file: Vec<(std::path::PathBuf, std::collections::HashMap<String, String>)> =
        Vec::new();
    for m in matches {
        match per_file.last_mut() {
            Some((file, repairs)) if *file == m.file => {
                repairs.insert(m.raw_href, destination.to_string());
            }
            _ => per_file.push((
                m.file,
                [(m.raw_href, destination.to_string())].into_iter().collect(),
            )),
        }
    }

    let mut total = 0usize;
    let mut files: Vec<serde_json::Value> = Vec::new();

    for (file, repairs) in per_file {
        let Ok(content) = std::fs::read_to_string(&file) else {
            continue;
        };

        let repaired = crate::content::linking::repair_blog_link_hrefs(&content, &repairs);
        std::fs::write(&file, repaired)
            .map_err(|e| format!("Failed to write {}: {}", file.display(), e))?;

        total += repairs.len();
        files.push(serde_json::json!({
            "file": file.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            "rewrites": repairs.len(),
        }));
    }

    Ok((total, files))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_project() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-rewrite-inbound-{}",
            uuid::Uuid::new_v4()
        ));
        // `content/blog` is the first locator candidate.
        std::fs::create_dir_all(dir.join("content").join("blog")).unwrap();
        std::fs::create_dir_all(dir.join(".github").join("automation")).unwrap();
        dir
    }

    fn sources(slugs: &[&str]) -> HashSet<String> {
        slugs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn rewrites_all_source_slug_variants() {
        let dir = temp_project();
        let content_dir = dir.join("content").join("blog");
        std::fs::write(
            content_dir.join("1_post.mdx"),
            "---\ntitle: P\n---\n\n\
             [underscore](/blog/248_old_post) and \
             [plain](/blog/old-post) and \
             [slash](/blog/old-post/) and \
             [other](/blog/unrelated-post)\n",
        )
        .unwrap();

        let (total, files) = rewrite_links_to_redirected_slugs(
            &content_dir,
            &sources(&["old-post"]),
            "/blog/hub-coffee",
        )
        .unwrap();

        assert_eq!(total, 3, "underscore, plain, trailing-slash forms rewritten");
        assert_eq!(files.len(), 1);
        let written = std::fs::read_to_string(content_dir.join("1_post.mdx")).unwrap();
        assert_eq!(written.matches("[underscore](/blog/hub-coffee)").count(), 1);
        assert!(written.contains("[plain](/blog/hub-coffee)"));
        assert!(written.contains("[slash](/blog/hub-coffee)"));
        assert!(written.contains("[other](/blog/unrelated-post)"));
        assert!(!written.contains("old_post"));
        assert!(!written.contains("old-post"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_matching_links_is_noop() {
        let dir = temp_project();
        let content_dir = dir.join("content").join("blog");
        let original = "[a](/blog/some-post)\n";
        std::fs::write(content_dir.join("1_post.mdx"), original).unwrap();

        let (total, files) = rewrite_links_to_redirected_slugs(
            &content_dir,
            &sources(&["old-post"]),
            "/blog/hub-coffee",
        )
        .unwrap();

        assert_eq!(total, 0);
        assert!(files.is_empty());
        assert_eq!(
            std::fs::read_to_string(content_dir.join("1_post.mdx")).unwrap(),
            original
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn exec_step_rewrites_via_merge_plan() {
        let dir = temp_project();
        let content_dir = dir.join("content").join("blog");
        std::fs::write(
            content_dir.join("1_post.mdx"),
            "---\ntitle: P\n---\n\n[old](/blog/12_old_post)\n",
        )
        .unwrap();

        let now = chrono::Utc::now().to_rfc3339();
        let task = Task {
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
        };

        let result = exec_merge_rewrite_inbound_links(&task, dir.to_str().unwrap());
        assert!(result.success, "step failed: {}", result.message);
        assert!(result.message.contains("Rewrote 1 inbound link(s)"));
        let written = std::fs::read_to_string(content_dir.join("1_post.mdx")).unwrap();
        assert!(written.contains("[old](/blog/hub-coffee)"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
