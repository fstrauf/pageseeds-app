//! Quality gate execution for the `review_article_quality` task.
//!
//! 1. `exec_content_quality_context` loads the MDX file written by the parent
//!    `write_article` task and returns a structured context JSON.
//! 2. `exec_content_quality_review` runs a Rig Extractor against the new
//!    `content-quality-review` skill and persists the result to
//!    `article_quality_reviews`.

use std::path::Path;

use crate::engine::workflows::StepResult;
use crate::models::content_review::ContentQualityReview;
use crate::models::task::Task;
use crate::rig::provider::LlmBackend;

const MAX_BODY_EXCERPT_CHARS: usize = 4_000;

/// Resolve the article file to review from task artifacts or the parent task state.
///
/// Priority:
/// 1. `article_file` artifact passed by the spawning post-action.
/// 2. Most recently created/updated article for this project that is still a draft.
fn resolve_target_file(task: &Task, project_path: &str) -> Option<String> {
    // 1. Artifact from parent/spawner.
    if let Some(artifact) = task.artifacts.iter().find(|a| a.key == "article_file") {
        if let Some(file) = artifact.content.as_deref() {
            if !file.is_empty() {
                return Some(file.to_string());
            }
        }
    }

    // 2. Fallback: parse "File: ..." from description.
    if let Some(desc) = task.description.as_deref() {
        if let Some(start) = desc.find("File: ") {
            let rest = &desc[start + 6..];
            let end = rest.find(" |").or_else(|| rest.find('\n')).unwrap_or(rest.len());
            let file = rest[..end].trim();
            if !file.is_empty() {
                return Some(file.to_string());
            }
        }
    }

    // 3. Fallback: most recent draft article in the project.
    let db = match rusqlite::Connection::open(crate::db::default_db_path()) {
        Ok(conn) => conn,
        Err(_) => return None,
    };
    let row: Result<String, rusqlite::Error> = db.query_row(
        "SELECT file FROM articles
         WHERE project_id = ?1 AND file IS NOT NULL AND file != ''
         ORDER BY COALESCE(updated_at, created_at) DESC
         LIMIT 1",
        rusqlite::params![&task.project_id],
        |r| r.get(0),
    );
    row.ok()
}

/// Deterministic context step: load the MDX file and build a focused review context.
pub(crate) fn exec_content_quality_context(
    task: &Task,
    project_path: &str,
) -> StepResult {
    let Some(file) = resolve_target_file(task, project_path) else {
        return StepResult::fail("Could not resolve the article file to review".to_string());
    };

    let repo_root = Path::new(project_path);
    let Some(full_path) = crate::engine::exec::audit_health::resolve_content_file(repo_root, &file)
    else {
        return StepResult::fail(format!("Article file not found: {}", file));
    };

    let content = match std::fs::read_to_string(&full_path) {
        Ok(c) => c,
        Err(e) => {
            return StepResult::fail(format!("Failed to read {}: {}", file, e));
        }
    };

    let (fm_text, body) = crate::content::frontmatter::split_mdx(&content)
        .unwrap_or(("", content.as_str()));
    let frontmatter = crate::content::frontmatter::parse(fm_text)
        .map(|f| f.parsed)
        .unwrap_or_else(|_| serde_yaml::Value::Mapping(Default::default()));

    let title = yaml_string(&frontmatter, "title");
    let description = yaml_string(&frontmatter, "description");
    let h1 = yaml_string(&frontmatter, "h1");
    let target_keyword = yaml_string(&frontmatter, "target_keyword");
    let slug = yaml_string(&frontmatter, "slug");
    let canonical = yaml_string(&frontmatter, "canonical");
    let image = yaml_string(&frontmatter, "image");

    let word_count = crate::content::ops::count_words(body);
    let internal_links = extract_internal_links(body);
    let body_excerpt = body.chars().take(MAX_BODY_EXCERPT_CHARS).collect::<String>();

    let context = serde_json::json!({
        "file": file,
        "title": title,
        "description": description,
        "h1": h1,
        "target_keyword": target_keyword,
        "slug": slug,
        "canonical": canonical,
        "image": image,
        "word_count": word_count,
        "internal_link_count": internal_links.len(),
        "body_excerpt": body_excerpt,
    });

    StepResult {
        success: true,
        message: format!("Loaded quality review context for {}", file),
        output: Some(context.to_string()),
    }
}

