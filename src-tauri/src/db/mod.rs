use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;

use crate::error::Result;

pub mod content_audit;
pub mod export;
pub mod global_settings;
pub mod research_shortlist;
pub mod seo_discovery;

/// Get the default database path based on platform conventions.
/// Used when we need to access the DB without having the AppState.
///
/// Can be overridden via the `PAGESEEDS_DB_PATH` environment variable for testing.
pub fn default_db_path() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("PAGESEEDS_DB_PATH") {
        return std::path::PathBuf::from(path);
    }

    let app_dir = dirs::data_dir()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
        .join("com.pageseeds.app");

    // Ensure directory exists
    let _ = std::fs::create_dir_all(&app_dir);

    app_dir.join("pageseeds.db")
}

static MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS schema_version (
    version     INTEGER PRIMARY KEY,
    applied_at  TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS projects (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    path        TEXT NOT NULL,
    content_dir TEXT,
    site_url    TEXT,
    site_id     TEXT,
    sitemap_url TEXT,
    project_mode TEXT NOT NULL DEFAULT 'workspace',
    active      INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS tasks (
    id              TEXT PRIMARY KEY,
    type            TEXT NOT NULL,
    phase           TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'todo',
    priority        TEXT NOT NULL DEFAULT 'medium',
    execution_mode  TEXT NOT NULL DEFAULT 'manual',
    agent_policy    TEXT NOT NULL DEFAULT 'none',
    title           TEXT,
    description     TEXT,
    project_id      TEXT NOT NULL,
    depends_on      TEXT NOT NULL DEFAULT '[]',
    artifacts       TEXT NOT NULL DEFAULT '[]',
    run_attempts    INTEGER NOT NULL DEFAULT 0,
    run_last_error  TEXT,
    run_provider    TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id)
);

CREATE INDEX IF NOT EXISTS idx_tasks_project ON tasks(project_id);
CREATE INDEX IF NOT EXISTS idx_tasks_status  ON tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_phase   ON tasks(phase);

CREATE TABLE IF NOT EXISTS articles (
    id                          INTEGER NOT NULL,
    title                       TEXT NOT NULL DEFAULT '',
    url_slug                    TEXT NOT NULL DEFAULT '',
    file                        TEXT NOT NULL DEFAULT '',
    target_keyword              TEXT,
    keyword_difficulty          TEXT,
    target_volume               INTEGER DEFAULT 0,
    published_date              TEXT,
    word_count                  INTEGER DEFAULT 0,
    status                      TEXT NOT NULL DEFAULT 'draft',
    content_gaps_addressed      TEXT NOT NULL DEFAULT '[]',
    estimated_traffic_monthly   TEXT,
    project_id                  TEXT NOT NULL,
    PRIMARY KEY (id, project_id),
    FOREIGN KEY (project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS articles_meta (
    project_id      TEXT PRIMARY KEY,
    next_article_id INTEGER NOT NULL DEFAULT 1,
    FOREIGN KEY (project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS reddit_opportunities (
    post_id             TEXT PRIMARY KEY,
    title               TEXT,
    url                 TEXT,
    subreddit           TEXT,
    author              TEXT,
    posted_date         TEXT,
    upvotes             INTEGER,
    comment_count       INTEGER,
    relevance_score     REAL,
    engagement_score    REAL,
    accessibility_score REAL,
    final_score         REAL,
    severity            TEXT,
    why_relevant        TEXT,
    key_pain_points     TEXT,
    website_fit         TEXT,
    reply_status        TEXT DEFAULT 'pending',
    reply_text          TEXT,
    reply_url           TEXT,
    reply_upvotes       INTEGER,
    reply_replies       INTEGER,
    posted_at           TEXT,
    project_id          TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS scheduler_rules (
    rule_id         TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL,
    task_type       TEXT NOT NULL,
    action          TEXT NOT NULL DEFAULT 'create_task',
    interval_hours  INTEGER NOT NULL,
    priority        TEXT NOT NULL DEFAULT 'medium',
    phase           TEXT NOT NULL,
    enabled         INTEGER NOT NULL DEFAULT 1,
    last_run_at     TEXT,
    FOREIGN KEY (project_id) REFERENCES projects(id)
);
"#;

static MIGRATION_V7: &str = r#"
-- Idempotency tracking for task creation
CREATE TABLE IF NOT EXISTS task_idempotency_keys (
    key TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_idempotency_task ON task_idempotency_keys(task_id);
"#;

static MIGRATION_V8: &str = r#"
-- Add image_generation_prompt to social_posts for AI image generation workflow
ALTER TABLE social_posts ADD COLUMN image_generation_prompt TEXT;
"#;

static MIGRATION_V9: &str = r#"
-- Per-URL GSC indexing status for stateful diagnostics
CREATE TABLE IF NOT EXISTS gsc_url_indexing_status (
    url                 TEXT NOT NULL,
    project_id          TEXT NOT NULL,
    last_inspected_at   TEXT,
    last_reason_code    TEXT,
    last_verdict        TEXT,
    last_action         TEXT,
    consecutive_passes  INTEGER NOT NULL DEFAULT 0,
    last_task_created_at TEXT,
    last_task_type      TEXT,
    last_task_id        TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    PRIMARY KEY (url, project_id),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_gsc_url_status_project ON gsc_url_indexing_status(project_id);
CREATE INDEX IF NOT EXISTS idx_gsc_url_status_reason ON gsc_url_indexing_status(last_reason_code);
CREATE INDEX IF NOT EXISTS idx_gsc_url_status_inspected ON gsc_url_indexing_status(last_inspected_at);
"#;

static MIGRATION_V10: &str = r#"
-- Track fix history per URL for better diagnostics
ALTER TABLE gsc_url_indexing_status ADD COLUMN last_fix_summary TEXT;
ALTER TABLE gsc_url_indexing_status ADD COLUMN fix_attempt_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE gsc_url_indexing_status ADD COLUMN last_task_resolved_at TEXT;

CREATE INDEX IF NOT EXISTS idx_gsc_url_status_resolved ON gsc_url_indexing_status(last_task_resolved_at);
"#;

static MIGRATION_V11: &str = r#"
-- NO-OP: V11 was an incomplete skill_embeddings migration.
-- V12 supersedes it with the final schema. This no-op remains so
-- existing databases that already applied V11 do not re-run it.
"#;

static MIGRATION_V12: &str = r#"
-- Skill embeddings for semantic search (Rig.rs integration)
-- SUPersedes V11: V11 was an incomplete/leftover migration with the same
-- table name but a slightly different comment. V12 is the canonical schema.
CREATE TABLE IF NOT EXISTS skill_embeddings (
    skill_name      TEXT PRIMARY KEY,
    project_id      TEXT NOT NULL,
    content_hash    TEXT NOT NULL,        -- Hash of content to detect changes
    embedding       BLOB NOT NULL,        -- Serialized vector (f32 array)
    model_name      TEXT NOT NULL,        -- E.g., "nomic-embed-text"
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_skill_embeddings_project ON skill_embeddings(project_id);
"#;

static MIGRATION_V16: &str = r#"
-- Track durable content review state per article
ALTER TABLE articles ADD COLUMN review_status TEXT;
ALTER TABLE articles ADD COLUMN review_started_at TEXT;
ALTER TABLE articles ADD COLUMN last_reviewed_at TEXT;
ALTER TABLE articles ADD COLUMN review_count INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_articles_review_status ON articles(project_id, review_status);
CREATE INDEX IF NOT EXISTS idx_articles_last_reviewed ON articles(project_id, last_reviewed_at);
"#;

static MIGRATION_V17: &str = r#"
-- Track whether a project is repo-backed or live-site-backed
ALTER TABLE projects ADD COLUMN project_mode TEXT NOT NULL DEFAULT 'workspace';
"#;

static MIGRATION_V18: &str = r#"
-- Normalized live-site inventory for non-repo projects
CREATE TABLE IF NOT EXISTS live_site_pages (
    project_id           TEXT NOT NULL,
    url                  TEXT NOT NULL,
    path                 TEXT NOT NULL,
    title                TEXT NOT NULL DEFAULT '',
    meta_description     TEXT,
    h1                   TEXT,
    content_excerpt      TEXT,
    word_count           INTEGER NOT NULL DEFAULT 0,
    heading_count        INTEGER NOT NULL DEFAULT 0,
    internal_links_out   INTEGER NOT NULL DEFAULT 0,
    status_code          INTEGER,
    gsc_clicks           REAL,
    gsc_impressions      REAL,
    gsc_ctr              REAL,
    gsc_position         REAL,
    gsc_synced_at        TEXT,
    last_crawled_at      TEXT NOT NULL,
    PRIMARY KEY (project_id, url),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_live_site_pages_project_path ON live_site_pages(project_id, path);

CREATE TABLE IF NOT EXISTS live_site_links (
    project_id     TEXT NOT NULL,
    source_url     TEXT NOT NULL,
    target_url     TEXT NOT NULL,
    anchor_text    TEXT NOT NULL DEFAULT '',
    created_at     TEXT NOT NULL,
    PRIMARY KEY (project_id, source_url, target_url, anchor_text),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_live_site_links_source ON live_site_links(project_id, source_url);
CREATE INDEX IF NOT EXISTS idx_live_site_links_target ON live_site_links(project_id, target_url);
"#;

static MIGRATION_V19: &str = r#"
-- Optional manual sitemap override for live-site projects
ALTER TABLE projects ADD COLUMN sitemap_url TEXT;
"#;

static MIGRATION_V20: &str = r#"
-- Optional GSC metrics cached on imported live-site pages
ALTER TABLE live_site_pages ADD COLUMN gsc_clicks REAL;
ALTER TABLE live_site_pages ADD COLUMN gsc_impressions REAL;
ALTER TABLE live_site_pages ADD COLUMN gsc_ctr REAL;
ALTER TABLE live_site_pages ADD COLUMN gsc_position REAL;
ALTER TABLE live_site_pages ADD COLUMN gsc_synced_at TEXT;
"#;

static MIGRATION_V21: &str = r#"
-- Persistent audit state per article per audit type
CREATE TABLE IF NOT EXISTS article_audit_state (
    project_id      TEXT NOT NULL,
    article_file    TEXT NOT NULL,
    audit_type      TEXT NOT NULL,
    last_audited_at TEXT NOT NULL,
    was_healthy     INTEGER NOT NULL DEFAULT 0,
    content_hash    TEXT NOT NULL,
    issues_found    TEXT NOT NULL DEFAULT '[]',
    PRIMARY KEY (project_id, article_file, audit_type),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_article_audit_state_project ON article_audit_state(project_id);
CREATE INDEX IF NOT EXISTS idx_article_audit_state_audit ON article_audit_state(project_id, audit_type);
"#;

static MIGRATION_V22: &str = r#"
-- Add JSON embedding storage for rig-based vector search
-- Replaces raw f32 BLOB storage with structured JSON.
ALTER TABLE skill_embeddings ADD COLUMN embedding_json TEXT;
"#;

static MIGRATION_V23: &str = r#"
-- Token usage tracking for LLM observability
ALTER TABLE task_runs ADD COLUMN prompt_tokens INTEGER;
ALTER TABLE task_runs ADD COLUMN completion_tokens INTEGER;
"#;

static MIGRATION_V24: &str = r#"
-- Approval state for cannibalization strategy recommendations
CREATE TABLE IF NOT EXISTS strategy_reviews (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    strategy_id         TEXT NOT NULL,
    project_id          TEXT NOT NULL,
    recommendation_type TEXT NOT NULL,
    recommendation_id   TEXT NOT NULL,
    approval_status     TEXT NOT NULL DEFAULT 'pending',
    approved_by         TEXT,
    approved_at         TEXT,
    notes               TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    UNIQUE(strategy_id, recommendation_type, recommendation_id),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_strategy_reviews_project ON strategy_reviews(project_id);
CREATE INDEX IF NOT EXISTS idx_strategy_reviews_strategy ON strategy_reviews(strategy_id);
"#;

static MIGRATION_V25: &str = r#"
-- Query-level GSC metrics for CTR audit context
CREATE TABLE IF NOT EXISTS ctr_query_metrics (
    project_id      TEXT NOT NULL,
    article_id      INTEGER NOT NULL,
    page_url        TEXT NOT NULL,
    query           TEXT NOT NULL,
    impressions     REAL NOT NULL DEFAULT 0,
    clicks          REAL NOT NULL DEFAULT 0,
    ctr             REAL NOT NULL DEFAULT 0,
    avg_position    REAL NOT NULL DEFAULT 0,
    period_start    TEXT,
    period_end      TEXT,
    intent          TEXT,
    fetched_at      TEXT NOT NULL,
    PRIMARY KEY (project_id, article_id, query),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_ctr_query_metrics_project ON ctr_query_metrics(project_id);
CREATE INDEX IF NOT EXISTS idx_ctr_query_metrics_article ON ctr_query_metrics(project_id, article_id);
"#;

static MIGRATION_V26: &str = r#"
-- Rendered SERP audit results per page
CREATE TABLE IF NOT EXISTS ctr_rendered_page_audits (
    project_id              TEXT NOT NULL,
    article_id              INTEGER NOT NULL,
    url                     TEXT NOT NULL,
    file                    TEXT NOT NULL,
    source_title            TEXT NOT NULL DEFAULT '',
    rendered_title          TEXT NOT NULL DEFAULT '',
    rendered_title_length   INTEGER NOT NULL DEFAULT 0,
    title_issue_source      TEXT NOT NULL DEFAULT 'unknown',
    source_description      TEXT NOT NULL DEFAULT '',
    rendered_description    TEXT,
    canonical_url           TEXT,
    rendered_h1             TEXT,
    schema_types_json       TEXT NOT NULL DEFAULT '[]',
    has_rendered_faq_page   INTEGER NOT NULL DEFAULT 0,
    snippet_markup_json     TEXT NOT NULL DEFAULT '{}',
    issues_json             TEXT NOT NULL DEFAULT '[]',
    checked_at              TEXT NOT NULL,
    PRIMARY KEY (project_id, article_id),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_ctr_rendered_audits_project ON ctr_rendered_page_audits(project_id);
"#;

static MIGRATION_V27: &str = r#"
-- Flexible sidecar metadata for articles (GSC, quality, analytics, custom)
CREATE TABLE IF NOT EXISTS article_metadata (
    project_id      TEXT NOT NULL,
    article_id      INTEGER NOT NULL,
    namespace       TEXT NOT NULL,
    payload         TEXT NOT NULL DEFAULT '{}',
    updated_at      TEXT NOT NULL,
    PRIMARY KEY (project_id, article_id, namespace),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_article_metadata_project ON article_metadata(project_id);
CREATE INDEX IF NOT EXISTS idx_article_metadata_article ON article_metadata(project_id, article_id);
"#;

static MIGRATION_V28: &str = r#"
-- Add rendered_faq_question_count to ctr_rendered_page_audits
ALTER TABLE ctr_rendered_page_audits ADD COLUMN rendered_faq_question_count INTEGER NOT NULL DEFAULT 0;
"#;

static MIGRATION_V29: &str = r#"
-- CTR outcome tracking: before/after metrics per article fix
CREATE TABLE IF NOT EXISTS ctr_outcomes (
    project_id          TEXT NOT NULL,
    article_id          INTEGER NOT NULL,
    fix_task_id         TEXT NOT NULL,
    baseline_start      TEXT NOT NULL,
    baseline_end        TEXT NOT NULL,
    after_start         TEXT,
    after_end           TEXT,
    baseline_clicks     REAL NOT NULL DEFAULT 0,
    baseline_impressions REAL NOT NULL DEFAULT 0,
    baseline_ctr        REAL NOT NULL DEFAULT 0,
    baseline_position   REAL NOT NULL DEFAULT 0,
    after_clicks        REAL,
    after_impressions   REAL,
    after_ctr           REAL,
    after_position      REAL,
    position_delta      REAL,
    outcome_status      TEXT NOT NULL DEFAULT 'pending',
    deployed_at         TEXT,
    reviewed_at         TEXT,
    PRIMARY KEY (project_id, article_id, fix_task_id),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_ctr_outcomes_project ON ctr_outcomes(project_id);
CREATE INDEX IF NOT EXISTS idx_ctr_outcomes_task ON ctr_outcomes(fix_task_id);
"#;

static MIGRATION_V30: &str = r#"
-- CTR issue lifecycle: durable state for per-article CTR issues
CREATE TABLE IF NOT EXISTS article_ctr_issues (
    project_id                  TEXT NOT NULL,
    article_id                  INTEGER NOT NULL,
    issue_type                  TEXT NOT NULL,
    status                      TEXT NOT NULL DEFAULT 'open',
    detected_at                 TEXT NOT NULL,
    last_verified_at            TEXT,
    content_hash_at_detection   TEXT NOT NULL DEFAULT '',
    fix_task_id                 TEXT,
    failure_reason              TEXT,
    verified_hash               TEXT,
    PRIMARY KEY (project_id, article_id, issue_type),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_article_ctr_issues_project ON article_ctr_issues(project_id);
CREATE INDEX IF NOT EXISTS idx_article_ctr_issues_status ON article_ctr_issues(status);
"#;

static MIGRATION_V32: &str = r#"
-- Add task execution policy columns (replaces overloaded execution_mode)
ALTER TABLE tasks ADD COLUMN run_policy TEXT NOT NULL DEFAULT 'user_enqueue';
ALTER TABLE tasks ADD COLUMN review_surface TEXT NOT NULL DEFAULT 'none';
ALTER TABLE tasks ADD COLUMN follow_up_policy TEXT NOT NULL DEFAULT 'none';

-- Map legacy execution_mode values to new run_policy
UPDATE tasks SET run_policy = 'auto_enqueue' WHERE execution_mode IN ('automatic', 'batchable');
UPDATE tasks SET run_policy = 'user_enqueue' WHERE execution_mode IN ('manual', 'spec');

-- Backfill review_surface for task types that previously had review_on_success=true
UPDATE tasks SET review_surface = 'keyword_picker' WHERE type IN ('research_keywords', 'custom_keyword_research', 'research_landing_pages');
UPDATE tasks SET review_surface = 'reddit_picker' WHERE type = 'reddit_opportunity_search';
UPDATE tasks SET review_surface = 'artifact_review' WHERE type IN ('fix_ctr_site_template', 'consolidate_cluster', 'create_hub_page', 'refresh_hub_page', 'territory_research', 'calculator_rollout');
UPDATE tasks SET review_surface = 'follow_up_tasks' WHERE type = 'content_review';

-- Backfill follow_up_policy for task types that spawn follow-ups
UPDATE tasks SET follow_up_policy = 'backend_auto' WHERE type IN ('collect_gsc', 'ctr_audit', 'cannibalization_audit', 'content_review', 'indexing_diagnostics', 'link_audit', 'keyword_gap_analysis', 'analyze_gsc_performance', 'analyze_keyword_coverage', 'fix_ctr_site_template', 'consolidate_cluster', 'create_hub_page', 'refresh_hub_page', 'territory_research', 'calculator_rollout');
UPDATE tasks SET follow_up_policy = 'user_selection' WHERE type IN ('research_keywords', 'custom_keyword_research', 'research_landing_pages', 'reddit_opportunity_search');
"#;

static MIGRATION_V38: &str = r#"
-- Reset over-long GSC-backfilled target_keywords (word count > 5) to empty so
-- the next GSC sync re-backfills them through normalize_backfilled_keyword
-- (issue #74).
UPDATE articles
SET target_keyword = ''
WHERE target_keyword IS NOT NULL
  AND target_keyword != ''
  AND length(target_keyword) - length(replace(target_keyword, ' ', '')) + 1 > 5;
"#;

static MIGRATION_V31: &str = r#"
-- Backend-owned task queue: durable queue runs and items
CREATE TABLE IF NOT EXISTS queue_runs (
    id              TEXT PRIMARY KEY,
    status          TEXT NOT NULL DEFAULT 'idle',
    pause_on_error  INTEGER NOT NULL DEFAULT 1,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    started_at      TEXT,
    finished_at     TEXT
);

CREATE TABLE IF NOT EXISTS queue_items (
    run_id          TEXT NOT NULL,
    position        INTEGER NOT NULL,
    task_id         TEXT NOT NULL,
    project_id      TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    error           TEXT,
    result_json     TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    PRIMARY KEY (run_id, task_id),
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE,
    FOREIGN KEY (run_id) REFERENCES queue_runs(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_queue_items_run ON queue_items(run_id);
CREATE INDEX IF NOT EXISTS idx_queue_items_task ON queue_items(task_id);
CREATE INDEX IF NOT EXISTS idx_queue_items_status ON queue_items(status);
"#;

static MIGRATION_V6: &str = r#"
-- Social media marketing campaigns
CREATE TABLE IF NOT EXISTS social_campaigns (
    id                  TEXT PRIMARY KEY,
    project_id          TEXT NOT NULL,
    name                TEXT NOT NULL,
    description         TEXT,
    
    -- Source configuration (JSON)
    source_config       TEXT NOT NULL,
    
    -- Target platforms and templates (JSON arrays)
    target_platforms    TEXT NOT NULL,
    template_ids        TEXT NOT NULL,
    
    status              TEXT NOT NULL DEFAULT 'draft',
    post_count          INTEGER DEFAULT 0,
    
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_social_campaigns_project ON social_campaigns(project_id);
CREATE INDEX IF NOT EXISTS idx_social_campaigns_status ON social_campaigns(status);

-- Social media posts
CREATE TABLE IF NOT EXISTS social_posts (
    id                      TEXT PRIMARY KEY,
    campaign_id             TEXT NOT NULL,
    project_id              TEXT NOT NULL,
    
    source_type             TEXT NOT NULL,
    source_id               TEXT NOT NULL,
    source_url              TEXT,
    
    platform                TEXT NOT NULL,
    format                  TEXT NOT NULL,
    
    hook                    TEXT NOT NULL,
    caption                 TEXT NOT NULL,
    hashtags                TEXT NOT NULL,        -- JSON array
    cta                     TEXT NOT NULL,
    
    visual_assets           TEXT NOT NULL,       -- JSON array
    
    status                  TEXT NOT NULL DEFAULT 'draft',
    
    scheduled_at            TEXT,
    posted_at               TEXT,
    platform_post_id        TEXT,
    platform_post_url       TEXT,
    
    metrics                 TEXT,                  -- JSON
    
    template_id             TEXT NOT NULL,
    generated_by            TEXT,
    generation_prompt_hash  TEXT,
    
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,
    
    FOREIGN KEY (campaign_id) REFERENCES social_campaigns(id) ON DELETE CASCADE,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_social_posts_campaign ON social_posts(campaign_id);
CREATE INDEX IF NOT EXISTS idx_social_posts_project ON social_posts(project_id);
CREATE INDEX IF NOT EXISTS idx_social_posts_status ON social_posts(status);
CREATE INDEX IF NOT EXISTS idx_social_posts_scheduled ON social_posts(scheduled_at) 
    WHERE status = 'scheduled';

-- Content templates (global or project-specific)
CREATE TABLE IF NOT EXISTS social_templates (
    id                  TEXT PRIMARY KEY,
    project_id          TEXT,                     -- NULL for global templates
    
    name                TEXT NOT NULL,
    description         TEXT,
    platform            TEXT NOT NULL,
    format              TEXT NOT NULL,
    
    creation_prompt     TEXT NOT NULL,
    overlay_config      TEXT NOT NULL,           -- JSON
    default_hashtags    TEXT NOT NULL,          -- JSON array
    example_output      TEXT,                    -- JSON
    
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_social_templates_project ON social_templates(project_id);
"#;

pub fn init(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    run_migrations(&conn)?;
    Ok(conn)
}

/// Initialize schema on an existing connection (for testing).
/// This allows tests to use in-memory databases while still getting the full schema.
pub fn init_with_conn(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    run_migrations(conn)?;
    Ok(())
}

/// Run a migration step whose failure is tolerated (the statement may already
/// be applied, so idempotency is preserved), but log loudly — a silently
/// swallowed failure with the schema version still recorded hides schema drift
/// (issue #71).
fn run_tolerated_migration(conn: &Connection, version: u32, sql: &str) {
    if let Err(e) = conn.execute_batch(sql) {
        log::error!("[db::run_migrations] V{} migration failed: {}", version, e);
    }
}

fn run_migrations(conn: &Connection) -> Result<()> {
    // unwrap_or(0) handles the case where schema_version doesn't exist yet
    let version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if version < 1 {
        conn.execute_batch(MIGRATION_V1)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (1, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 2 {
        // Add site_id column if it doesn't exist yet (idempotent)
        run_tolerated_migration(conn, 2, "ALTER TABLE projects ADD COLUMN site_id TEXT;");
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (2, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 3 {
        run_tolerated_migration(
            conn,
            3,
            "CREATE TABLE IF NOT EXISTS task_runs (
                id          INTEGER PRIMARY KEY AUTOINCREMENT,
                task_id     TEXT NOT NULL,
                attempt     INTEGER NOT NULL,
                provider    TEXT,
                started_at  TEXT NOT NULL,
                finished_at TEXT,
                success     INTEGER,
                error       TEXT,
                FOREIGN KEY (task_id) REFERENCES tasks(id)
            );
            CREATE INDEX IF NOT EXISTS idx_task_runs_task ON task_runs(task_id);",
        );
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (3, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 4 {
        // Add agent_provider to projects; ignore error if column already exists
        // Note: This is legacy - agent_provider is now global. Default to 'kimi'.
        run_tolerated_migration(
            conn,
            4,
            "ALTER TABLE projects ADD COLUMN agent_provider TEXT DEFAULT 'kimi';",
        );
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (4, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 5 {
        // Add mention_stance column to reddit_opportunities; tolerate "duplicate
        // column" for idempotency, but log loudly — a failed ALTER with the version
        // still recorded hides schema drift (issue #71).
        run_tolerated_migration(
            conn,
            5,
            "ALTER TABLE reddit_opportunities ADD COLUMN mention_stance TEXT;",
        );
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (5, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 6 {
        conn.execute_batch(MIGRATION_V6)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (6, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 7 {
        conn.execute_batch(MIGRATION_V7)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (7, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 8 {
        conn.execute_batch(MIGRATION_V8)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (8, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 9 {
        conn.execute_batch(MIGRATION_V9)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (9, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 10 {
        conn.execute_batch(MIGRATION_V10)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (10, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 11 {
        conn.execute_batch(MIGRATION_V11)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (11, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 12 {
        conn.execute_batch(MIGRATION_V12)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (12, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 13 {
        // Add seo_provider column for dual SEO provider support (Ahrefs / DataForSEO)
        run_tolerated_migration(
            conn,
            13,
            "ALTER TABLE projects ADD COLUMN seo_provider TEXT NOT NULL DEFAULT 'ahrefs';",
        );
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (13, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    // Repair: V13 migration silently failed on some databases (let _ = swallowed the error).
    // Ensure seo_provider column exists regardless of recorded schema version.
    {
        let has_col: bool = conn
            .prepare(
                "SELECT COUNT(*) FROM pragma_table_info('projects') WHERE name='seo_provider'",
            )?
            .query_row([], |r| r.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !has_col {
            conn.execute_batch(
                "ALTER TABLE projects ADD COLUMN seo_provider TEXT NOT NULL DEFAULT 'ahrefs';",
            )?;
        }
    }

    if version < 14 {
        // Add global_settings table for application-wide settings
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS global_settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );",
        )?;
        // Default to 'kimi' as the preferred agent
        conn.execute(
            "INSERT OR IGNORE INTO global_settings (key, value, updated_at) VALUES ('agent_provider', 'kimi', ?1)",
            [&now],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (14, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 15 {
        // Add product_name column to reddit_opportunities for agentic config consumption.
        // Tolerate failure for idempotency, but log loudly (issue #71).
        run_tolerated_migration(
            conn,
            15,
            "ALTER TABLE reddit_opportunities ADD COLUMN product_name TEXT;",
        );
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (15, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 16 {
        run_tolerated_migration(conn, 16, MIGRATION_V16);
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (16, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    // Repair: ensure article review-state columns exist even if V16 partially applied.
    {
        let review_columns = [
            (
                "review_status",
                "ALTER TABLE articles ADD COLUMN review_status TEXT;",
            ),
            (
                "review_started_at",
                "ALTER TABLE articles ADD COLUMN review_started_at TEXT;",
            ),
            (
                "last_reviewed_at",
                "ALTER TABLE articles ADD COLUMN last_reviewed_at TEXT;",
            ),
            (
                "review_count",
                "ALTER TABLE articles ADD COLUMN review_count INTEGER NOT NULL DEFAULT 0;",
            ),
        ];

        for (name, sql) in review_columns {
            let has_col: bool = conn
                .prepare("SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name = ?1")?
                .query_row([name], |r| r.get::<_, i64>(0))
                .unwrap_or(0)
                > 0;
            if !has_col {
                conn.execute_batch(sql)?;
            }
        }

        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_articles_review_status ON articles(project_id, review_status);
             CREATE INDEX IF NOT EXISTS idx_articles_last_reviewed ON articles(project_id, last_reviewed_at);",
        )?;
    }

    if version < 17 {
        run_tolerated_migration(conn, 17, MIGRATION_V17);
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (17, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    // Repair: ensure project_mode exists even if the migration was skipped.
    {
        let has_col: bool = conn
            .prepare(
                "SELECT COUNT(*) FROM pragma_table_info('projects') WHERE name='project_mode'",
            )?
            .query_row([], |r| r.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !has_col {
            conn.execute_batch(
                "ALTER TABLE projects ADD COLUMN project_mode TEXT NOT NULL DEFAULT 'workspace';",
            )?;
        }
    }

    if version < 18 {
        conn.execute_batch(MIGRATION_V18)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (18, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 19 {
        run_tolerated_migration(conn, 19, MIGRATION_V19);
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (19, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 20 {
        run_tolerated_migration(conn, 20, MIGRATION_V20);
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (20, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 21 {
        conn.execute_batch(MIGRATION_V21)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (21, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 22 {
        run_tolerated_migration(conn, 22, MIGRATION_V22);
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (22, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 23 {
        run_tolerated_migration(conn, 23, MIGRATION_V23);
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (23, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 24 {
        conn.execute_batch(MIGRATION_V24)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (24, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 25 {
        conn.execute_batch(MIGRATION_V25)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (25, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 26 {
        conn.execute_batch(MIGRATION_V26)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (26, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 27 {
        conn.execute_batch(MIGRATION_V27)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (27, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 28 {
        conn.execute_batch(MIGRATION_V28)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (28, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 29 {
        conn.execute_batch(MIGRATION_V29)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (29, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 30 {
        conn.execute_batch(MIGRATION_V30)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (30, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 31 {
        conn.execute_batch(MIGRATION_V31)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (31, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 32 {
        conn.execute_batch(MIGRATION_V32)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (32, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 33 {
        // Migrate tasks that failed but were left in 'todo' status to the new 'failed' status.
        // A task is considered failed if it has a last_error and at least one attempt.
        conn.execute(
            "UPDATE tasks SET status = 'failed', updated_at = ?1 WHERE status = 'todo' AND run_attempts > 0 AND run_last_error IS NOT NULL",
            [chrono::Utc::now().to_rfc3339()],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (33, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 34 {
        // Add expiration support to idempotency keys for cooldown-based deduplication
        conn.execute_batch(
            "ALTER TABLE task_idempotency_keys ADD COLUMN expires_at TEXT;
             CREATE INDEX IF NOT EXISTS idx_idempotency_expires ON task_idempotency_keys(expires_at);
             CREATE INDEX IF NOT EXISTS idx_tasks_project_type_status ON tasks(project_id, type, status);",
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (34, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 35 {
        // Update existing cannibalization_audit tasks to use the new task-drawer picker flow.
        conn.execute(
            "UPDATE tasks SET review_surface = 'cannibalization_picker', follow_up_policy = 'user_selection', updated_at = ?1 WHERE type = 'cannibalization_audit'",
            [chrono::Utc::now().to_rfc3339()],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (35, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 36 {
        // Add page_type to articles for hub/pillar/spoke classification
        run_tolerated_migration(conn, 36, "ALTER TABLE articles ADD COLUMN page_type TEXT;");
        run_tolerated_migration(
            conn,
            36,
            "CREATE INDEX IF NOT EXISTS idx_articles_page_type ON articles(project_id, page_type);",
        );
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (36, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 37 {
        // Switch all installs to the native Kimi CLI provider (kimi --print via
        // tokio::process). This removes the dependency on the Python HTTP bridge.
        // Users can manually switch back to "bridge" or "auto" in settings if needed.
        conn.execute(
            "UPDATE global_settings SET value = 'cli', updated_at = ?1 WHERE key = 'kimi_backend_mode'",
            [chrono::Utc::now().to_rfc3339()],
        )?;
        // Also set the default for installs that don't have the key yet.
        conn.execute(
            "INSERT OR IGNORE INTO global_settings (key, value, updated_at) VALUES ('kimi_backend_mode', 'cli', ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (37, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 38 {
        conn.execute_batch(MIGRATION_V38)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (38, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    // Repair: ensure page_type column exists even if V36 was skipped
    {
        let has_col: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name='page_type'")?
            .query_row([], |r| r.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !has_col {
            conn.execute_batch("ALTER TABLE articles ADD COLUMN page_type TEXT;")?;
            conn.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_articles_page_type ON articles(project_id, page_type);"
            )?;
        }
    }

    // Repair: ensure sitemap_url exists even if the migration was skipped.
    {
        let has_col: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('projects') WHERE name='sitemap_url'")?
            .query_row([], |r| r.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !has_col {
            conn.execute_batch("ALTER TABLE projects ADD COLUMN sitemap_url TEXT;")?;
        }
    }

    // Repair: ensure live-site GSC metric columns exist even if the migration was skipped.
    for column in [
        (
            "gsc_clicks",
            "ALTER TABLE live_site_pages ADD COLUMN gsc_clicks REAL;",
        ),
        (
            "gsc_impressions",
            "ALTER TABLE live_site_pages ADD COLUMN gsc_impressions REAL;",
        ),
        (
            "gsc_ctr",
            "ALTER TABLE live_site_pages ADD COLUMN gsc_ctr REAL;",
        ),
        (
            "gsc_position",
            "ALTER TABLE live_site_pages ADD COLUMN gsc_position REAL;",
        ),
        (
            "gsc_synced_at",
            "ALTER TABLE live_site_pages ADD COLUMN gsc_synced_at TEXT;",
        ),
    ] {
        let has_col: bool = conn
            .prepare("SELECT COUNT(*) FROM pragma_table_info('live_site_pages') WHERE name = ?1")?
            .query_row([column.0], |row| row.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !has_col {
            conn.execute_batch(column.1)?;
        }
    }

    if version < 37 {
        // Add not_before support for delayed task execution (e.g. outcome reviews)
        run_tolerated_migration(conn, 37, "ALTER TABLE tasks ADD COLUMN not_before TEXT;");
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (37, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 40 {
        // Add content_hash and last_edited_at to articles for tracking
        run_tolerated_migration(conn, 40, "ALTER TABLE articles ADD COLUMN content_hash TEXT;");
        run_tolerated_migration(
            conn,
            40,
            "ALTER TABLE articles ADD COLUMN last_edited_at TEXT;",
        );
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (40, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    // Repair: ensure content_hash and last_edited_at exist even if V40 was skipped
    {
        let tracking_columns = [
            ("content_hash", "ALTER TABLE articles ADD COLUMN content_hash TEXT;"),
            ("last_edited_at", "ALTER TABLE articles ADD COLUMN last_edited_at TEXT;"),
        ];
        for (name, sql) in tracking_columns {
            let has_col: bool = conn
                .prepare("SELECT COUNT(*) FROM pragma_table_info('articles') WHERE name = ?1")?
                .query_row([name], |r| r.get::<_, i64>(0))
                .unwrap_or(0)
                > 0;
            if !has_col {
                conn.execute_batch(sql)?;
            }
        }
    }

    if version < 38 {
        // GSC indexing recovery history: track attempts and outcomes per URL
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS gsc_recovery_history (
                id              INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id      TEXT NOT NULL,
                url             TEXT NOT NULL,
                article_id      INTEGER,
                campaign_task_id TEXT NOT NULL,
                child_task_id   TEXT NOT NULL,
                reason_code     TEXT NOT NULL,
                incoming_before INTEGER NOT NULL DEFAULT 0,
                incoming_after  INTEGER,
                links_added     INTEGER NOT NULL DEFAULT 0,
                outcome_status  TEXT NOT NULL DEFAULT 'pending',
                created_at      TEXT NOT NULL,
                resolved_at     TEXT,
                FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_recovery_history_project ON gsc_recovery_history(project_id);
            CREATE INDEX IF NOT EXISTS idx_recovery_history_url ON gsc_recovery_history(url);
            CREATE INDEX IF NOT EXISTS idx_recovery_history_campaign ON gsc_recovery_history(campaign_task_id);"
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (38, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 39 {
        // Research shortlist: persistent queue of themes/keywords to research
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS research_shortlist (
                id                  INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id          TEXT NOT NULL,
                theme               TEXT NOT NULL,
                seeds               TEXT NOT NULL DEFAULT '[]',
                source              TEXT NOT NULL DEFAULT 'territory_analysis',
                status              TEXT NOT NULL DEFAULT 'pending',
                priority            TEXT NOT NULL DEFAULT 'medium',
                article_count       INTEGER,
                total_impressions   REAL,
                added_at            TEXT NOT NULL,
                researched_at       TEXT,
                covered_at          TEXT,
                FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_research_shortlist_project ON research_shortlist(project_id);
            CREATE INDEX IF NOT EXISTS idx_research_shortlist_status ON research_shortlist(status);
            CREATE INDEX IF NOT EXISTS idx_research_shortlist_project_status ON research_shortlist(project_id, status);"
        )?;
        // Clean up deprecated territory_research tasks (delete child runs first to satisfy FK).
        // Wrapped in its own execute_batch so a missing task_runs table doesn't block the migration.
        run_tolerated_migration(
            conn,
            39,
            "DELETE FROM task_runs WHERE task_id IN (SELECT id FROM tasks WHERE type = 'territory_research');
             DELETE FROM tasks WHERE type = 'territory_research';",
        );
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (39, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    // Repair: ensure global_settings exists even if V14 was skipped or the table was dropped.
    {
        let table_exists: bool = conn
            .prepare("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='global_settings'")?
            .query_row([], |r| r.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;
        if !table_exists {
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute_batch(
                "CREATE TABLE global_settings (
                    key TEXT PRIMARY KEY,
                    value TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );",
            )?;
            conn.execute(
                "INSERT OR IGNORE INTO global_settings (key, value, updated_at) VALUES ('agent_provider', 'kimi', ?1)",
                [&now],
            )?;
        }
    }

    // Sanitize: clear invalid legacy agent_provider values from projects so they fall back to global.
    // Allowlist is derived from VALID_PROVIDERS (single source of truth).
    {
        let allowlist = global_settings::valid_providers_sql_list();
        let sql = format!(
            "UPDATE projects SET agent_provider = NULL WHERE agent_provider IS NOT NULL AND agent_provider NOT IN ({allowlist})"
        );
        let affected = conn.execute(&sql, [])?;
        if affected > 0 {
            log::info!("[db::run_migrations] Cleared invalid agent_provider from {} project(s)", affected);
        }
    }

    // Repair: fix recovery history entries incorrectly marked 'failed' when links actually exist.
    {
        let affected = conn.execute(
            "UPDATE gsc_recovery_history SET outcome_status = 'linked' WHERE outcome_status = 'failed' AND incoming_after >= 1",
            [],
        )?;
        if affected > 0 {
            log::info!("[db::run_migrations] Fixed {} recovery history entries from 'failed' to 'linked' (incoming_after >= 1)", affected);
        }
    }

    // One-off: clean up research_shortlist entries mangled by the old canonical_keyword bug
    // (words were sorted alphabetically, destroying readability). Runs once per database.
    {
        let already_done: bool = conn
            .prepare("SELECT COUNT(*) FROM global_settings WHERE key = 'shortlist_repair_v1_done'")?
            .query_row([], |r| r.get::<_, i64>(0))
            .unwrap_or(0)
            > 0;

        if !already_done {
            let entries = crate::db::research_shortlist::list_entries(conn, "", None)
                .unwrap_or_default();
            let mut deleted = 0usize;
            for entry in entries {
                let words: Vec<String> = entry
                    .theme
                    .split(|c: char| !c.is_alphanumeric())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_lowercase())
                    .collect();
                if words.len() >= 3 {
                    let mut sorted = words.clone();
                    sorted.sort_unstable();
                    if words == sorted {
                        if let Some(id) = entry.id {
                            let _ = conn.execute(
                                "DELETE FROM research_shortlist WHERE id = ?1",
                                rusqlite::params![id],
                            );
                            deleted += 1;
                        }
                    }
                }
            }
            if deleted > 0 {
                log::info!(
                    "[db::run_migrations] Deleted {} mangled research_shortlist entries (old canonical_keyword bug)",
                    deleted
                );
            }
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                "INSERT OR IGNORE INTO global_settings (key, value, updated_at) VALUES ('shortlist_repair_v1_done', '1', ?1)",
                [&now],
            )?;
        }
    }

    if version < 41 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS content_audit_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT NOT NULL,
                run_at TEXT NOT NULL,
                total_audited INTEGER NOT NULL DEFAULT 0,
                good_count INTEGER NOT NULL DEFAULT 0,
                needs_improvement_count INTEGER NOT NULL DEFAULT 0,
                poor_count INTEGER NOT NULL DEFAULT 0,
                duplicate_groups_json TEXT NOT NULL DEFAULT '[]',
                FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_content_audit_runs_project ON content_audit_runs(project_id);
            CREATE INDEX IF NOT EXISTS idx_content_audit_runs_project_run_at ON content_audit_runs(project_id, run_at);

            CREATE TABLE IF NOT EXISTS article_content_audits (
                run_id INTEGER NOT NULL,
                article_id INTEGER NOT NULL,
                article_file TEXT NOT NULL,
                title TEXT NOT NULL DEFAULT '',
                url_slug TEXT NOT NULL DEFAULT '',
                health TEXT NOT NULL DEFAULT 'unknown',
                health_score INTEGER DEFAULT 0,
                priority_score INTEGER DEFAULT 0,
                data_json TEXT NOT NULL DEFAULT '{}',
                PRIMARY KEY (run_id, article_id),
                FOREIGN KEY (run_id) REFERENCES content_audit_runs(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_article_content_audits_run ON article_content_audits(run_id);
            CREATE INDEX IF NOT EXISTS idx_article_content_audits_health ON article_content_audits(health);
            "
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (41, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 42 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS audit_artifacts (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT NOT NULL,
                artifact_type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                data_json TEXT NOT NULL DEFAULT '{}',
                FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_audit_artifacts_project_type ON audit_artifacts(project_id, artifact_type);
            CREATE INDEX IF NOT EXISTS idx_audit_artifacts_project_type_created ON audit_artifacts(project_id, artifact_type, created_at);
            "
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (42, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 43 {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS clarity_export_rows (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                project_id TEXT NOT NULL,
                exported_at TEXT NOT NULL,
                clarity_date TEXT NOT NULL,
                dimension_set TEXT NOT NULL,
                metric_name TEXT NOT NULL,
                dimension_json TEXT NOT NULL,
                value_json TEXT NOT NULL,
                raw_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
            );
            CREATE INDEX IF NOT EXISTS idx_clarity_rows_project_date ON clarity_export_rows(project_id, clarity_date);
            CREATE INDEX IF NOT EXISTS idx_clarity_rows_metric ON clarity_export_rows(project_id, metric_name, clarity_date);
            "
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (43, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 44 {
        run_tolerated_migration(conn, 44, "ALTER TABLE projects ADD COLUMN clarity_project_id TEXT;");
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (44, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 45 {
        conn.execute_batch(MIGRATION_V45)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (45, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 46 {
        conn.execute_batch(MIGRATION_V46)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (46, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 47 {
        // Add selftext column to reddit_opportunities; tolerate failure for
        // idempotency, but log loudly — silently recording V47 while the column is
        // missing caused the reddit persistence drift in issue #71.
        run_tolerated_migration(conn, 47, MIGRATION_V47);
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (47, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 48 {
        conn.execute_batch(MIGRATION_V48)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (48, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    Ok(())
}

static MIGRATION_V45: &str = r#"
-- Unified SEO discovery opportunity backlog
CREATE TABLE IF NOT EXISTS seo_opportunities (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    article_id INTEGER NOT NULL,
    url_slug TEXT NOT NULL,
    generated_at TEXT NOT NULL,
    opportunity_score INTEGER NOT NULL,
    effort TEXT NOT NULL,
    recommended_action TEXT NOT NULL,
    signals_json TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'open',
    accepted_at TEXT,
    resulting_task_id TEXT,
    UNIQUE(project_id, article_id, generated_at),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_seo_opportunities_project_score ON seo_opportunities(project_id, status, opportunity_score DESC);
CREATE INDEX IF NOT EXISTS idx_seo_opportunities_project_generated ON seo_opportunities(project_id, generated_at);
"#;

static MIGRATION_V46: &str = r#"
-- Topic health tracking on research_shortlist
ALTER TABLE research_shortlist ADD COLUMN signal_score REAL;
ALTER TABLE research_shortlist ADD COLUMN health_status TEXT NOT NULL DEFAULT 'unproven';
ALTER TABLE research_shortlist ADD COLUMN last_reviewed_at TEXT;
CREATE INDEX IF NOT EXISTS idx_research_shortlist_health ON research_shortlist(project_id, health_status);

-- Quality review fields on articles for quick UI surfacing
ALTER TABLE articles ADD COLUMN quality_score INTEGER;
ALTER TABLE articles ADD COLUMN quality_reviewed_at TEXT;
ALTER TABLE articles ADD COLUMN quality_pass INTEGER;
CREATE INDEX IF NOT EXISTS idx_articles_quality ON articles(project_id, quality_pass);

-- Structured quality reviews for individual articles
CREATE TABLE IF NOT EXISTS article_quality_reviews (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    article_file TEXT NOT NULL,
    overall_pass INTEGER NOT NULL DEFAULT 0,
    scores_json TEXT NOT NULL DEFAULT '{}',
    checks_json TEXT NOT NULL DEFAULT '[]',
    reviewed_at TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_article_quality_reviews_project ON article_quality_reviews(project_id);
CREATE INDEX IF NOT EXISTS idx_article_quality_reviews_task ON article_quality_reviews(task_id);
CREATE INDEX IF NOT EXISTS idx_article_quality_reviews_file ON article_quality_reviews(project_id, article_file);
"#;

static MIGRATION_V47: &str = r#"
-- Persist Reddit post body (selftext) so enrichment/drafting see more than the title
ALTER TABLE reddit_opportunities ADD COLUMN selftext TEXT;
"#;

static MIGRATION_V48: &str = r#"
-- Append-only per-page daily GSC snapshots (issue #23).
-- NEVER delete from this table: it is the time series behind before/after
-- outcome measurement. INSERT OR IGNORE on (project_id, page, date) keeps
-- re-syncs idempotent without destroying history.
CREATE TABLE IF NOT EXISTS gsc_page_daily (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      TEXT NOT NULL,
    page            TEXT NOT NULL,
    date            TEXT NOT NULL,
    clicks          REAL NOT NULL DEFAULT 0,
    impressions     REAL NOT NULL DEFAULT 0,
    ctr             REAL NOT NULL DEFAULT 0,
    position        REAL NOT NULL DEFAULT 0,
    fetched_at      TEXT NOT NULL,
    UNIQUE(project_id, page, date),
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_gsc_page_daily_project_page_date
    ON gsc_page_daily(project_id, page, date);

-- Classification results from content_outcome_review tasks (issue #23).
-- Append-only outcome history, queryable by research/keeper-selection prompts.
CREATE TABLE IF NOT EXISTS content_outcome_results (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id        TEXT NOT NULL,
    slug              TEXT NOT NULL,
    parent_task_type  TEXT NOT NULL,
    parent_task_id    TEXT NOT NULL,
    classification    TEXT NOT NULL,
    baseline_json     TEXT NOT NULL DEFAULT '{}',
    recent_json       TEXT NOT NULL DEFAULT '{}',
    reviewed_at       TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_content_outcome_results_project
    ON content_outcome_results(project_id, slug);
CREATE INDEX IF NOT EXISTS idx_content_outcome_results_parent
    ON content_outcome_results(parent_task_type, classification);
"#;

// ═══════════════════════════════════════════════════════════════════════════════
// Article Audit State CRUD
// ═══════════════════════════════════════════════════════════════════════════════

/// Persistent state for a single article under a specific audit type.
#[derive(Debug, Clone)]
pub struct ArticleAuditState {
    pub project_id: String,
    pub article_file: String,
    pub audit_type: String,
    pub last_audited_at: String,
    pub was_healthy: bool,
    pub content_hash: String,
    pub issues_found: Vec<String>,
}

/// Retrieve the stored audit state for an article.
pub fn get_article_audit_state(
    conn: &Connection,
    project_id: &str,
    article_file: &str,
    audit_type: &str,
) -> Result<Option<ArticleAuditState>> {
    let mut stmt = conn.prepare(
        "SELECT last_audited_at, was_healthy, content_hash, issues_found
         FROM article_audit_state
         WHERE project_id = ?1 AND article_file = ?2 AND audit_type = ?3",
    )?;

    let row = stmt.query_row(
        rusqlite::params![project_id, article_file, audit_type],
        |row| {
            let issues_json: String = row.get(3)?;
            let issues: Vec<String> = serde_json::from_str(&issues_json).unwrap_or_default();
            Ok(ArticleAuditState {
                project_id: project_id.to_string(),
                article_file: article_file.to_string(),
                audit_type: audit_type.to_string(),
                last_audited_at: row.get(0)?,
                was_healthy: row.get::<_, i64>(1)? != 0,
                content_hash: row.get(2)?,
                issues_found: issues,
            })
        },
    );

    match row {
        Ok(state) => Ok(Some(state)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Store (or update) the audit state for an article.
pub fn set_article_audit_state(
    conn: &Connection,
    project_id: &str,
    article_file: &str,
    audit_type: &str,
    was_healthy: bool,
    content_hash: &str,
    issues_found: &[String],
) -> Result<()> {
    let issues_json = serde_json::to_string(issues_found).unwrap_or_else(|_| "[]".to_string());
    let now = chrono::Utc::now().to_rfc3339();
    let healthy_i64 = if was_healthy { 1 } else { 0 };

    conn.execute(
        "INSERT INTO article_audit_state
         (project_id, article_file, audit_type, last_audited_at, was_healthy, content_hash, issues_found)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(project_id, article_file, audit_type)
         DO UPDATE SET
             last_audited_at = excluded.last_audited_at,
             was_healthy = excluded.was_healthy,
             content_hash = excluded.content_hash,
             issues_found = excluded.issues_found",
        rusqlite::params![
            project_id,
            article_file,
            audit_type,
            now,
            healthy_i64,
            content_hash,
            issues_json,
        ],
    )?;

    Ok(())
}

/// Delete all audit state for a project (e.g. when project is deleted).
pub fn delete_project_audit_state(conn: &Connection, project_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM article_audit_state WHERE project_id = ?1",
        [project_id],
    )?;
    Ok(())
}

/// Get all audit states for a project + audit type.
pub fn list_article_audit_states(
    conn: &Connection,
    project_id: &str,
    audit_type: &str,
) -> Result<Vec<ArticleAuditState>> {
    let mut stmt = conn.prepare(
        "SELECT article_file, last_audited_at, was_healthy, content_hash, issues_found
         FROM article_audit_state
         WHERE project_id = ?1 AND audit_type = ?2",
    )?;

    let rows = stmt.query_map(rusqlite::params![project_id, audit_type], |row| {
        let issues_json: String = row.get(4)?;
        let issues: Vec<String> = serde_json::from_str(&issues_json).unwrap_or_default();
        Ok(ArticleAuditState {
            project_id: project_id.to_string(),
            article_file: row.get(0)?,
            audit_type: audit_type.to_string(),
            last_audited_at: row.get(1)?,
            was_healthy: row.get::<_, i64>(2)? != 0,
            content_hash: row.get(3)?,
            issues_found: issues,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

// ═══════════════════════════════════════════════════════════════════════════════
// CTR Query Metrics CRUD
// ═══════════════════════════════════════════════════════════════════════════════

/// Single query-level GSC metric row for a page.
#[derive(Debug, Clone)]
pub struct CtrQueryMetricRow {
    pub project_id: String,
    pub article_id: i64,
    pub page_url: String,
    pub query: String,
    pub impressions: f64,
    pub clicks: f64,
    pub ctr: f64,
    pub avg_position: f64,
    pub period_start: Option<String>,
    pub period_end: Option<String>,
    pub intent: Option<String>,
    pub fetched_at: String,
}

/// Store (or replace) query metrics for an article.
pub fn set_ctr_query_metrics(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    page_url: &str,
    metrics: &[(String, f64, f64, f64, f64, Option<String>)],
    period_start: Option<&str>,
    period_end: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;

    // Clear old metrics for this article
    tx.execute(
        "DELETE FROM ctr_query_metrics WHERE project_id = ?1 AND article_id = ?2",
        rusqlite::params![project_id, article_id],
    )?;

    for (query, impressions, clicks, ctr, avg_position, intent) in metrics {
        tx.execute(
            "INSERT INTO ctr_query_metrics
             (project_id, article_id, page_url, query, impressions, clicks, ctr, avg_position, period_start, period_end, intent, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(project_id, article_id, query)
             DO UPDATE SET
                 page_url = excluded.page_url,
                 impressions = excluded.impressions,
                 clicks = excluded.clicks,
                 ctr = excluded.ctr,
                 avg_position = excluded.avg_position,
                 period_start = excluded.period_start,
                 period_end = excluded.period_end,
                 intent = excluded.intent,
                 fetched_at = excluded.fetched_at",
            rusqlite::params![
                project_id,
                article_id,
                page_url,
                query,
                impressions,
                clicks,
                ctr,
                avg_position,
                period_start,
                period_end,
                intent.as_deref(),
                &now,
            ],
        )?;
    }

    tx.commit()?;
    Ok(())
}

/// Load stored query metrics for an article.
pub fn get_ctr_query_metrics(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
) -> Result<Vec<CtrQueryMetricRow>> {
    let mut stmt = conn.prepare(
        "SELECT page_url, query, impressions, clicks, ctr, avg_position, period_start, period_end, intent, fetched_at
         FROM ctr_query_metrics
         WHERE project_id = ?1 AND article_id = ?2
         ORDER BY impressions DESC",
    )?;

    let rows = stmt.query_map(rusqlite::params![project_id, article_id], |row| {
        Ok(CtrQueryMetricRow {
            project_id: project_id.to_string(),
            article_id,
            page_url: row.get(0)?,
            query: row.get(1)?,
            impressions: row.get(2)?,
            clicks: row.get(3)?,
            ctr: row.get(4)?,
            avg_position: row.get(5)?,
            period_start: row.get(6)?,
            period_end: row.get(7)?,
            intent: row.get(8)?,
            fetched_at: row.get(9)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Newest `fetched_at` across all of a project's query metrics, or `None` when
/// the table has no rows for the project. Used for staleness warnings (issue #25).
pub fn ctr_query_metrics_max_fetched_at(
    conn: &Connection,
    project_id: &str,
) -> Result<Option<String>> {
    conn.query_row(
        "SELECT MAX(fetched_at) FROM ctr_query_metrics WHERE project_id = ?1",
        rusqlite::params![project_id],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

// ═══════════════════════════════════════════════════════════════════════════════
// GSC Page Daily Snapshots (append-only, issue #23)
// ═══════════════════════════════════════════════════════════════════════════════

/// Append per-page daily GSC rows to the snapshot table.
///
/// Append-only by contract: INSERT OR IGNORE on (project_id, page, date).
/// There is deliberately no delete/update path — re-syncs must never destroy
/// history. Returns the number of newly inserted rows.
pub fn insert_gsc_page_daily_snapshots(
    conn: &Connection,
    project_id: &str,
    rows: &[crate::models::gsc::PageDailyMetrics],
) -> Result<usize> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut inserted = 0usize;
    for row in rows {
        inserted += conn.execute(
            "INSERT OR IGNORE INTO gsc_page_daily
             (project_id, page, date, clicks, impressions, ctr, position, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                project_id,
                row.page,
                row.date,
                row.clicks,
                row.impressions,
                row.ctr,
                row.position,
                now,
            ],
        )?;
    }
    Ok(inserted)
}

/// Aggregated snapshot metrics for one page over a date window (inclusive).
///
/// `position` is the impressions-weighted average across days with data.
/// Returns `None` when the page has no snapshot rows in the window.
pub fn gsc_page_daily_window_metrics(
    conn: &Connection,
    project_id: &str,
    page: &str,
    start_date: &str,
    end_date: &str,
) -> Result<Option<GscDailyWindowMetrics>> {
    let row = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(clicks), 0), COALESCE(SUM(impressions), 0),
                COALESCE(SUM(position * impressions) / NULLIF(SUM(impressions), 0), 0)
         FROM gsc_page_daily
         WHERE project_id = ?1 AND page = ?2 AND date >= ?3 AND date <= ?4",
        rusqlite::params![project_id, page, start_date, end_date],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, f64>(3)?,
            ))
        },
    )?;

    let (days, clicks, impressions, position) = row;
    if days == 0 {
        return Ok(None);
    }

    Ok(Some(GscDailyWindowMetrics {
        days_with_data: days,
        clicks,
        impressions,
        position,
    }))
}

/// Aggregated snapshot metrics for **all** pages over a date window (inclusive).
///
/// Same semantics as [`gsc_page_daily_window_metrics`]: position is the
/// impressions-weighted average. Pages with no rows in the window are omitted.
/// Prefer this for overview/catalog joins so callers avoid O(pages) SQL round-trips.
pub fn gsc_page_daily_window_metrics_bulk(
    conn: &Connection,
    project_id: &str,
    start_date: &str,
    end_date: &str,
) -> Result<HashMap<String, GscDailyWindowMetrics>> {
    let mut stmt = conn.prepare(
        "SELECT page,
                COUNT(*) AS days_with_data,
                COALESCE(SUM(clicks), 0),
                COALESCE(SUM(impressions), 0),
                COALESCE(SUM(position * impressions) / NULLIF(SUM(impressions), 0), 0)
         FROM gsc_page_daily
         WHERE project_id = ?1 AND date >= ?2 AND date <= ?3
         GROUP BY page",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![project_id, start_date, end_date],
        |r| {
            Ok((
                r.get::<_, String>(0)?,
                GscDailyWindowMetrics {
                    days_with_data: r.get(1)?,
                    clicks: r.get(2)?,
                    impressions: r.get(3)?,
                    position: r.get(4)?,
                },
            ))
        },
    )?;
    let mut map = HashMap::new();
    for row in rows {
        let (page, metrics) = row?;
        map.insert(page, metrics);
    }
    Ok(map)
}

/// Distinct pages with snapshot rows for a project (used for slug matching).
pub fn list_gsc_page_daily_pages(conn: &Connection, project_id: &str) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT DISTINCT page FROM gsc_page_daily WHERE project_id = ?1")?;
    let rows = stmt.query_map(rusqlite::params![project_id], |r| r.get::<_, String>(0))?;
    let mut pages = Vec::new();
    for row in rows {
        pages.push(row?);
    }
    Ok(pages)
}

/// Aggregated metrics for one page over a snapshot window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GscDailyWindowMetrics {
    pub days_with_data: i64,
    pub clicks: f64,
    pub impressions: f64,
    pub position: f64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// Content Outcome Results (issue #23)
// ═══════════════════════════════════════════════════════════════════════════════

/// One classified outcome review for an article. Append-only history.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContentOutcomeResult {
    pub project_id: String,
    pub slug: String,
    pub parent_task_type: String,
    pub parent_task_id: String,
    pub classification: String,
    pub baseline_json: String,
    pub recent_json: String,
    pub reviewed_at: String,
}

/// Persist a content outcome classification. Append-only: each review inserts
/// a new row so the history of repeated reviews is preserved.
pub fn insert_content_outcome_result(conn: &Connection, result: &ContentOutcomeResult) -> Result<()> {
    conn.execute(
        "INSERT INTO content_outcome_results
         (project_id, slug, parent_task_type, parent_task_id, classification, baseline_json, recent_json, reviewed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            result.project_id,
            result.slug,
            result.parent_task_type,
            result.parent_task_id,
            result.classification,
            result.baseline_json,
            result.recent_json,
            result.reviewed_at,
        ],
    )?;
    Ok(())
}

/// List outcome results for a project, newest first. Queryable history for
/// research/keeper-selection prompts (issue #23).
pub fn list_content_outcome_results(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<ContentOutcomeResult>> {
    let mut stmt = conn.prepare(
        "SELECT project_id, slug, parent_task_type, parent_task_id, classification, baseline_json, recent_json, reviewed_at
         FROM content_outcome_results
         WHERE project_id = ?1
         ORDER BY reviewed_at DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![project_id], |row| {
        Ok(ContentOutcomeResult {
            project_id: row.get(0)?,
            slug: row.get(1)?,
            parent_task_type: row.get(2)?,
            parent_task_id: row.get(3)?,
            classification: row.get(4)?,
            baseline_json: row.get(5)?,
            recent_json: row.get(6)?,
            reviewed_at: row.get(7)?,
        })
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

// ═══════════════════════════════════════════════════════════════════════════════
// CTR Rendered Page Audit CRUD
// ═══════════════════════════════════════════════════════════════════════════════

/// Store (or replace) a rendered SERP audit record for an article.
pub fn set_ctr_rendered_audit(
    conn: &Connection,
    project_id: &str,
    audit: &crate::models::ctr::CtrRenderedPageAudit,
) -> Result<()> {
    let schema_json =
        serde_json::to_string(&audit.schema_types).unwrap_or_else(|_| "[]".to_string());
    let snippet_json =
        serde_json::to_string(&audit.snippet_markup).unwrap_or_else(|_| "{}".to_string());
    let issues_json = serde_json::to_string(&audit.issues).unwrap_or_else(|_| "[]".to_string());
    let has_faq = if audit.has_rendered_faq_page { 1 } else { 0 };

    conn.execute(
        "INSERT INTO ctr_rendered_page_audits
         (project_id, article_id, url, file, source_title, rendered_title, rendered_title_length,
          title_issue_source, source_description, rendered_description, canonical_url, rendered_h1,
          schema_types_json, has_rendered_faq_page, rendered_faq_question_count, snippet_markup_json, issues_json, checked_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
         ON CONFLICT(project_id, article_id)
         DO UPDATE SET
             url = excluded.url,
             file = excluded.file,
             source_title = excluded.source_title,
             rendered_title = excluded.rendered_title,
             rendered_title_length = excluded.rendered_title_length,
             title_issue_source = excluded.title_issue_source,
             source_description = excluded.source_description,
             rendered_description = excluded.rendered_description,
             canonical_url = excluded.canonical_url,
             rendered_h1 = excluded.rendered_h1,
             schema_types_json = excluded.schema_types_json,
             has_rendered_faq_page = excluded.has_rendered_faq_page,
             rendered_faq_question_count = excluded.rendered_faq_question_count,
             snippet_markup_json = excluded.snippet_markup_json,
             issues_json = excluded.issues_json,
             checked_at = excluded.checked_at",
        rusqlite::params![
            project_id,
            audit.article_id,
            &audit.url,
            &audit.file,
            &audit.source_title,
            &audit.rendered_title,
            audit.rendered_title_length as i64,
            &audit.title_issue_source,
            &audit.source_description,
            audit.rendered_description.as_deref(),
            audit.canonical_url.as_deref(),
            audit.rendered_h1.as_deref(),
            schema_json,
            has_faq,
            audit.rendered_faq_question_count as i64,
            snippet_json,
            issues_json,
            &audit.checked_at,
        ],
    )?;
    Ok(())
}

/// Load the latest rendered audit for an article.
pub fn get_ctr_rendered_audit(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
) -> Result<Option<crate::models::ctr::CtrRenderedPageAudit>> {
    let mut stmt = conn.prepare(
        "SELECT url, file, source_title, rendered_title, rendered_title_length,
                title_issue_source, source_description, rendered_description,
                canonical_url, rendered_h1, schema_types_json, has_rendered_faq_page,
                rendered_faq_question_count, snippet_markup_json, issues_json, checked_at
         FROM ctr_rendered_page_audits
         WHERE project_id = ?1 AND article_id = ?2",
    )?;

    let row = stmt.query_row(rusqlite::params![project_id, article_id], |row| {
        let schema_json: String = row.get(10)?;
        let has_faq: i64 = row.get(11)?;
        let faq_count: i64 = row.get(12)?;
        let snippet_json: String = row.get(13)?;
        let issues_json: String = row.get(14)?;

        Ok(crate::models::ctr::CtrRenderedPageAudit {
            article_id,
            url: row.get(0)?,
            file: row.get(1)?,
            source_title: row.get(2)?,
            rendered_title: row.get(3)?,
            rendered_title_length: row.get::<_, i64>(4)? as usize,
            title_issue_source: row.get(5)?,
            source_description: row.get(6)?,
            rendered_description: row.get(7)?,
            canonical_url: row.get(8)?,
            rendered_h1: row.get(9)?,
            schema_types: serde_json::from_str(&schema_json).unwrap_or_default(),
            has_rendered_faq_page: has_faq != 0,
            rendered_faq_question_count: faq_count as usize,
            snippet_markup: serde_json::from_str(&snippet_json).unwrap_or_default(),
            issues: serde_json::from_str(&issues_json).unwrap_or_default(),
            checked_at: row.get(15)?,
        })
    });

    match row {
        Ok(audit) => Ok(Some(audit)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Load all rendered audits for a project.
pub fn list_ctr_rendered_audits(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<crate::models::ctr::CtrRenderedPageAudit>> {
    let mut stmt = conn.prepare(
        "SELECT article_id, url, file, source_title, rendered_title, rendered_title_length,
                title_issue_source, source_description, rendered_description,
                canonical_url, rendered_h1, schema_types_json, has_rendered_faq_page,
                rendered_faq_question_count, snippet_markup_json, issues_json, checked_at
         FROM ctr_rendered_page_audits
         WHERE project_id = ?1
         ORDER BY article_id",
    )?;

    let rows = stmt.query_map([project_id], |row| {
        let schema_json: String = row.get(11)?;
        let has_faq: i64 = row.get(12)?;
        let faq_count: i64 = row.get(13)?;
        let snippet_json: String = row.get(14)?;
        let issues_json: String = row.get(15)?;

        Ok(crate::models::ctr::CtrRenderedPageAudit {
            article_id: row.get(0)?,
            url: row.get(1)?,
            file: row.get(2)?,
            source_title: row.get(3)?,
            rendered_title: row.get(4)?,
            rendered_title_length: row.get::<_, i64>(5)? as usize,
            title_issue_source: row.get(6)?,
            source_description: row.get(7)?,
            rendered_description: row.get(8)?,
            canonical_url: row.get(9)?,
            rendered_h1: row.get(10)?,
            schema_types: serde_json::from_str(&schema_json).unwrap_or_default(),
            has_rendered_faq_page: has_faq != 0,
            rendered_faq_question_count: faq_count as usize,
            snippet_markup: serde_json::from_str(&snippet_json).unwrap_or_default(),
            issues: serde_json::from_str(&issues_json).unwrap_or_default(),
            checked_at: row.get(16)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

// ═══════════════════════════════════════════════════════════════════════════════
// CTR Outcome Tracking CRUD
// ═══════════════════════════════════════════════════════════════════════════════

/// Store a CTR outcome baseline record.
pub fn set_ctr_outcome(conn: &Connection, outcome: &crate::models::ctr::CtrOutcome) -> Result<()> {
    conn.execute(
        "INSERT INTO ctr_outcomes
         (project_id, article_id, fix_task_id, baseline_start, baseline_end,
          after_start, after_end, baseline_clicks, baseline_impressions, baseline_ctr,
          baseline_position, after_clicks, after_impressions, after_ctr, after_position,
          position_delta, outcome_status, deployed_at, reviewed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)
         ON CONFLICT(project_id, article_id, fix_task_id)
         DO UPDATE SET
             baseline_start = excluded.baseline_start,
             baseline_end = excluded.baseline_end,
             after_start = excluded.after_start,
             after_end = excluded.after_end,
             baseline_clicks = excluded.baseline_clicks,
             baseline_impressions = excluded.baseline_impressions,
             baseline_ctr = excluded.baseline_ctr,
             baseline_position = excluded.baseline_position,
             after_clicks = excluded.after_clicks,
             after_impressions = excluded.after_impressions,
             after_ctr = excluded.after_ctr,
             after_position = excluded.after_position,
             position_delta = excluded.position_delta,
             outcome_status = excluded.outcome_status,
             deployed_at = excluded.deployed_at,
             reviewed_at = excluded.reviewed_at",
        rusqlite::params![
            &outcome.project_id,
            outcome.article_id,
            &outcome.fix_task_id,
            &outcome.baseline_start,
            &outcome.baseline_end,
            outcome.after_start.as_deref(),
            outcome.after_end.as_deref(),
            outcome.baseline_clicks,
            outcome.baseline_impressions,
            outcome.baseline_ctr,
            outcome.baseline_position,
            outcome.after_clicks,
            outcome.after_impressions,
            outcome.after_ctr,
            outcome.after_position,
            outcome.position_delta,
            &outcome.outcome_status,
            outcome.deployed_at.as_deref(),
            outcome.reviewed_at.as_deref(),
        ],
    )?;
    Ok(())
}

/// Load a CTR outcome by project, article, and fix task.
pub fn get_ctr_outcome(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    fix_task_id: &str,
) -> Result<Option<crate::models::ctr::CtrOutcome>> {
    let mut stmt = conn.prepare(
        "SELECT baseline_start, baseline_end, after_start, after_end,
                baseline_clicks, baseline_impressions, baseline_ctr, baseline_position,
                after_clicks, after_impressions, after_ctr, after_position,
                position_delta, outcome_status, deployed_at, reviewed_at
         FROM ctr_outcomes
         WHERE project_id = ?1 AND article_id = ?2 AND fix_task_id = ?3",
    )?;

    let row = stmt.query_row(
        rusqlite::params![project_id, article_id, fix_task_id],
        |row| {
            Ok(crate::models::ctr::CtrOutcome {
                project_id: project_id.to_string(),
                article_id,
                fix_task_id: fix_task_id.to_string(),
                baseline_start: row.get(0)?,
                baseline_end: row.get(1)?,
                after_start: row.get(2)?,
                after_end: row.get(3)?,
                baseline_clicks: row.get(4)?,
                baseline_impressions: row.get(5)?,
                baseline_ctr: row.get(6)?,
                baseline_position: row.get(7)?,
                after_clicks: row.get(8)?,
                after_impressions: row.get(9)?,
                after_ctr: row.get(10)?,
                after_position: row.get(11)?,
                position_delta: row.get(12)?,
                outcome_status: row.get(13)?,
                deployed_at: row.get(14)?,
                reviewed_at: row.get(15)?,
            })
        },
    );

    match row {
        Ok(o) => Ok(Some(o)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// List all CTR outcomes for a project.
pub fn list_ctr_outcomes(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<crate::models::ctr::CtrOutcome>> {
    let mut stmt = conn.prepare(
        "SELECT article_id, fix_task_id, baseline_start, baseline_end, after_start, after_end,
                baseline_clicks, baseline_impressions, baseline_ctr, baseline_position,
                after_clicks, after_impressions, after_ctr, after_position,
                position_delta, outcome_status, deployed_at, reviewed_at
         FROM ctr_outcomes
         WHERE project_id = ?1
         ORDER BY article_id",
    )?;

    let rows = stmt.query_map([project_id], |row| {
        Ok(crate::models::ctr::CtrOutcome {
            project_id: project_id.to_string(),
            article_id: row.get(0)?,
            fix_task_id: row.get(1)?,
            baseline_start: row.get(2)?,
            baseline_end: row.get(3)?,
            after_start: row.get(4)?,
            after_end: row.get(5)?,
            baseline_clicks: row.get(6)?,
            baseline_impressions: row.get(7)?,
            baseline_ctr: row.get(8)?,
            baseline_position: row.get(9)?,
            after_clicks: row.get(10)?,
            after_impressions: row.get(11)?,
            after_ctr: row.get(12)?,
            after_position: row.get(13)?,
            position_delta: row.get(14)?,
            outcome_status: row.get(15)?,
            deployed_at: row.get(16)?,
            reviewed_at: row.get(17)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Strategy Review CRUD
// ═══════════════════════════════════════════════════════════════════════════════

use crate::models::cannibalization::{ApprovalStatus, StrategyReview};

/// Upsert a strategy review decision.
pub fn set_strategy_review(
    conn: &Connection,
    strategy_id: &str,
    project_id: &str,
    recommendation_type: &str,
    recommendation_id: &str,
    status: ApprovalStatus,
    approved_by: Option<&str>,
    notes: Option<&str>,
) -> Result<StrategyReview> {
    let now = chrono::Utc::now().to_rfc3339();
    let approved_at = if status == ApprovalStatus::Approved {
        Some(now.clone())
    } else {
        None
    };

    conn.execute(
        "INSERT INTO strategy_reviews
         (strategy_id, project_id, recommendation_type, recommendation_id, approval_status, approved_by, approved_at, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)
         ON CONFLICT(strategy_id, recommendation_type, recommendation_id)
         DO UPDATE SET
             approval_status = excluded.approval_status,
             approved_by = excluded.approved_by,
             approved_at = excluded.approved_at,
             notes = excluded.notes,
             updated_at = excluded.updated_at",
        rusqlite::params![
            strategy_id,
            project_id,
            recommendation_type,
            recommendation_id,
            status,
            approved_by,
            approved_at,
            notes,
            now,
        ],
    )?;

    get_strategy_review(conn, strategy_id, recommendation_type, recommendation_id).and_then(|opt| {
        opt.ok_or_else(|| crate::error::Error::Database(rusqlite::Error::QueryReturnedNoRows))
    })
}

/// Get a single strategy review by composite key.
pub fn get_strategy_review(
    conn: &Connection,
    strategy_id: &str,
    recommendation_type: &str,
    recommendation_id: &str,
) -> Result<Option<StrategyReview>> {
    let mut stmt = conn.prepare(
        "SELECT id, strategy_id, project_id, recommendation_type, recommendation_id,
                approval_status, approved_by, approved_at, notes, created_at, updated_at
         FROM strategy_reviews
         WHERE strategy_id = ?1 AND recommendation_type = ?2 AND recommendation_id = ?3",
    )?;

    let row = stmt.query_row(
        rusqlite::params![strategy_id, recommendation_type, recommendation_id],
        |row| {
            Ok(StrategyReview {
                id: row.get(0)?,
                strategy_id: row.get(1)?,
                project_id: row.get(2)?,
                recommendation_type: row.get(3)?,
                recommendation_id: row.get(4)?,
                approval_status: row.get(5)?,
                approved_by: row.get(6)?,
                approved_at: row.get(7)?,
                notes: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        },
    );

    match row {
        Ok(review) => Ok(Some(review)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// List all reviews for a given strategy.
pub fn list_strategy_reviews(conn: &Connection, strategy_id: &str) -> Result<Vec<StrategyReview>> {
    let mut stmt = conn.prepare(
        "SELECT id, strategy_id, project_id, recommendation_type, recommendation_id,
                approval_status, approved_by, approved_at, notes, created_at, updated_at
         FROM strategy_reviews
         WHERE strategy_id = ?1
         ORDER BY updated_at DESC",
    )?;

    let rows = stmt.query_map([strategy_id], |row| {
        Ok(StrategyReview {
            id: row.get(0)?,
            strategy_id: row.get(1)?,
            project_id: row.get(2)?,
            recommendation_type: row.get(3)?,
            recommendation_id: row.get(4)?,
            approval_status: row.get(5)?,
            approved_by: row.get(6)?,
            approved_at: row.get(7)?,
            notes: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    })?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Delete all reviews for a strategy (e.g. when strategy is regenerated).
pub fn delete_strategy_reviews(conn: &Connection, strategy_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM strategy_reviews WHERE strategy_id = ?1",
        [strategy_id],
    )?;
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Article Metadata CRUD
// ═══════════════════════════════════════════════════════════════════════════════

/// Upsert sidecar metadata for an article namespace.
pub fn set_article_metadata(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    namespace: &str,
    payload: &str,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO article_metadata (project_id, article_id, namespace, payload, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(project_id, article_id, namespace) DO UPDATE SET
            payload = excluded.payload,
            updated_at = excluded.updated_at",
        rusqlite::params![project_id, article_id, namespace, payload, now],
    )?;
    Ok(())
}

/// Get sidecar metadata for a specific article namespace.
pub fn get_article_metadata(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    namespace: &str,
) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT payload FROM article_metadata
         WHERE project_id = ?1 AND article_id = ?2 AND namespace = ?3",
    )?;
    let result = stmt.query_row(
        rusqlite::params![project_id, article_id, namespace],
        |row| row.get::<_, String>(0),
    );
    match result {
        Ok(payload) => Ok(Some(payload)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// List all metadata namespaces for a given article.
pub fn list_article_metadata(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT namespace, payload FROM article_metadata
         WHERE project_id = ?1 AND article_id = ?2
         ORDER BY namespace",
    )?;
    let rows = stmt.query_map(rusqlite::params![project_id, article_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// List all metadata for a project (useful for bulk export).
pub fn list_project_metadata(
    conn: &Connection,
    project_id: &str,
) -> Result<Vec<(i64, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT article_id, namespace, payload FROM article_metadata
         WHERE project_id = ?1
         ORDER BY article_id, namespace",
    )?;
    let rows = stmt.query_map([project_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// Delete metadata for a specific article namespace.
pub fn delete_article_metadata(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    namespace: &str,
) -> Result<()> {
    conn.execute(
        "DELETE FROM article_metadata
         WHERE project_id = ?1 AND article_id = ?2 AND namespace = ?3",
        rusqlite::params![project_id, article_id, namespace],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path) VALUES ('proj1', 'Test', '/tmp/test')",
            [],
        )
        .unwrap();
        conn
    }

    fn daily_row(page: &str, date: &str, clicks: f64, impressions: f64) -> crate::models::gsc::PageDailyMetrics {
        crate::models::gsc::PageDailyMetrics {
            page: page.to_string(),
            date: date.to_string(),
            clicks,
            impressions,
            ctr: 0.0,
            position: 5.0,
        }
    }

    #[test]
    fn gsc_page_daily_insert_is_append_only_and_idempotent() {
        let conn = in_memory_db();
        let rows = vec![
            daily_row("https://example.com/blog/foo", "2026-07-01", 1.0, 10.0),
            daily_row("https://example.com/blog/foo", "2026-07-02", 2.0, 20.0),
        ];

        let inserted = insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();
        assert_eq!(inserted, 2);

        // Re-inserting the same rows (a re-sync of an overlapping window) must
        // not duplicate or replace anything — INSERT OR IGNORE.
        let reinserted = insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();
        assert_eq!(reinserted, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM gsc_page_daily", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);

        // Original values preserved (first write wins, never updated).
        let clicks: f64 = conn
            .query_row(
                "SELECT clicks FROM gsc_page_daily WHERE page = 'https://example.com/blog/foo' AND date = '2026-07-01'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(clicks, 1.0);
    }

    #[test]
    fn gsc_page_daily_window_metrics_aggregates_and_weights_position() {
        let conn = in_memory_db();
        let rows = vec![
            daily_row("https://example.com/blog/foo", "2026-07-01", 1.0, 10.0),
            daily_row("https://example.com/blog/foo", "2026-07-02", 3.0, 30.0),
            // Outside the window — must be excluded.
            daily_row("https://example.com/blog/foo", "2026-06-01", 100.0, 1000.0),
        ];
        insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();

        let m = gsc_page_daily_window_metrics(
            &conn,
            "proj1",
            "https://example.com/blog/foo",
            "2026-07-01",
            "2026-07-31",
        )
        .unwrap()
        .expect("window has data");

        assert_eq!(m.days_with_data, 2);
        assert_eq!(m.clicks, 4.0);
        assert_eq!(m.impressions, 40.0);
        // Both days have position 5.0, so the weighted average is 5.0.
        assert_eq!(m.position, 5.0);

        // Unknown page → None (no data in window).
        assert!(gsc_page_daily_window_metrics(
            &conn,
            "proj1",
            "https://example.com/blog/unknown",
            "2026-07-01",
            "2026-07-31",
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn gsc_page_daily_window_metrics_bulk_matches_single_page_semantics() {
        let conn = in_memory_db();
        let rows = vec![
            daily_row("https://example.com/blog/foo", "2026-07-01", 1.0, 10.0),
            daily_row("https://example.com/blog/foo", "2026-07-02", 3.0, 30.0),
            daily_row("https://example.com/blog/bar", "2026-07-01", 2.0, 20.0),
            // Outside the window — must be excluded.
            daily_row("https://example.com/blog/foo", "2026-06-01", 100.0, 1000.0),
            daily_row("https://example.com/blog/baz", "2026-06-15", 9.0, 90.0),
        ];
        insert_gsc_page_daily_snapshots(&conn, "proj1", &rows).unwrap();

        let bulk = gsc_page_daily_window_metrics_bulk(
            &conn,
            "proj1",
            "2026-07-01",
            "2026-07-31",
        )
        .unwrap();

        assert_eq!(bulk.len(), 2);

        let foo = bulk
            .get("https://example.com/blog/foo")
            .expect("foo in bulk");
        let foo_single = gsc_page_daily_window_metrics(
            &conn,
            "proj1",
            "https://example.com/blog/foo",
            "2026-07-01",
            "2026-07-31",
        )
        .unwrap()
        .expect("foo single");
        assert_eq!(*foo, foo_single);
        assert_eq!(foo.days_with_data, 2);
        assert_eq!(foo.clicks, 4.0);
        assert_eq!(foo.impressions, 40.0);
        assert_eq!(foo.position, 5.0);

        let bar = bulk
            .get("https://example.com/blog/bar")
            .expect("bar in bulk");
        assert_eq!(bar.days_with_data, 1);
        assert_eq!(bar.clicks, 2.0);
        assert_eq!(bar.impressions, 20.0);

        assert!(!bulk.contains_key("https://example.com/blog/baz"));
        assert!(!bulk.contains_key("https://example.com/blog/unknown"));
    }

    #[test]
    fn content_outcome_results_are_append_only_history() {
        let conn = in_memory_db();
        for (i, classification) in ["neutral", "improved"].iter().enumerate() {
            insert_content_outcome_result(
                &conn,
                &ContentOutcomeResult {
                    project_id: "proj1".to_string(),
                    slug: "foo".to_string(),
                    parent_task_type: "fix_content_article".to_string(),
                    parent_task_id: format!("task-{}", i),
                    classification: classification.to_string(),
                    baseline_json: "{}".to_string(),
                    recent_json: "{}".to_string(),
                    reviewed_at: format!("2026-07-0{}T00:00:00Z", i + 1),
                },
            )
            .unwrap();
        }

        let results = list_content_outcome_results(&conn, "proj1").unwrap();
        assert_eq!(results.len(), 2);
        // Newest first.
        assert_eq!(results[0].classification, "improved");
        assert_eq!(results[1].classification, "neutral");
    }

    #[test]
    fn migration_v38_resets_overlong_target_keywords() {
        // Exercise the migration SQL directly: schema_version is already at the
        // latest version after init, so the `version < 38` gate would skip it.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE articles (
                id INTEGER NOT NULL,
                project_id TEXT NOT NULL,
                target_keyword TEXT,
                PRIMARY KEY (id, project_id)
            );",
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO articles (id, project_id, target_keyword) VALUES
                (1, 'proj1', 'adding custom categories to google sheets budget template'),
                (2, 'proj1', 'iron condor'),
                (3, 'proj1', NULL),
                (4, 'proj1', ''),
                (5, 'proj1', 'one two three four five');",
        )
        .unwrap();

        conn.execute_batch(MIGRATION_V38).unwrap();

        let keyword = |id: i64| -> Option<String> {
            conn.query_row(
                "SELECT target_keyword FROM articles WHERE id = ?1 AND project_id = 'proj1'",
                [id],
                |r| r.get(0),
            )
            .unwrap()
        };
        // >5 words → reset to empty so the next sync re-backfills normalized.
        assert_eq!(keyword(1), Some(String::new()));
        // ≤5 words, NULL, and already-empty keywords are untouched.
        assert_eq!(keyword(2), Some("iron condor".to_string()));
        assert_eq!(keyword(3), None);
        assert_eq!(keyword(4), Some(String::new()));
        assert_eq!(keyword(5), Some("one two three four five".to_string()));
    }
}
