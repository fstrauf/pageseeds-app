use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use rusqlite::Connection;

use crate::engine::workflows::{StepKind, StepResult, WorkflowStep};
use crate::models::task::Task;

/// Register a synchronous handler that runs inside `tokio::task::spawn_blocking`.
///
/// **Simple variant** (task + project_path only):
/// ```ignore
/// register_blocking!(handlers, StepKind::ContentAudit,
///     crate::engine::exec::content_audit::exec_content_audit);
/// ```
///
/// **With provider variant** (task + project_path + ctx field):
/// ```ignore
/// register_blocking!(handlers, StepKind::ClusterLinkStrategy,
///     crate::engine::exec::content::exec_cluster_link_strategy, agent_provider);
/// ```
macro_rules! register_blocking {
    ($registry:ident, $kind:expr, $fn:path) => {
        $registry.insert(
            $kind,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || $fn(&task, &project_path))
                        .await
                        .unwrap_or_else(|e| StepResult {
                            success: false,
                            message: format!("Step panicked: {}", e),
                            output: None,
                        })
                })
            }),
        )
    };
    ($registry:ident, $kind:expr, $fn:path, gsc_token) => {
        $registry.insert(
            $kind,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let gsc_token = ctx.gsc_token.map(|s| s.to_string());
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        $fn(&task, &project_path, gsc_token.as_deref())
                    })
                    .await
                    .unwrap_or_else(|e| StepResult {
                        success: false,
                        message: format!("Step panicked: {}", e),
                        output: None,
                    })
                })
            }),
        )
    };
    ($registry:ident, $kind:expr, $fn:path, optional_context) => {
        $registry.insert(
            $kind,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let context_json = ctx.latest_raw.map(|s| s.to_string());
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        $fn(&task, &project_path, context_json.as_deref())
                    })
                    .await
                    .unwrap_or_else(|e| StepResult {
                        success: false,
                        message: format!("Step panicked: {}", e),
                        output: None,
                    })
                })
            }),
        )
    };
    ($registry:ident, $kind:expr, $fn:path, db_conn) => {
        $registry.insert(
            $kind,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let db_path = crate::db::default_db_path();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        let conn = match rusqlite::Connection::open(&db_path) {
                            Ok(c) => c,
                            Err(e) => {
                                return StepResult {
                                    success: false,
                                    message: format!("Failed to open DB: {}", e),
                                    output: None,
                                }
                            }
                        };
                        $fn(&task, &project_path, &conn)
                    })
                    .await
                    .unwrap_or_else(|e| StepResult {
                        success: false,
                        message: format!("Step panicked: {}", e),
                        output: None,
                    })
                })
            }),
        )
    };
    ($registry:ident, $kind:expr, $fn:path, $provider:ident) => {
        $registry.insert(
            $kind,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let provider = ctx.$provider.to_string();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || $fn(&task, &project_path, &provider))
                        .await
                        .unwrap_or_else(|e| StepResult {
                            success: false,
                            message: format!("Step panicked: {}", e),
                            output: None,
                        })
                })
            }),
        )
    };
    ($registry:ident, $kind:expr, $fn:path, $provider:ident, optional_context) => {
        $registry.insert(
            $kind,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let provider = ctx.$provider.to_string();
                let context_json = ctx.latest_raw.map(|s| s.to_string());
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        $fn(&task, &project_path, &provider, context_json.as_deref())
                    })
                    .await
                    .unwrap_or_else(|e| StepResult {
                        success: false,
                        message: format!("Step panicked: {}", e),
                        output: None,
                    })
                })
            }),
        )
    };
    ($registry:ident, $kind:expr, $fn:path, $provider:ident, context_json) => {
        $registry.insert(
            $kind,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let provider = ctx.$provider.to_string();
                let context_json = ctx.latest_raw.unwrap_or("{}").to_string();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        $fn(&task, &project_path, &provider, &context_json)
                    })
                    .await
                    .unwrap_or_else(|e| StepResult {
                        success: false,
                        message: format!("Step panicked: {}", e),
                        output: None,
                    })
                })
            }),
        )
    };
    ($registry:ident, $kind:expr, $fn:path, $provider:ident, step) => {
        $registry.insert(
            $kind,
            Box::new(|step, ctx| {
                let step = step.clone();
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let provider = ctx.$provider.to_string();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || $fn(&step, &task, &project_path, &provider))
                        .await
                        .unwrap_or_else(|e| StepResult {
                            success: false,
                            message: format!("Step panicked: {}", e),
                            output: None,
                        })
                })
            }),
        )
    };
}
