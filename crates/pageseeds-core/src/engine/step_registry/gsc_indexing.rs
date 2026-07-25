use std::collections::HashMap;

use super::HandlerFn;
use crate::engine::workflows::{StepKind, StepResult};

pub(super) fn register(handlers: &mut HashMap<StepKind, HandlerFn>) {
        // ─── GSC Indexing Recovery ──────────────────────────────────────────────

        register_blocking!(
            handlers,
            StepKind::GscRecoveryPrepare,
            crate::engine::exec::gsc::exec_gsc_recovery_prepare,
            gsc_token
        );

        register_blocking!(
            handlers,
            StepKind::GscRecoveryDrift,
            crate::engine::exec::gsc::exec_gsc_recovery_drift
        );

        register_blocking!(
            handlers,
            StepKind::GscRecoveryPlan,
            crate::engine::exec::gsc::exec_gsc_recovery_plan
        );

        // ─── Fix Indexing Internal Links ────────────────────────────────────────

        register_blocking!(
            handlers,
            StepKind::IndexingLinkContext,
            crate::engine::exec::content::exec_indexing_link_context
        );

        register_blocking!(
            handlers,
            StepKind::IndexingLinkPlan,
            crate::engine::exec::content::exec_indexing_link_plan,
            agent_provider
        );

        register_blocking!(
            handlers,
            StepKind::IndexingLinkApply,
            crate::engine::exec::content::exec_indexing_link_apply
        );

        register_blocking!(
            handlers,
            StepKind::IndexingLinkVerify,
            crate::engine::exec::content::exec_indexing_link_verify
        );

        // ─── GSC Indexing Outcome Review ────────────────────────────────────────

        register_blocking!(
            handlers,
            StepKind::GscIndexingOutcomeInspect,
            crate::engine::exec::gsc::exec_gsc_indexing_outcome_inspect,
            gsc_token
        );

        register_blocking!(
            handlers,
            StepKind::GscIndexingOutcomeReport,
            crate::engine::exec::gsc::exec_gsc_indexing_outcome_report
        );

        // ─── Content Outcome Review (issue #23) ─────────────────────────────

        register_blocking!(
            handlers,
            StepKind::ContentOutcomeCompare,
            crate::engine::exec::outcome_review::exec_content_outcome_compare,
            db_conn
        );

        // ─── Indexing Health Campaign ───────────────────────────────────────────

        register_blocking!(
            handlers,
            StepKind::IhcCheckPrerequisites,
            crate::engine::exec::indexing_health::exec_ihc_check_prerequisites
        );

        register_blocking!(
            handlers,
            StepKind::IhcBuildTargetContext,
            crate::engine::exec::indexing_health::exec_ihc_build_target_context
        );

        register_blocking!(
            handlers,
            StepKind::IhcDistinctivenessReview,
            crate::engine::exec::indexing_health::exec_ihc_distinctiveness_review,
            agent_provider,
            optional_context
        );

        register_blocking!(
            handlers,
            StepKind::IhcReducePlan,
            crate::engine::exec::indexing_health::exec_ihc_reduce_plan
        );

        handlers.insert(
            StepKind::GenerateFeatureSpec,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let agent_provider = ctx.agent_provider.to_string();
                Box::pin(async move {
                    crate::engine::exec::feature_spec::exec_generate_feature_spec(
                        &task,
                        &project_path,
                        &agent_provider,
                    )
                    .await
                })
            }),
        );

        // ─── SEO Discovery ────────────────────────────────────────────────────────

        register_blocking!(
            handlers,
            StepKind::RankOpportunities,
            crate::engine::exec::seo_discovery::exec_rank_opportunities,
            db_conn
        );

        handlers.insert(
            StepKind::OpportunityReviewAgent,
            Box::new(|_step, _ctx| {
                Box::pin(async move {
                    StepResult {
                        success: true,
                        message: "Opportunity review agent placeholder — Phase 2".to_string(),
                        output: None,
                        artifact_key: None,
                    }
                })
            }),
        );
}
