use rusqlite::Connection;
use std::path::Path;

use crate::error::Result;

pub mod export;
pub mod global_settings;

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
        let _ = conn.execute_batch("ALTER TABLE projects ADD COLUMN site_id TEXT;");
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (2, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 3 {
        let _ = conn.execute_batch(
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
        let _ = conn
            .execute_batch("ALTER TABLE projects ADD COLUMN agent_provider TEXT DEFAULT 'kimi';");
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (4, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 5 {
        // Add mention_stance column to reddit_opportunities; ignore error if already exists
        let _ =
            conn.execute_batch("ALTER TABLE reddit_opportunities ADD COLUMN mention_stance TEXT;");
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
        let _ = conn.execute_batch(
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
        // Add product_name column to reddit_opportunities for agentic config consumption
        let _ =
            conn.execute_batch("ALTER TABLE reddit_opportunities ADD COLUMN product_name TEXT;");
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (15, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 16 {
        let _ = conn.execute_batch(MIGRATION_V16);
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
        let _ = conn.execute_batch(MIGRATION_V17);
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
        let _ = conn.execute_batch(MIGRATION_V19);
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (19, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 20 {
        let _ = conn.execute_batch(MIGRATION_V20);
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
        let _ = conn.execute_batch(MIGRATION_V22);
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (22, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 23 {
        let _ = conn.execute_batch(MIGRATION_V23);
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

    Ok(())
}

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
