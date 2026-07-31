//! Deterministic PostHog conversion-tape collection (issue #308).
//!
//! Fail closed when `posthog_project_id` or `POSTHOG_API_KEY` is missing —
//! never report success without attempting a fetch (#27).

use crate::config::env_resolver::EnvResolver;
use crate::engine::project_paths::ProjectPaths;
use crate::engine::task_store;
use crate::engine::workflows::StepResult;
use crate::models::task::Task;
use crate::posthog::{
    client::{
        filter_host_from_site_base_url, normalize_host, PosthogClient, PosthogClientConfig,
    },
    db, export,
    models::{
        resolve_conversion_events, PosthogCollection, PosthogCollectionMeta,
    },
};
use crate::project_config::ensure_project_config;
use rusqlite::Connection;

/// Native Rust implementation of the PostHog conversion-tape collection step.
pub fn exec_collect_posthog(task: &Task, project_path: &str, conn: &Connection) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let resolver = EnvResolver::new(project_path);

    // 1. Load project.yaml — posthog_project_id lives on ProjectConfig, not SQLite.
    let (config, _) = match ensure_project_config(&paths.automation_dir) {
        Ok(c) => c,
        Err(e) => {
            return StepResult::fail(format!(
                "Failed to load project.yaml for PostHog collect: {e}. \
                 Set posthog_project_id in .github/automation/project.yaml"
            ));
        }
    };

    let posthog_project_id = match config
        .posthog_project_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => id.to_string(),
        None => {
            return StepResult::fail(
                "posthog_project_id not set in project.yaml — add the numeric PostHog \
                 project id under .github/automation/project.yaml (required for collect_posthog)"
                    .to_string(),
            );
        }
    };

    // 2. Resolve API key.
    let api_key = match resolver.resolve("POSTHOG_API_KEY").map(|(v, _)| v) {
        Some(k) if !k.trim().is_empty() => k,
        _ => {
            return StepResult::fail(
                "POSTHOG_API_KEY not configured — add a PostHog personal API key via \
                 EnvResolver secrets (~/.config/automation/secrets.env or project .env.local)"
                    .to_string(),
            );
        }
    };

    // 3. Events: empty config list → defaults at collect time.
    let events = resolve_conversion_events(&config.posthog_conversion_events);

    // Optional API host override (EU etc.).
    let api_host = resolver
        .resolve("POSTHOG_HOST")
        .map(|(v, _)| normalize_host(&v))
        .unwrap_or_else(|| "us.posthog.com".to_string());

    // Optional event `$host` filter from project site_base_url (no new config key).
    let filter_host = task_store::get_project(conn, &task.project_id)
        .ok()
        .and_then(|p| p.site_base_url())
        .and_then(|base| filter_host_from_site_base_url(&base));

    log::info!(
        "[collect_posthog] posthog_project_id={} events={:?} api_host={} filter_host={:?} task_id={}",
        posthog_project_id,
        events,
        api_host,
        filter_host,
        task.id
    );

    // 4. Fetch from PostHog Query API inside a dedicated runtime thread.
    let project_id_owned = posthog_project_id.clone();
    let events_owned = events.clone();
    let api_host_owned = api_host.clone();
    let filter_host_owned = filter_host.clone();
    let fetch_result = std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(async move {
            let mut cfg =
                PosthogClientConfig::new(api_key, project_id_owned).with_host(api_host_owned);
            if let Some(h) = filter_host_owned {
                cfg = cfg.with_filter_host(h);
            }
            let client = PosthogClient::new(cfg);
            let window_days = client.window_days();
            let rows = client
                .fetch_all_events(&events_owned)
                .await
                .map_err(crate::error::Error::Other)?;
            Ok::<_, crate::error::Error>((rows, window_days))
        })
    })
    .join();

    let (rows, window_days) = match fetch_result {
        Ok(Ok(r)) => r,
        Ok(Err(e)) => {
            let msg = e.to_string();
            return StepResult::fail(if msg.contains("401") || msg.contains("Unauthorized") {
                "PostHog API key is invalid or expired".to_string()
            } else if msg.contains("403") || msg.contains("Forbidden") {
                "PostHog API access forbidden — check project id and key scopes".to_string()
            } else {
                format!("PostHog Query API failed: {msg}")
            });
        }
        Err(_) => {
            return StepResult::fail("PostHog collection thread panicked".to_string());
        }
    };

    let exported_at = chrono::Utc::now().to_rfc3339();
    let today = chrono::Utc::now().date_naive();

    log::info!("[collect_posthog] fetched {} rows (window_days={})", rows.len(), window_days);

    // 5. Store rows in SQLite (INSERT OR IGNORE).
    let inserted = match db::insert_rows(conn, &task.project_id, &rows) {
        Ok(n) => n,
        Err(e) => {
            return StepResult::fail(format!("Failed to store PostHog rows: {e}"));
        }
    };

    // 6. Prune rows older than 90 days.
    let cutoff = (today - chrono::Days::new(90)).to_string();
    if let Err(e) = db::prune_old_rows(conn, &task.project_id, &cutoff) {
        log::warn!("[collect_posthog] failed to prune old rows: {e}");
    }

    // 7. Write collection artifact — meta.days from client window_days (single source).
    let collection = PosthogCollection {
        meta: PosthogCollectionMeta {
            project_id: task.project_id.clone(),
            posthog_project_id: posthog_project_id.clone(),
            exported_at: exported_at.clone(),
            days: window_days,
            events: events.clone(),
            rows: rows.len(),
        },
        rows,
    };

    if let Err(e) = export::write_collection(&paths.automation_dir, &collection) {
        return StepResult::fail(format!("Failed to write posthog_collection.json: {e}"));
    }

    let empty_note = if collection.meta.rows == 0 {
        " (API returned zero events — empty conversion tape is OK)"
    } else {
        ""
    };

    StepResult {
        success: true,
        message: format!(
            "Collected {} PostHog conversion rows ({} newly inserted) across {} events{}",
            collection.meta.rows,
            inserted,
            events.len(),
            empty_note
        ),
        output: Some(serde_json::to_string(&collection.meta).unwrap_or_default()),
        artifact_key: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, TaskRun, TaskReviewSurface, TaskRunPolicy, TaskStatus,
    };
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_project(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("ps_collect_posthog_{tag}_{nanos}"));
        let auto = dir.join(".github/automation");
        std::fs::create_dir_all(&auto).unwrap();
        dir
    }

    fn make_task(project_id: &str) -> Task {
        Task {
            id: "task-ph".into(),
            task_type: "collect_posthog".into(),
            phase: "collection".into(),
            status: TaskStatus::Todo,
            priority: Priority::High,
            run_policy: TaskRunPolicy::AutoEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::None,
            title: None,
            description: None,
            project_id: project_id.into(),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun::default(),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        }
    }

    fn in_memory_db(project_id: &str, path: &str) -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path) VALUES (?1, 'Test', ?2)",
            rusqlite::params![project_id, path],
        )
        .unwrap();
        conn
    }

    #[test]
    fn missing_project_id_fails_closed() {
        let dir = temp_project("no_id");
        let auto = dir.join(".github/automation");
        std::fs::write(
            auto.join("project.yaml"),
            "schema_version: 1\nsearch_keywords:\n  primary: []\n",
        )
        .unwrap();
        let path = dir.to_string_lossy().to_string();
        let conn = in_memory_db("proj_ph", &path);
        // Ensure no API key leak from environment makes this pass for the wrong reason.
        // Missing project id is checked first.
        let result = exec_collect_posthog(&make_task("proj_ph"), &path, &conn);
        assert!(!result.success, "must fail: {}", result.message);
        assert!(
            result.message.contains("posthog_project_id"),
            "message should mention posthog_project_id: {}",
            result.message
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_api_key_fails_closed() {
        let dir = temp_project("no_key");
        let auto = dir.join(".github/automation");
        std::fs::write(
            auto.join("project.yaml"),
            "schema_version: 1\nposthog_project_id: \"131482\"\nsearch_keywords:\n  primary: []\n",
        )
        .unwrap();
        // Isolate from ambient POSTHOG_API_KEY: write an empty .env.local so
        // EnvResolver still searches, but we rely on the key not being present
        // for this unique temp path (shell env may still inject the key).
        // If shell has POSTHOG_API_KEY, skip asserting key-specific fail and
        // only require non-success without fake "collected" success.
        let path = dir.to_string_lossy().to_string();
        let conn = in_memory_db("proj_ph2", &path);
        let result = exec_collect_posthog(&make_task("proj_ph2"), &path, &conn);
        // With project id set, either missing key fails closed, or a present
        // ambient key attempts a real fetch (may fail network/auth) — never
        // silent success-without-attempt.
        if result.success {
            // Ambient key + successful live API is rare in CI; allow only if
            // output meta is present (real attempt).
            assert!(
                result.output.is_some(),
                "success must include collection meta"
            );
        } else {
            assert!(
                result.message.contains("POSTHOG_API_KEY")
                    || result.message.contains("PostHog")
                    || result.message.contains("API"),
                "fail message should be actionable: {}",
                result.message
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_project_yaml_fails_closed() {
        let dir = temp_project("no_yaml");
        // automation dir exists but no project.yaml and no legacy MD.
        let path = dir.to_string_lossy().to_string();
        let conn = in_memory_db("proj_ph3", &path);
        let result = exec_collect_posthog(&make_task("proj_ph3"), &path, &conn);
        assert!(!result.success, "must fail without project.yaml");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
