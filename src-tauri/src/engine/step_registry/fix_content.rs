use std::collections::HashMap;

use super::HandlerFn;
use crate::engine::workflows::{StepKind, StepResult};

pub(super) fn register(handlers: &mut HashMap<StepKind, HandlerFn>) {
        // ─── Fix Content Article ────────────────────────────────────────────────

        register_blocking!(
            handlers,
            StepKind::FixContentArticleContext,
            crate::engine::exec::content::exec_fix_content_article_context
        );

        register_blocking!(
            handlers,
            StepKind::LinkIntegrityVerify,
            crate::engine::exec::content::exec_link_integrity_verify
        );

        handlers.insert(
            StepKind::ContentWriteVerify,
            Box::new(|_step, ctx| {
                let result = crate::engine::exec::content::exec_content_write_verify(
                    ctx.conn,
                    ctx.task,
                    ctx.project_path,
                );
                Box::pin(async move { result })
            }),
        );

        handlers.insert(
            StepKind::FixContentArticleGenerate,
            Box::new(|step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let agent_provider = ctx.agent_provider.to_string();
                let step = step.clone();
                Box::pin(async move {
                    crate::engine::exec::content::exec_fix_content_article_generate(
                        &step,
                        &task,
                        &project_path,
                        &agent_provider,
                    )
                    .await
                })
            }),
        );

        handlers.insert(
            StepKind::FixContentArticleApply,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let latest_raw = ctx.latest_raw.map(|s| s.to_string());
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        crate::engine::exec::content::exec_fix_content_article_apply(
                            &task,
                            &project_path,
                            latest_raw.as_deref(),
                        )
                    })
                    .await
                    .unwrap_or_else(|e| StepResult {
                        success: false,
                        message: format!("Step panicked: {}", e),
                        output: None,
                    })
                })
            }),
        );

        handlers.insert(
            StepKind::FixContentArticleVerify,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        crate::engine::exec::content::exec_fix_content_article_verify(
                            &task,
                            &project_path,
                        )
                    })
                    .await
                    .unwrap_or_else(|e| StepResult {
                        success: false,
                        message: format!("Step panicked: {}", e),
                        output: None,
                    })
                })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::CtrTemplateVerifyRender,
            crate::engine::exec::ctr_audit::exec_ctr_template_verify_render,
            db_conn
        );

        register_blocking!(
            handlers,
            StepKind::CtrSchemaDetect,
            crate::engine::exec::ctr_audit::exec_ctr_schema_detect,
            db_conn
        );

        register_blocking!(
            handlers,
            StepKind::CtrSchemaVerifyRender,
            crate::engine::exec::ctr_audit::exec_ctr_schema_verify_render,
            db_conn
        );

        register_blocking!(
            handlers,
            StepKind::CtrOutcomeCompare,
            crate::engine::exec::ctr_audit::exec_ctr_outcome_compare,
            db_conn
        );

        register_blocking!(
            handlers,
            StepKind::CtrOutcomeReport,
            crate::engine::exec::ctr_audit::exec_ctr_outcome_report,
            db_conn
        );

        register_blocking!(
            handlers,
            StepKind::MergeLoadPlan,
            crate::engine::exec::consolidate_cluster::exec_merge_load_plan
        );

        handlers.insert(
            StepKind::MergePreflight,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let plan_json = ctx.latest_raw.unwrap_or("{}").to_string();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        crate::engine::exec::consolidate_cluster::exec_merge_preflight(
                            &task,
                            &project_path,
                            &plan_json,
                        )
                    })
                    .await
                    .unwrap_or_else(|e| StepResult {
                        success: false,
                        message: format!("Step panicked: {}", e),
                        output: None,
                    })
                })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::MergeExtractSections,
            crate::engine::exec::consolidate_cluster::exec_merge_extract_sections
        );

        register_blocking!(
            handlers,
            StepKind::MergeDraftPatch,
            crate::engine::exec::consolidate_cluster::exec_merge_draft_patch,
            agent_provider,
            context_json
        );

        handlers.insert(
            StepKind::MergeApplyPatch,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let patch_json = ctx.latest_raw.unwrap_or("{}").to_string();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        crate::engine::exec::consolidate_cluster::exec_merge_apply_patch(
                            &task,
                            &project_path,
                            &patch_json,
                        )
                    })
                    .await
                    .unwrap_or_else(|e| StepResult {
                        success: false,
                        message: format!("Step panicked: {}", e),
                        output: None,
                    })
                })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::MergeGenerateRedirects,
            crate::engine::exec::consolidate_cluster::exec_merge_generate_redirects
        );

        register_blocking!(
            handlers,
            StepKind::MergeRewriteInboundLinks,
            crate::engine::exec::consolidate_cluster::exec_merge_rewrite_inbound_links
        );

        register_blocking!(
            handlers,
            StepKind::MergeValidateOutput,
            crate::engine::exec::consolidate_cluster::exec_merge_validate_output
        );

        register_blocking!(
            handlers,
            StepKind::MergeSyncArticles,
            crate::engine::exec::consolidate_cluster::exec_merge_sync_articles
        );

        // ─── Keyword Research: Territory Analysis ───────────────────────────────

        register_blocking!(
            handlers,
            StepKind::ResearchTerritoryAnalysis,
            crate::engine::exec::keywords::exec_research_territory_analysis
        );

        // ─── Territory Research ─────────────────────────────────────────────────

        register_blocking!(
            handlers,
            StepKind::TerritoryLoadRecommendation,
            crate::engine::exec::territory_research::exec_territory_load_recommendation
        );

        register_blocking!(
            handlers,
            StepKind::TerritoryBuildContext,
            crate::engine::exec::territory_research::exec_territory_build_context
        );

        handlers.insert(
            StepKind::TerritoryStrategy,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let agent_provider = ctx.agent_provider.to_string();
                let context_json = ctx.latest_raw.unwrap_or("{}").to_string();
                Box::pin(async move {
                    crate::engine::exec::territory_research::exec_territory_strategy(
                        &task,
                        &project_path,
                        &agent_provider,
                        &context_json,
                    )
                    .await
                })
            }),
        );

        handlers.insert(
            StepKind::TerritoryApply,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let strategy_json = ctx.latest_raw.unwrap_or("{}").to_string();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        crate::engine::exec::territory_research::exec_territory_apply(
                            &task,
                            &project_path,
                            &strategy_json,
                        )
                    })
                    .await
                    .unwrap_or_else(|e| StepResult {
                        success: false,
                        message: format!("Step panicked: {}", e),
                        output: None,
                    })
                })
            }),
        );
}
