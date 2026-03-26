use rusqlite::Connection;
use std::path::Path;

use crate::error::Result;

pub mod export;

/// Get the default database path based on platform conventions.
/// Used when we need to access the DB without having the AppState.
pub fn default_db_path() -> std::path::PathBuf {
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
        let _ = conn.execute_batch(
            "ALTER TABLE projects ADD COLUMN agent_provider TEXT DEFAULT 'copilot';",
        );
        conn.execute(
            "INSERT OR IGNORE INTO schema_version (version, applied_at) VALUES (4, ?1)",
            [chrono::Utc::now().to_rfc3339()],
        )?;
    }

    if version < 5 {
        // Add mention_stance column to reddit_opportunities; ignore error if already exists
        let _ = conn.execute_batch(
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

    Ok(())
}
