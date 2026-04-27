mod commands;
mod config;
mod content;
pub mod db;
pub mod engine;
mod error;
mod gsc;
mod live_site;
pub mod logging;
pub mod models;
mod reddit;
mod rig;
mod seo;
mod social;

use commands::{AppState, GscState, SeoState};
use std::sync::Mutex;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .setup(|app| {
            let db_path = app.path().app_data_dir()?.join("pageseeds.db");
            let conn = db::init(&db_path)?;
            // Initialize logging table
            let _ = logging::init_logs_table(&conn);
            // Reset any tasks that were left in_progress from a previous session
            // (e.g. app was closed or crashed mid-execution).
            let _ = conn.execute(
                "UPDATE tasks SET status='todo', updated_at=?1 WHERE status='in_progress'",
                rusqlite::params![chrono::Utc::now().to_rfc3339()],
            );
            // Startup self-check: log registry counts for debugging silent misconfigurations
            let handlers = engine::workflows::handlers::default_handlers();
            log::info!(
                "[startup] Registered {} workflow handlers, {} Tauri commands",
                handlers.len(),
                85 // Approximate count; hard-coded because tauri::generate_handler! is macro-generated
            );
            app.manage(AppState {
                db: std::sync::Arc::new(std::sync::Mutex::new(conn)),
                db_path: db_path.clone(),
            });
            app.manage(GscState {
                token: Mutex::new(None),
            });
            app.manage(SeoState {
                sig_cache: Mutex::new(std::collections::HashMap::new()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_projects,
            commands::create_project,
            commands::update_project,
            commands::delete_project,
            commands::list_tasks,
            commands::get_task,
            commands::create_task,
            commands::update_task_status,
            commands::update_task,
            commands::delete_task,
            commands::cancel_task,
            commands::create_article_tasks_from_keywords,
            commands::list_articles,
            commands::list_live_site_pages,
            commands::get_live_site_audit,
            commands::import_from_repo,
            commands::import_live_site,
            commands::scan_live_site_links,
            commands::sync_live_site_gsc,
            commands::export_to_repo,
            commands::get_secrets_status,
            commands::get_secrets_file_path,
            commands::resolve_content_dir,
            commands::scan_content_health,
            commands::fix_content_dates,
            commands::analyze_article_date_policy,
            commands::suggest_next_article_publish_date,
            commands::scan_content_links,
            commands::analyze_article_readability,
            commands::compare_competitor_content,
            commands::analyze_keyword_density,
            commands::search_reddit,
            commands::list_reddit_opportunities,
            commands::upsert_reddit_opportunity,
            commands::mark_reddit_posted,
            commands::mark_reddit_skipped,
            commands::post_to_reddit,
            commands::import_env_file,
            commands::get_reddit_statistics,
            commands::validate_reddit_reply,
            commands::migrate_reddit_db,
            commands::draft_reddit_reply,
            commands::enrich_reddit_opportunities,
            commands::create_reddit_reply_tasks,
            commands::gsc_get_auth_status,
            commands::gsc_authenticate,
            commands::gsc_oauth_start,
            commands::gsc_fetch_analytics,
            commands::gsc_fetch_queries_for_page,
            commands::gsc_compute_movers,
            commands::gsc_inspect_urls,
            commands::gsc_generate_indexing_report,
            commands::gsc_parse_coverage_csv,
            commands::gsc_parse_redirect_csv,
            commands::seo_get_keyword_ideas,
            commands::seo_get_keyword_difficulty,
            commands::seo_batch_keyword_difficulty,
            commands::seo_get_backlinks,
            commands::seo_check_traffic,
            commands::get_seo_provider,
            commands::set_seo_provider,
            commands::classify_search_intent,
            commands::score_keyword_opportunities,
            // Phase 6 — Workflow Engine + Batch + Scheduler + Ledger
            commands::execute_task,
            commands::dry_run_task,
            commands::get_batch_summary,
            commands::run_batch,
            commands::list_scheduler_rules,
            commands::upsert_scheduler_rule,
            commands::delete_scheduler_rule,
            commands::set_scheduler_rule_enabled,
            commands::run_scheduler_cycle,
            commands::list_ledger_runs,
            commands::get_ledger_run_summary,
            commands::get_ledger_run_events,
            // Phase 7 — Skills, Prompts, and Agent Interaction
            commands::list_skills,
            commands::get_skill,
            commands::check_embedding_status,
            commands::index_skills,
            commands::search_skills,
            commands::build_prompt_preview,
            commands::list_task_artifacts,
            commands::get_project_overview,
            commands::quick_run_workflow,
            commands::check_agent_status,
            commands::set_agent_provider,
            commands::get_global_agent_provider,
            commands::get_kimi_backend_mode,
            commands::set_kimi_backend_mode,
            commands::get_global_settings,
            commands::check_agent_status_for_project,
            commands::check_project_setup,
            commands::get_project_config_files_status,
            commands::init_workspace_config,
            commands::initialize_project_workspace,
            commands::get_content_health,
            commands::fix_date_mismatches,
            commands::repair_article_paths,
            commands::get_ctr_health_summary,
            commands::validate_content_format,
            commands::fix_content_format,
            commands::ingest_orphan_articles,
            commands::clean_stale_articles,
            commands::get_keyword_coverage,
            commands::preflight_publish_articles,
            commands::apply_publish_articles,
            commands::resolve_year_mismatch_agent,
            // Phase 8 — Social Media Marketing
            commands::list_social_campaigns,
            commands::get_social_campaign,
            commands::create_social_campaign,
            commands::delete_social_campaign,
            commands::get_campaign_posts,
            commands::get_social_post,
            commands::update_social_post_status,
            commands::update_social_post,
            commands::schedule_social_post,
            commands::mark_social_post_posted,
            commands::delete_social_post,
            commands::list_social_templates,
            commands::get_social_template,
            commands::create_social_template,
            commands::delete_social_template,
            commands::get_social_campaign_stats,
            commands::get_social_posts_by_project,
            commands::run_social_campaign,
            // Task Queue
            commands::execute_queue,
            commands::mark_tasks_queued,
            commands::mark_tasks_todo,
            commands::pause_queue,
            commands::resume_queue,
            commands::clear_completed_queue_items,
            // Logging
            commands::get_log_file_path,
            commands::submit_log,
            commands::submit_logs_batch,
            commands::query_logs,
            commands::get_recent_logs,
            commands::get_log_stats,
            commands::clear_old_logs,
            commands::export_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