/// Agentic review step: score the article and persist the structured result.
pub(crate) async fn exec_content_quality_review(
    _step: &crate::engine::workflows::WorkflowStep,
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
    let backend =
        match crate::rig::provider::resolve_backend(agent_provider, None, None, None).await {
            Ok(b) => b,
            Err(e) => {
                return StepResult::fail(format!("Could not resolve LLM backend: {}", e));
            }
        };

    match &backend {
        LlmBackend::KimiDirect => {
            return StepResult::fail("Structured extraction is not supported with KimiDirect. \
                 Use Kimi bridge, Claude, OpenAI, or Ollama."
                    .to_string());
        }
        _ => {}
    }

    let context_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "content_quality_context")
        .and_then(|a| a.content.as_deref())
        .unwrap_or("");

    if context_json.is_empty() {
        return StepResult::fail("No content_quality_context artifact found on task".to_string());
    }

    let prompt = match build_review_prompt(project_path, context_json) {
        Ok(p) => p,
        Err(e) => {
            return StepResult::fail(format!("Failed to build quality review prompt: {}", e));
        }
    };

    let review = match crate::rig::extraction::extract_with_backend::<ContentQualityReview>(
        &backend,
        &prompt,
        Some(
            "You are a content quality reviewer. \
             Score the article and return only a valid ContentQualityReview by calling the submit tool.",
        ),
        Some("cqr"),
        None,
    )
    .await
    {
        Ok(r) => r,
        Err(e) => {
            return StepResult::fail(format!("Structured extraction failed for ContentQualityReview: {}", e));
        }
    };

    // Persist review to DB.
    if let Err(e) = persist_review(task, project_path, &review) {
        log::warn!("[content_quality_review] failed to persist review: {}", e);
    }

    let review_json = match serde_json::to_string_pretty(&review) {
        Ok(s) => s,
        Err(e) => {
            return StepResult::fail(format!("Failed to serialize ContentQualityReview: {}", e));
        }
    };

    let status = if review.overall_pass { "passed" } else { "failed" };
    StepResult {
        success: true,
        message: format!(
            "Quality review {} (usefulness: {}, image: {}, SEO: {}, cluster fit: {})",
            status,
            review.usefulness_score,
            review.image_score,
            review.seo_score,
            review.cluster_fit_score
        ),
        output: Some(review_json),
    }
}

fn build_review_prompt(project_path: &str, context_json: &str) -> Result<String, String> {
    let repo_root = Path::new(project_path);
    let skill_content = crate::engine::skills::load_skill(repo_root, "content-quality-review")
        .map(|s| s.content)
        .unwrap_or_else(|| DEFAULT_SKILL.to_string());

    let context: serde_json::Value =
        serde_json::from_str(context_json).map_err(|e| e.to_string())?;

    Ok(format!(
        "{skill_content}\n\n## Article Context\n\n```json\n{context}\n```\n\nReturn a ContentQualityReview JSON object.",
        skill_content = skill_content.trim(),
        context = serde_json::to_string_pretty(&context).unwrap_or_default(),
    ))
}

const DEFAULT_SKILL: &str = r#"Review the article against four criteria and return a ContentQualityReview:

1. usefulness_score (1-100): Does it answer a specific question with original examples, data, or first-hand insight? Would a reader learn something not found in the top 3 Google results?
2. image_score (1-100): Does it include at least one relevant, genuinely useful image, diagram, chart, or screenshot?
3. seo_score (1-100): Does it have a clean title (<60 chars), meta description, H1 aligned with the target keyword, clean slug, canonical URL, and internal links?
4. cluster_fit_score (1-100): Does it clearly map to a pillar/cluster and reference related content on the site?

Set overall_pass to true only if all four scores are >= 60 and no critical SEO field is missing.
Include a checks array with one entry per failed or borderline criterion, using ids: usefulness, image, seo_basics, cluster_fit.
"#;

