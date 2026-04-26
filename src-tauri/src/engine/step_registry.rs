use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use rusqlite::Connection;

use crate::engine::workflows::{handlers, StepKind, StepResult, WorkflowStep};
use crate::models::task::Task;

/// Register a synchronous handler that runs inside `tokio::task::spawn_blocking`.
///
/// **Simple variant** (task + project_path only):
/// ```rust
/// register_blocking!(handlers, StepKind::LandingPageSpecWrite,
///     crate::engine::exec::research::exec_landing_page_spec_write);
/// ```
///
/// **With provider variant** (task + project_path + ctx field):
/// ```rust
/// register_blocking!(handlers, StepKind::ClusterLinkStrategy,
///     crate::engine::exec::content::exec_cluster_link_strategy, agent_provider);
/// ```
macro_rules! register_blocking {
    ($registry:ident, $kind:expr, $fn:path) => {
        $registry.insert($kind, Box::new(|_step, ctx| {
            let task = ctx.task.clone();
            let project_path = ctx.project_path.to_string();
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    $fn(&task, &project_path)
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: format!("Step panicked: {}", e),
                    output: None,
                })
            })
        }))
    };
    ($registry:ident, $kind:expr, $fn:path, $provider:ident) => {
        $registry.insert($kind, Box::new(|_step, ctx| {
            let task = ctx.task.clone();
            let project_path = ctx.project_path.to_string();
            let provider = ctx.$provider.to_string();
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    $fn(&task, &project_path, &provider)
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: format!("Step panicked: {}", e),
                    output: None,
                })
            })
        }))
    };
    ($registry:ident, $kind:expr, $fn:path, $provider:ident, step) => {
        $registry.insert($kind, Box::new(|step, ctx| {
            let step = step.clone();
            let task = ctx.task.clone();
            let project_path = ctx.project_path.to_string();
            let provider = ctx.$provider.to_string();
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    $fn(&step, &task, &project_path, &provider)
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: format!("Step panicked: {}", e),
                    output: None,
                })
            })
        }))
    };
}

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

        handlers.insert(StepKind::Deterministic, Box::new(|step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            let seo_provider = ctx.seo_provider;
            Box::pin(async move {
                handlers::exec_deterministic(step, task, project_path, seo_provider).await
            })
        }));

        handlers.insert(StepKind::Agentic, Box::new(|step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            let site_url = ctx.site_url;
            let agent_provider = ctx.agent_provider;
            let latest_raw = ctx.latest_raw;
            Box::pin(async move {
                handlers::exec_agentic(step, task, project_path, site_url, agent_provider, latest_raw).await
            })
        }));

        handlers.insert(StepKind::Manual, Box::new(|step, _ctx| {
            let name = step.name.clone();
            Box::pin(async move {
                StepResult {
                    success: true,
                    message: format!("Manual step '{}' — requires user action", name),
                    output: None,
                }
            })
        }));

        handlers.insert(StepKind::Normalizer, Box::new(|step, ctx| {
            let name = step.name.clone();
            let latest_raw = ctx.latest_raw;
            Box::pin(async move {
                if let Some(raw) = latest_raw {
                    let normalized = crate::engine::normalizer::normalize_agent_output(raw);
                    let msg = if normalized.success {
                        format!(
                            "Normalized via '{}' — {} chars",
                            normalized.extraction_method,
                            normalized.raw_output.len()
                        )
                    } else {
                        format!("Normalizer recorded raw output ({} chars)", normalized.raw_output.len())
                    };
                    let output_str = normalized
                        .json_artifact
                        .as_ref()
                        .and_then(|v| serde_json::to_string_pretty(v).ok())
                        .unwrap_or_else(|| normalized.raw_output.clone());
                    StepResult {
                        success: true,
                        message: msg,
                        output: Some(output_str),
                    }
                } else {
                    StepResult {
                        success: true,
                        message: format!("Normalizer step '{}' — no raw output to normalize", name),
                        output: None,
                    }
                }
            })
        }));

        handlers.insert(StepKind::ClusterLinkScan, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::content::exec_cluster_link_scan(task, project_path)
            })
        }));

        register_blocking!(handlers, StepKind::ClusterLinkStrategy,
            crate::engine::exec::content::exec_cluster_link_strategy, agent_provider);

        handlers.insert(StepKind::ClusterLinkApply, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::content::exec_cluster_link_apply(task, project_path)
            })
        }));

        register_blocking!(handlers, StepKind::ContentReviewRecommend,
            crate::engine::exec::content::exec_content_review_recommend, agent_provider);

        register_blocking!(handlers, StepKind::ContentReviewApplyExecute,
            crate::engine::exec::content::exec_content_review_apply, agent_provider);

        register_blocking!(handlers, StepKind::KeywordResearchNative,
            crate::engine::exec::keywords::exec_keyword_research_native, seo_provider);

        handlers.insert(StepKind::ResearchFinalSelection, Box::new(|_step, ctx| {
            let task = ctx.task.clone();
            let project_path = ctx.project_path.to_string();
            let previous_output = ctx.latest_raw.map(|s| s.to_string());
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    let rt = tokio::runtime::Handle::current();
                    rt.block_on(async {
                        crate::engine::exec::research::exec_research_final_selection(
                            &task,
                            &project_path,
                            previous_output.as_deref(),
                        )
                        .await
                    })
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: format!("Step panicked: {}", e),
                    output: None,
                })
            })
        }));

        register_blocking!(handlers, StepKind::LandingPageSpecWrite,
            crate::engine::exec::research::exec_landing_page_spec_write);

        register_blocking!(handlers, StepKind::ResearchAutocomplete,
            crate::engine::exec::research::exec_research_autocomplete);

        register_blocking!(handlers, StepKind::RedditConfigParse,
            crate::engine::exec::reddit::exec_reddit_config_parse, agent_provider);

        handlers.insert(StepKind::RedditSearch, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::reddit::exec_reddit_search(task, project_path).await
            })
        }));

        handlers.insert(StepKind::RedditEnrich, Box::new(|_step, _ctx| {
            Box::pin(async move {
                StepResult {
                    success: true,
                    message: "Reddit enrichment pass — starting AI scoring loop".to_string(),
                    output: None,
                }
            })
        }));

        handlers.insert(StepKind::RedditFetchResults, Box::new(|_step, _ctx| {
            Box::pin(async move {
                StepResult {
                    success: true,
                    message: "Reddit results fetch — starting DB query".to_string(),
                    output: None,
                }
            })
        }));

        handlers.insert(StepKind::ContentSync, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::content::exec_content_sync(task, project_path)
            })
        }));

        handlers.insert(StepKind::FormatValidation, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::content::exec_format_validation(task, project_path)
            })
        }));

        handlers.insert(StepKind::FormatFix, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::content::exec_format_fix(task, project_path)
            })
        }));

        handlers.insert(StepKind::GscSyncArticles, Box::new(|_step, ctx| {
            let task = ctx.task.clone();
            let project_path = ctx.project_path.to_string();
            let gsc_token = ctx.gsc_token.map(|s| s.to_string());
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    crate::engine::exec::gsc::exec_gsc_sync_articles(&task, &project_path, gsc_token.as_deref())
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: format!("Step panicked: {}", e),
                    output: None,
                })
            })
        }));

        handlers.insert(StepKind::GscSummarise, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::gsc::exec_gsc_summarise(task, project_path)
            })
        }));

        handlers.insert(StepKind::IndexingFixContext, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::indexing_fix::exec_indexing_fix_context(task, project_path)
            })
        }));

        handlers.insert(StepKind::IndexingFixApply, Box::new(|_step, ctx| {
            let task = ctx.task.clone();
            let project_path = ctx.project_path.to_string();
            let agent_provider = ctx.agent_provider.to_string();
            let context_json = ctx.latest_raw.map(|s| s.to_string());
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    crate::engine::exec::indexing_fix::exec_indexing_fix_apply(
                        &task,
                        &project_path,
                        &agent_provider,
                        context_json.as_deref(),
                    )
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: format!("Step panicked: {}", e),
                    output: None,
                })
            })
        }));

        handlers.insert(StepKind::ContentAudit, Box::new(|_step, ctx| {
            let task = ctx.task.clone();
            let project_path = ctx.project_path.to_string();
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    crate::engine::exec::content_audit::exec_content_audit(&task, &project_path)
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: format!("Step panicked: {}", e),
                    output: None,
                })
            })
        }));

        handlers.insert(StepKind::CollectGscInspect, Box::new(|_step, ctx| {
            let task = ctx.task.clone();
            let project_path = ctx.project_path.to_string();
            let gsc_token = ctx.gsc_token.map(|s| s.to_string());
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    crate::engine::exec::gsc::exec_collect_gsc(&task, &project_path, gsc_token.as_deref())
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: format!("Step panicked: {}", e),
                    output: None,
                })
            })
        }));

        handlers.insert(StepKind::IndexingDiagnosticsRun, Box::new(|_step, ctx| {
            let result = crate::engine::exec::gsc_diagnostics::exec_indexing_diagnostics(
                ctx.task, ctx.project_path, ctx.gsc_token, ctx.conn,
            );
            Box::pin(async move { result })
        }));

        register_blocking!(handlers, StepKind::GscInvestigateAgentic,
            crate::engine::exec::gsc::exec_gsc_investigate, agent_provider, step);

        handlers.insert(StepKind::SocialCollectSources, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::social::exec_social_collect_sources(task, project_path)
            })
        }));

        handlers.insert(StepKind::SocialLoadTemplates, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::social::exec_social_load_templates(task, project_path)
            })
        }));

        register_blocking!(handlers, StepKind::SocialGeneratePosts,
            crate::engine::exec::social::exec_social_generate_posts, agent_provider, step);

        handlers.insert(StepKind::SocialBuildVisuals, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::social::exec_social_build_visuals(task, project_path)
            })
        }));

        handlers.insert(StepKind::SocialSaveCampaign, Box::new(|_step, ctx| {
            let result =
                crate::engine::exec::social::exec_social_save_campaign(ctx.task, ctx.project_path, ctx.conn);
            Box::pin(async move { result })
        }));

        register_blocking!(handlers, StepKind::SocialRegenerateSingle,
            crate::engine::exec::social::exec_social_regenerate_single, agent_provider, step);

        handlers.insert(StepKind::SocialRebuildVisual, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::social::exec_social_rebuild_visual(task, project_path)
            })
        }));

        handlers.insert(StepKind::SocialUpdatePost, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::social::exec_social_update_post(task, project_path)
            })
        }));

        register_blocking!(handlers, StepKind::SocialDesignTemplate,
            crate::engine::exec::social::exec_social_design_template, agent_provider, step);

        handlers.insert(StepKind::SocialSaveTemplate, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::social::exec_social_save_template(task, project_path)
            })
        }));

        handlers.insert(StepKind::CoverageLoadArticles, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::coverage::exec_coverage_load_articles(task, project_path)
            })
        }));

        handlers.insert(StepKind::CoverageClusterAnalysis, Box::new(|_step, ctx| {
            let task = ctx.task.clone();
            let project_path = ctx.project_path.to_string();
            let agent_provider = ctx.agent_provider.to_string();
            let articles_json = ctx.latest_raw.unwrap_or("{}").to_string();
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    crate::engine::exec::coverage::exec_coverage_cluster_analysis(
                        &task,
                        &project_path,
                        &agent_provider,
                        &articles_json,
                    )
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: format!("Step panicked: {}", e),
                    output: None,
                })
            })
        }));

        handlers.insert(StepKind::CoverageSave, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::coverage::exec_coverage_save(task, project_path)
            })
        }));

        handlers.insert(StepKind::RedditPostReply, Box::new(|_step, ctx| {
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
                    crate::engine::exec::reddit::exec_reddit_post_reply(&task, &project_path, &conn)
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: format!("Step panicked: {}", e),
                    output: None,
                })
            })
        }));

        handlers.insert(StepKind::SocialExtractArticle, Box::new(|_step, ctx| {
            let task = ctx.task;
            let project_path = ctx.project_path;
            Box::pin(async move {
                crate::engine::exec::social::exec_social_extract_article(task, project_path)
            })
        }));

        handlers.insert(StepKind::CtrBuildContext, Box::new(|_step, ctx| {
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
                            };
                        }
                    };
                    crate::engine::exec::ctr_audit::exec_ctr_build_context(&task, &project_path, &conn)
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: format!("Step panicked: {}", e),
                    output: None,
                })
            })
        }));

        handlers.insert(StepKind::CtrAnalyze, Box::new(|_step, ctx| {
            let task = ctx.task.clone();
            let project_path = ctx.project_path.to_string();
            let agent_provider = ctx.agent_provider.to_string();
            let context_json = ctx.latest_raw.unwrap_or("{}").to_string();
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    crate::engine::exec::ctr_audit::exec_ctr_analyze(
                        &task, &project_path, &agent_provider, &context_json,
                    )
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: format!("Step panicked: {}", e),
                    output: None,
                })
            })
        }));

        register_blocking!(handlers, StepKind::CanBuildContext,
            crate::engine::exec::cannibalization_audit::exec_can_build_context);

        handlers.insert(StepKind::CanAnalyze, Box::new(|_step, ctx| {
            let task = ctx.task.clone();
            let project_path = ctx.project_path.to_string();
            let agent_provider = ctx.agent_provider.to_string();
            let context_json = ctx.latest_raw.unwrap_or("{}").to_string();
            Box::pin(async move {
                tokio::task::spawn_blocking(move || {
                    crate::engine::exec::cannibalization_audit::exec_can_analyze(
                        &task, &project_path, &agent_provider, &context_json,
                    )
                })
                .await
                .unwrap_or_else(|e| StepResult {
                    success: false,
                    message: format!("Step panicked: {}", e),
                    output: None,
                })
            })
        }));

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
