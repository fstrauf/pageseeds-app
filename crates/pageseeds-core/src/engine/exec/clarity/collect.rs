use crate::clarity::{
    client::{ClarityClient, ClarityClientConfig},
    db, export,
    models::{ClarityCollection, ClarityCollectionMeta, ClarityExportRow},
};
use crate::config::env_resolver::EnvResolver;
use crate::engine::project_paths::ProjectPaths;
use crate::engine::task_store;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;
use rusqlite::Connection;

/// Native Rust implementation of the Clarity collection step.
///
/// 1. Reads clarity_project_id from the project record.
/// 2. Loads CLARITY_API_TOKEN from the secrets resolver.
/// 3. Fetches a deterministic set of dimensioned metric blocks.
/// 4. Stores flattened rows in SQLite.
/// 5. Writes clarity_collection.json to the automation dir.
pub fn exec_collect_clarity(task: &Task, project_path: &str, conn: &Connection) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let resolver = EnvResolver::new(project_path);

    // 1. Resolve project ID from the project record.
    let project = match task_store::get_project(conn, &task.project_id) {
        Ok(p) => p,
        Err(e) => {
            return StepResult::fail(format!("Failed to load project '{}': {}", task.project_id, e))
        }
    };
    let project_id = match project.clarity_project_id.as_deref().filter(|id| !id.is_empty()) {
        Some(id) => id.to_string(),
        None => {
            return StepResult::fail("clarity_project_id not set in project settings".to_string())
        }
    };

    // 2. Resolve API token.
    let api_token = match resolver
        .resolve("CLARITY_API_TOKEN")
        .map(|(v, _)| v)
    {
        Some(t) => t,
        None => {
            return StepResult::fail("CLARITY_API_TOKEN not configured — add it in Settings → Secrets"
                    .to_string())
        }
    };

    log::info!(
        "[collect_clarity] project_id={} task_id={}",
        project_id,
        task.id
    );

    // 3. Fetch from Clarity API inside a dedicated runtime thread.
    let project_id_owned = project_id.clone();
    let fetch_result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async move {
            let client = ClarityClient::new(ClarityClientConfig::new(
                api_token,
                project_id_owned.clone(),
            ));
            let results = client.fetch_all().await.map_err(crate::error::Error::Other)?;
            Ok::<_, crate::error::Error>(results)
        })
    })
    .join();

    let results = match fetch_result {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            let msg = e.to_string();
            return StepResult::fail(if msg.contains("401") || msg.contains("Unauthorized") {
                    "Clarity API token is invalid or expired".to_string()
                } else if msg.contains("429") || msg.contains("TooManyRequests") {
                    "Clarity API daily request limit reached".to_string()
                } else {
                    format!("Clarity Export API failed: {}", msg)
                });
        }
        Err(_) => {
            return StepResult::fail("Clarity collection thread panicked".to_string())
        }
    };

    // 4. Flatten API results into export rows.
    let exported_at = chrono::Utc::now().to_rfc3339();
    let today = chrono::Utc::now().date_naive();
    let mut all_rows: Vec<ClarityExportRow> = Vec::new();
    let mut requests_made = 0usize;

    for (label, blocks) in results {
        requests_made += 1;
        // The API returns one block per metric. Each block's rows cover the
        // requested numOfDays in aggregate, not per day. We assign the latest
        // covered date (today) as clarity_date; callers analysing multi-day
        // windows should treat this as the snapshot date.
        let clarity_date = today.to_string();
        for block in blocks {
            for point in block.information {
                all_rows.push(ClarityExportRow::from_data_point(
                    &clarity_date,
                    label,
                    &block.metric_name,
                    &point,
                ));
            }
        }
    }

    log::info!("[collect_clarity] flattened {} rows", all_rows.len());

    // 5. Store rows in SQLite.
    if let Err(e) = db::insert_rows(conn, &task.project_id, &exported_at, &all_rows) {
        return StepResult::fail(format!("Failed to store Clarity rows: {}", e));
    }

    // 6. Prune rows older than 90 days to keep the table bounded.
    let cutoff = (today - chrono::Days::new(90)).to_string();
    if let Err(e) = db::prune_old_rows(conn, &task.project_id, &cutoff) {
        log::warn!("[collect_clarity] failed to prune old rows: {}", e);
    }

    // 7. Write collection artifact.
    let collection = ClarityCollection {
        meta: ClarityCollectionMeta {
            project_id: project_id.clone(),
            exported_at: exported_at.clone(),
            days: 3,
            requests_made,
        },
        rows: all_rows,
    };

    if let Err(e) = export::write_collection(&paths.automation_dir, &collection) {
        return StepResult::fail(format!("Failed to write clarity_collection.json: {}", e));
    }

    StepResult {
        success: true,
        message: format!(
            "Collected {} Clarity rows across {} requests",
            collection.rows.len(),
            requests_made
        ),
        output: Some(serde_json::to_string(&collection.meta).unwrap_or_default()),
        artifact_key: None,
    }
}