fn persist_review(
    task: &Task,
    project_path: &str,
    review: &ContentQualityReview,
) -> crate::error::Result<()> {
    let context_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "content_quality_context")
        .and_then(|a| a.content.as_deref())
        .unwrap_or("{}");
    let context: serde_json::Value = serde_json::from_str(context_json).unwrap_or_default();
    let article_file = context["file"].as_str().unwrap_or("").to_string();

    if article_file.is_empty() {
        return Ok(());
    }

    let db = rusqlite::Connection::open(crate::db::default_db_path())?;
    let scores = serde_json::json!({
        "usefulness_score": review.usefulness_score,
        "image_score": review.image_score,
        "seo_score": review.seo_score,
        "cluster_fit_score": review.cluster_fit_score,
        "signal_score": review.signal_score,
    });
    let checks = serde_json::to_string(&review.checks)?;
    let now = chrono::Utc::now().to_rfc3339();

    db.execute(
        "INSERT INTO article_quality_reviews
         (project_id, task_id, article_file, overall_pass, scores_json, checks_json, reviewed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            &task.project_id,
            &task.id,
            &article_file,
            review.overall_pass as i32,
            scores.to_string(),
            checks,
            &now,
        ],
    )?;

    // Also sync article status so the UI can surface quality failures.
    let _ = db.execute(
        "UPDATE articles
         SET quality_score = ?1,
             quality_reviewed_at = ?2,
             quality_pass = ?3
         WHERE project_id = ?4 AND file LIKE ?5",
        rusqlite::params![
            compute_average_score(review),
            &now,
            review.overall_pass as i32,
            &task.project_id,
            format!("%{}", article_file),
        ],
    );

    let _ = crate::content::article_index::export_projection(
        &db,
        &task.project_id,
        Path::new(project_path),
    );

    Ok(())
}

fn yaml_string(value: &serde_yaml::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn extract_internal_links(body: &str) -> Vec<String> {
    // Simple regex-free extraction of `/blog/...` markdown links.
    let mut links = Vec::new();
    for part in body.split("](") {
        if let Some(end) = part.find(')') {
            let href = &part[..end];
            if href.starts_with("/blog/") {
                links.push(href.to_string());
            }
        }
    }
    links
}

fn compute_average_score(review: &ContentQualityReview) -> i64 {
    (review.usefulness_score
        + review.image_score
        + review.seo_score
        + review.cluster_fit_score)
        / 4
}

/// Spawns a `review_article_quality` follow-up task after `write_article`.
///
/// The `article_file` artifact lets the review step load the correct MDX without
/// guessing from the project state.
pub(crate) fn create_review_article_quality_task(
    conn: &rusqlite::Connection,
    parent_task: &Task,
    project_path: &str,
    article_file: &str,
) -> Option<String> {
    use crate::engine::spawner::{TaskSpawner, TaskSpec};
    use crate::models::task::{AgentPolicy, Priority, TaskArtifact, TaskRunPolicy};

    let parent_title = parent_task
        .title
        .as_deref()
        .unwrap_or("new article")
        .trim_start_matches("Write article: ");

    let title = format!("Quality review: {}", parent_title);
    let description = format!(
        "Structured quality review for '{}' after write_article {}. File: {}",
        parent_title, parent_task.id, article_file
    );

    let idempotency_key = format!(
        "followup:{}:review_article_quality:{}",
        parent_task.id, article_file
    );

    let artifact = TaskArtifact {
        key: "article_file".to_string(),
        path: None,
        artifact_type: Some("string".to_string()),
        source: Some("write_article".to_string()),
        content: Some(article_file.to_string()),
    };

    let spec = TaskSpec {
        project_id: parent_task.project_id.clone(),
        task_type: "review_article_quality".to_string(),
        title: Some(title),
        description: Some(description),
        phase: Some("implementation".to_string()),
        run_policy: Some(TaskRunPolicy::AutoEnqueue),
        priority: Priority::Medium,
        agent_policy: AgentPolicy::Required,
        idempotency_key: Some(idempotency_key),
        artifacts: vec![artifact],
        depends_on: vec![parent_task.id.clone()],
        ..Default::default()
    };

    match TaskSpawner::spawn(conn, spec) {
        Ok(task) => {
            log::info!(
                "[quality_review] spawned review_article_quality task {} after write_article {}",
                task.id,
                parent_task.id
            );
            Some(task.id)
        }
        Err(e) => {
            log::warn!("[quality_review] failed to create review_article_quality task: {}", e);
            None
        }
    }
}
