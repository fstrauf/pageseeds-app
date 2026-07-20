#[macro_use]
mod macros;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use rusqlite::Connection;

use crate::engine::workflows::{StepKind, StepResult, WorkflowStep};
use crate::models::task::Task;

pub struct StepContext<'a> {
    pub task: &'a Task,
    pub project_path: &'a str,
    pub site_url: &'a str,
    pub agent_provider: &'a str,
    pub seo_provider: &'a str,
    pub latest_raw: Option<&'a str>,
    pub gsc_token: Option<&'a str>,
    pub conn: &'a Connection,
}

type HandlerFn = Box<
    dyn for<'b> Fn(
            &'b WorkflowStep,
            &'b StepContext<'b>,
        ) -> Pin<Box<dyn Future<Output = StepResult> + Send + 'b>>
        + Send
        + Sync,
>;

pub struct StepRegistry {
    handlers: HashMap<StepKind, HandlerFn>,
}

impl StepRegistry {
    pub fn new() -> Self {
        let mut handlers: HashMap<StepKind, HandlerFn> = HashMap::new();

        handlers.insert(
            StepKind::Agentic,
            Box::new(|step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                let site_url = ctx.site_url;
                let agent_provider = ctx.agent_provider;
                let latest_raw = ctx.latest_raw;
                let next_publish_date =
                    crate::engine::exec::agentic::compute_next_publish_date(ctx.conn, &task.project_id);
                Box::pin(async move {
                    crate::engine::exec::agentic::exec_agentic(
                        step,
                        task,
                        project_path,
                        site_url,
                        agent_provider,
                        latest_raw,
                        next_publish_date,
                    )
                    .await
                })
            }),
        );

        handlers.insert(
            StepKind::Manual,
            Box::new(|step, _ctx| {
                let name = step.name.clone();
                Box::pin(async move {
                    StepResult {
                        success: true,
                        message: format!("Manual step '{}' — requires user action", name),
                        output: None,
                    }
                })
            }),
        );

