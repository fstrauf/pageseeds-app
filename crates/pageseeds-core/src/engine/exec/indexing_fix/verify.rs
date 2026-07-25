//! Step 4 (deterministic): verify every planned change landed.

use crate::engine::workflows::StepResult;
use crate::models::task::Task;

use super::{extract_first_h1, resolve_plan, resolve_target_file};

/// Deterministic step: re-read the file and confirm every planned change
/// actually landed. Fails loudly when the file is unchanged or a planned
/// value is missing — this is what makes silent success impossible.
pub(crate) fn exec_indexing_fix_verify(task: &Task, project_path: &str) -> StepResult {
    let plan = match resolve_plan(task, None) {
        Ok(p) => p,
        Err(result) => return result,
    };

    let file_path = match resolve_target_file(task, project_path) {
        Ok(p) => p,
        Err(result) => return result,
    };

    let content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult::fail(format!("Failed to read {}: {}", file_path.display(), e))
        }
    };

    if let Err(e) = crate::content::cleaner::validate_mdx_structure(&content) {
        return StepResult::fail(format!("MDX structure invalid after fix: {}", e));
    }

    let (fm, body) = match crate::content::frontmatter::split_mdx(&content) {
        Some((f, b)) => (f, b),
        None => {
            return StepResult::fail("Could not parse frontmatter from MDX file".to_string())
        }
    };

    let scalars = crate::content::frontmatter::top_level_scalars(fm);
    let get_scalar = |key: &str| -> String {
        scalars
            .iter()
            .find(|f| f.key == key)
            .map(|f| f.raw_value.trim_matches('"').trim_matches('\'').to_string())
            .unwrap_or_default()
    };

    // Frontmatter scalar checks as (label, key, expected) triples. The label is
    // what shows up in the verified/failed report; the key is the frontmatter
    // scalar to compare against.
    let mut scalar_checks: Vec<(String, &str, &str)> = Vec::new();
    if let Some(ref expected) = plan.changes.title {
        scalar_checks.push(("title".to_string(), "title", expected));
    }
    if let Some(ref expected) = plan.changes.description {
        scalar_checks.push(("description".to_string(), "description", expected));
    }
    if let Some(ref edits) = plan.changes.frontmatter {
        for edit in edits {
            if edit.key == "title" || edit.key == "description" {
                continue; // covered above
            }
            scalar_checks.push((format!("frontmatter:{}", edit.key), &edit.key, &edit.value));
        }
    }

    let mut verified: Vec<String> = Vec::new();
    let mut failed: Vec<String> = Vec::new();

    for (label, key, expected) in &scalar_checks {
        let actual = get_scalar(key);
        if actual == expected.trim() {
            verified.push(label.clone());
        } else {
            failed.push(format!(
                "{}: expected {:?}, found {:?}",
                label,
                crate::engine::text::char_prefix(expected, 60),
                crate::engine::text::char_prefix(&actual, 60)
            ));
        }
    }

    if let Some(ref expected) = plan.changes.h1 {
        let actual = extract_first_h1(&content).unwrap_or_default();
        if actual == expected.trim() {
            verified.push("h1".to_string());
        } else {
            failed.push(format!(
                "h1: expected {:?}, found {:?}",
                crate::engine::text::char_prefix(expected, 60),
                crate::engine::text::char_prefix(&actual, 60)
            ));
        }
    }

    if let Some(ref expected) = plan.changes.intro {
        let first_para = crate::content::cleaner::find_first_paragraph_range(body)
            .map(|(start, end)| body[start..end].trim().to_string())
            .unwrap_or_default();
        if normalize_ws(&first_para) == normalize_ws(expected) {
            verified.push("intro".to_string());
        } else {
            failed.push("intro: first paragraph does not match the planned intro".to_string());
        }
    }

    let report = serde_json::json!({
        "file": file_path.to_string_lossy(),
        "verified": verified,
        "failed": failed,
    });

    if !failed.is_empty() {
        return StepResult::fail_with_output(format!(
                "Fix verification FAILED for {}: {}. The file was not changed as planned.",
                file_path.display(),
                failed.join("; ")
            ), report.to_string());
    }

    if verified.is_empty() {
        return StepResult::fail_with_output(format!(
                "Fix verification FAILED for {}: plan contained no verifiable changes.",
                file_path.display()
            ), report.to_string());
    }

    StepResult {
        success: true,
        message: format!(
            "Verified {} change(s) landed in {}: {}",
            verified.len(),
            file_path.display(),
            verified.join(", ")
        ),
        output: Some(report.to_string()),
        artifact_key: None,
    }
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
