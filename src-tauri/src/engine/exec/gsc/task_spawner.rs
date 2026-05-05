use rusqlite::{Connection, OptionalExtension};

use crate::engine::project_paths::ProjectPaths;
use crate::engine::spawner::{DeduplicationPolicy, TaskSpawner, TaskSpec};
use crate::models::task::{AgentPolicy, TaskRunPolicy, Priority, Task};

/// Post-completion hook: reads gsc_collection.json and spawns fix tasks.
///
/// Called from `execute_task_with_token` after a successful `collect_gsc` task.
pub(crate) fn create_tasks_from_collection_after_exec(
    conn: &Connection,
    parent_task: &Task,
    project_path: &str,
) -> Vec<String> {
    let paths = ProjectPaths::from_path(project_path);
    let collection_path = paths.automation_dir.join("gsc_collection.json");

    let json_str = match std::fs::read_to_string(&collection_path) {
        Ok(s) => s,
        Err(_) => {
            log::info!("[collect_gsc] gsc_collection.json not found — no tasks created");
            return vec![];
        }
    };
    let data: serde_json::Value = match serde_json::from_str(&json_str) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[collect_gsc] failed to parse gsc_collection.json: {}", e);
            return vec![];
        }
    };

    let created_ids = create_tasks_from_collection(conn, parent_task, &data);
    log::info!("[collect_gsc] spawned {} fix tasks", created_ids.len());
    created_ids
}

/// Parse gsc_collection.json and create specific fix tasks in SQLite.
///
/// Maps reason codes to task types:
///   robots_blocked / noindex / fetch_error / canonical_mismatch → fix_technical
///   not_indexed_other                                           → interlinking (add inbound links)
///   crawled_not_indexed / other not_indexed_*                   → fix_indexing
///   api_error                                                   → fix_gsc_access (batched)
///   (all indexed)                                               → investigate_gsc


pub(crate) fn create_tasks_from_collection(
    conn: &Connection,
    parent_task: &Task,
    data: &serde_json::Value,
) -> Vec<String> {
    let items = match data["items"].as_array() {
        Some(a) => a,
        None => return vec![],
    };

    let mut created_ids: Vec<String> = vec![];
    let mut seen_issues = std::collections::HashSet::<String>::new();
    let mut api_error_count = 0u32;

    for item in items.iter() {
        let url = item["url"].as_str().unwrap_or("");
        let reason = item["reason_code"].as_str().unwrap_or("unknown");
        let action = item["action"].as_str().unwrap_or("");
        let verdict = item["verdict"].as_str().unwrap_or("");
        let priority_val = item["priority"].as_i64().unwrap_or(999);

        if reason == "indexed_pass" {
            continue;
        }

        if reason == "api_error" {
            api_error_count += 1;
            continue;
        }

        let issue_key = format!("{}:{}", reason, url);
        if seen_issues.contains(&issue_key) {
            continue;
        }
        seen_issues.insert(issue_key);

        let task_type = match reason {
            "robots_blocked" | "noindex" | "fetch_error" | "canonical_mismatch" => "fix_technical",
            "not_indexed_other" => "interlinking",
            "crawled_not_indexed" => "fix_indexing",
            _ => "fix_indexing",
        };

        let url_slug = {
            let without_scheme = url
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            if let Some(slash_pos) = without_scheme.find('/') {
                &without_scheme[slash_pos..]
            } else {
                url
            }
        };
        let reason_human = reason.replace('_', " ");
        let title = format!("Fix {}: {}", reason_human, url_slug);
        let description = format!(
            "URL: {}\nIssue: {}\nAction: {}\nVerdict: {}",
            url, reason, action, verdict
        );

        let priority_enum = if priority_val <= 30 {
            Priority::High
        } else {
            Priority::Medium
        };

        // Idempotency key includes URL to prevent duplicate tasks for same URL+reason
        let idempotency_key = format!("gsc:{}:{}", reason, url);

        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: task_type.to_string(),
            title: Some(title),
            description: Some(description),
            phase: Some("implementation".to_string()),
            run_policy: Some(TaskRunPolicy::AutoEnqueue),
            priority: priority_enum,
            agent_policy: AgentPolicy::Optional,
            idempotency_key: Some(idempotency_key),
            dedup_policy: Some(DeduplicationPolicy::Cooldown { days: 14 }),
            artifacts: vec![],
            depends_on: vec![],
            ..Default::default()
        };

        match TaskSpawner::spawn(conn, spec) {
            Ok(task) => created_ids.push(task.id.clone()),
            Err(e) => log::warn!("[collect_gsc] failed to create fix task: {}", e),
        }
    }

    // One batched fix_gsc_access task for all API errors
    if api_error_count > 0 {
        let title = format!(
            "Fix GSC API access errors ({} URLs affected)",
            api_error_count
        );
        let description =
            "GSC URL Inspection API returned errors. Check service account property access."
                .to_string();

        // Use spawn with custom idempotency key to allow specific execution_mode and agent_policy
        let idempotency_key = format!("followup:{}:fix_gsc_access:{}", parent_task.id, title);

        let spec = TaskSpec {
            project_id: parent_task.project_id.clone(),
            task_type: "fix_gsc_access".to_string(),
            title: Some(title),
            description: Some(description),
            phase: Some("implementation".to_string()),
            run_policy: Some(TaskRunPolicy::UserEnqueue),
            priority: Priority::High,
            agent_policy: AgentPolicy::Optional,
            idempotency_key: Some(idempotency_key),
            dedup_policy: Some(DeduplicationPolicy::SkipIfActive),
            artifacts: vec![],
            depends_on: vec![parent_task.id.clone()],
            ..Default::default()
        };

        match TaskSpawner::spawn(conn, spec) {
            Ok(task) => created_ids.push(task.id.clone()),
            Err(e) => log::warn!("[collect_gsc] failed to create fix_gsc_access task: {}", e),
        }
    }

    // If no issues — all pages indexed — trigger investigation
    if created_ids.is_empty() && api_error_count == 0 {
        let all_indexed = items
            .iter()
            .all(|i| i["reason_code"].as_str().unwrap_or("") == "indexed_pass");
        if all_indexed {
            let title = "Investigate GSC — all pages indexed, look for opportunities".to_string();
            let description = "gsc_collection.json shows all pages are indexed. Run investigation to find optimization opportunities.".to_string();

            // Use spawn with custom idempotency key to allow specific execution_mode and agent_policy
            let idempotency_key = format!("followup:{}:investigate_gsc:{}", parent_task.id, title);

            let spec = TaskSpec {
                project_id: parent_task.project_id.clone(),
                task_type: "investigate_gsc".to_string(),
                title: Some(title),
                description: Some(description),
                phase: Some("investigation".to_string()),
                run_policy: Some(TaskRunPolicy::AutoEnqueue),
                priority: Priority::Medium,
                agent_policy: AgentPolicy::Required,
                idempotency_key: Some(idempotency_key),
                dedup_policy: Some(DeduplicationPolicy::SkipIfActive),
                artifacts: vec![],
                depends_on: vec![parent_task.id.clone()],
                ..Default::default()
            };

            match TaskSpawner::spawn(conn, spec) {
                Ok(task) => created_ids.push(task.id.clone()),
                Err(e) => log::warn!("[collect_gsc] failed to create investigate_gsc task: {}", e),
            }
        }
    }

    created_ids
}