        handlers.insert(
            StepKind::ClusterLinkScan,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::content::exec_cluster_link_scan(task, project_path)
                })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::ClusterLinkStrategy,
            crate::engine::exec::content::exec_cluster_link_strategy,
            agent_provider
        );

        handlers.insert(
            StepKind::ClusterLinkApply,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::content::exec_cluster_link_apply(task, project_path)
                })
            }),
        );

        handlers.insert(
            StepKind::ContentReviewRecommend,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let agent_provider = ctx.agent_provider.to_string();
                Box::pin(async move {
                    crate::engine::exec::content::exec_content_review_recommend(
                        &task,
                        &project_path,
                        &agent_provider,
                    )
                    .await
                })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::KeywordResearchNative,
            crate::engine::exec::keywords::exec_keyword_research_native,
            seo_provider
        );

        register_blocking!(
            handlers,
            StepKind::ResearchFinalSelection,
            crate::engine::exec::research::exec_research_final_selection,
            agent_provider,
            optional_context
        );

        register_blocking!(
            handlers,
            StepKind::LandingPageSpecWrite,
            crate::engine::exec::research::exec_landing_page_spec_write
        );

        register_blocking!(
            handlers,
            StepKind::RedditConfigParse,
            crate::engine::exec::reddit::exec_reddit_config_parse,
            agent_provider
        );

        handlers.insert(
            StepKind::RedditSearch,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::reddit::exec_reddit_search(task, project_path).await
                })
            }),
        );

        handlers.insert(
            StepKind::RedditEnrich,
            Box::new(|_step, _ctx| {
                Box::pin(async move {
                    StepResult {
                        success: true,
                        message: "Reddit enrichment pass — starting AI scoring loop".to_string(),
                        output: None,
                    }
                })
            }),
        );

        handlers.insert(
            StepKind::RedditFetchResults,
            Box::new(|_step, _ctx| {
                Box::pin(async move {
                    StepResult {
                        success: true,
                        message: "Reddit results fetch — starting DB query".to_string(),
                        output: None,
                    }
                })
            }),
        );

        handlers.insert(
            StepKind::ContentSync,
            Box::new(|_step, ctx| {
                let result = crate::engine::exec::content::exec_content_sync(
                    ctx.task,
                    ctx.project_path,
                    ctx.conn,
                );
                Box::pin(async move { result })
            }),
        );

        handlers.insert(
            StepKind::FormatValidation,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::content::exec_format_validation(task, project_path)
                })
            }),
        );

        handlers.insert(
            StepKind::FormatFix,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::content::exec_format_fix(task, project_path)
                })
            }),
        );

        handlers.insert(
            StepKind::SanitizeContent,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::content::exec_sanitize_content(task, project_path)
                })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::GscSyncArticles,
            crate::engine::exec::gsc::exec_gsc_sync_articles,
            gsc_token
        );

        handlers.insert(
            StepKind::GscSummarise,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(
                    async move { crate::engine::exec::gsc::exec_gsc_summarise(task, project_path) },
                )
            }),
        );

        handlers.insert(
            StepKind::IndexingFixContext,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::indexing_fix::exec_indexing_fix_context(task, project_path)
                })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::IndexingFixApply,
            crate::engine::exec::indexing_fix::exec_indexing_fix_apply,
            agent_provider,
            optional_context
        );

        register_blocking!(
            handlers,
            StepKind::ContentAudit,
            crate::engine::exec::content_audit::exec_content_audit
        );

        register_blocking!(
            handlers,
            StepKind::CollectGscInspect,
            crate::engine::exec::gsc::exec_collect_gsc,
            gsc_token
        );

        handlers.insert(
            StepKind::IndexingDiagnosticsRun,
            Box::new(|_step, ctx| {
                let result = crate::engine::exec::gsc_diagnostics::exec_indexing_diagnostics(
                    ctx.task,
                    ctx.project_path,
                    ctx.gsc_token,
                    ctx.conn,
                );
                Box::pin(async move { result })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::GscInvestigateAgentic,
            crate::engine::exec::gsc::exec_gsc_investigate,
            agent_provider,
            step
        );

        register_blocking!(
            handlers,
            StepKind::CollectClarity,
            crate::engine::exec::clarity::exec_collect_clarity,
            db_conn
        );

        register_blocking!(
            handlers,
            StepKind::ClaritySummarise,
            crate::engine::exec::clarity::exec_clarity_summarise,
            db_conn
        );

        register_blocking!(
            handlers,
            StepKind::ClarityInvestigateAgentic,
            crate::engine::exec::clarity::exec_clarity_investigate,
            agent_provider,
            step
        );

        handlers.insert(
            StepKind::SocialCollectSources,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::social::exec_social_collect_sources(task, project_path)
                })
            }),
        );

        handlers.insert(
            StepKind::SocialLoadTemplates,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::social::exec_social_load_templates(task, project_path)
                })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::SocialGeneratePosts,
            crate::engine::exec::social::exec_social_generate_posts,
            agent_provider,
            step
        );

        handlers.insert(
            StepKind::SocialBuildVisuals,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::social::exec_social_build_visuals(task, project_path)
                })
            }),
        );

        handlers.insert(
            StepKind::SocialSaveCampaign,
            Box::new(|_step, ctx| {
                let result = crate::engine::exec::social::exec_social_save_campaign(
                    ctx.task,
                    ctx.project_path,
                    ctx.conn,
                );
                Box::pin(async move { result })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::SocialRegenerateSingle,
            crate::engine::exec::social::exec_social_regenerate_single,
            agent_provider,
            step
        );

        handlers.insert(
            StepKind::SocialRebuildVisual,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::social::exec_social_rebuild_visual(task, project_path)
                })
            }),
        );

        handlers.insert(
            StepKind::SocialUpdatePost,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::social::exec_social_update_post(task, project_path)
                })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::SocialDesignTemplate,
            crate::engine::exec::social::exec_social_design_template,
            agent_provider,
            step
        );

        handlers.insert(
            StepKind::SocialSaveTemplate,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::social::exec_social_save_template(task, project_path)
                })
            }),
        );

        handlers.insert(
            StepKind::CoverageLoadArticles,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::coverage::exec_coverage_load_articles(task, project_path)
                })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::CoverageClusterAnalysis,
            crate::engine::exec::coverage::exec_coverage_cluster_analysis,
            agent_provider,
            context_json
        );

        handlers.insert(
            StepKind::CoverageSave,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::coverage::exec_coverage_save(task, project_path)
                })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::EnsureCoverageFresh,
            crate::engine::exec::coverage::exec_ensure_coverage_fresh,
            agent_provider
        );

        register_blocking!(
            handlers,
            StepKind::RedditPostReply,
            crate::engine::exec::reddit::exec_reddit_post_reply,
            db_conn
        );

        handlers.insert(
            StepKind::SocialExtractArticle,
            Box::new(|_step, ctx| {
                let task = ctx.task;
                let project_path = ctx.project_path;
                Box::pin(async move {
                    crate::engine::exec::social::exec_social_extract_article(task, project_path)
                })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::CtrRenderedSerpAudit,
            crate::engine::exec::ctr_audit::exec_ctr_rendered_serp_audit,
            db_conn
        );

        register_blocking!(
            handlers,
            StepKind::CtrTemplateDetect,
            crate::engine::exec::ctr_audit::exec_ctr_template_detect,
            db_conn
        );

        handlers.insert(
            StepKind::CtrBuildContext,
            Box::new(|_step, ctx| {
                let result = crate::engine::exec::ctr_audit::exec_ctr_build_context(
                    ctx.task,
                    ctx.project_path,
                    ctx.gsc_token,
                    ctx.conn,
                );
                Box::pin(async move { result })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::CtrAnalyze,
            crate::engine::exec::ctr_audit::exec_ctr_analyze,
            agent_provider,
            context_json
        );

        handlers.insert(
            StepKind::CtrFixGenerate,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let agent_provider = ctx.agent_provider.to_string();
                Box::pin(async move {
                    crate::engine::exec::ctr_audit::exec_ctr_fix_generate(
                        &task,
                        &project_path,
                        &agent_provider,
                    )
                    .await
                })
            }),
        );

        register_blocking!(
            handlers,
            StepKind::CanBuildContext,
            crate::engine::exec::cannibalization::exec_can_build_context
        );

        register_blocking!(
            handlers,
            StepKind::CanExactKeywordDupes,
            crate::engine::exec::cannibalization::exec_can_exact_keyword_dupes
        );

        register_blocking!(
            handlers,
            StepKind::CanSelectCandidates,
            crate::engine::exec::cannibalization::exec_can_select_candidates
        );

        register_blocking!(
            handlers,
            StepKind::CanAnalyzeCandidates,
            crate::engine::exec::cannibalization::exec_can_analyze_candidates,
            agent_provider
        );

        register_blocking!(
            handlers,
            StepKind::CanReduceStrategy,
            crate::engine::exec::cannibalization::exec_can_reduce_strategy
        );

        handlers.insert(
            StepKind::CtrFixApply,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                let latest_raw = ctx.latest_raw.map(|s| s.to_string());
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        crate::engine::exec::ctr_audit::exec_ctr_fix_apply(
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
            StepKind::CtrVerifyFix,
            Box::new(|_step, ctx| {
                let task = ctx.task.clone();
                let project_path = ctx.project_path.to_string();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || {
                        crate::engine::exec::ctr_audit::exec_ctr_verify_fix(&task, &project_path)
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

        Self { handlers }
    }

    pub fn get(&self, kind: &StepKind) -> Option<&HandlerFn> {
        self.handlers.get(kind)
    }
}

impl Default for StepRegistry {
    fn default() -> Self {
        Self::new()
    }
}
