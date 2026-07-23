# Data Persistence

PageSeeds uses a **dual-store architecture**:
- **SQLite** for runtime state (tasks, logs, opportunities)
- **JSON files** in the user's repo for committed content data

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                        DATA PERSISTENCE                                 │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│   ┌──────────────────────────────────────────────────────────────┐     │
│   │   RUNTIME STATE (SQLite)                                     │     │
│   │                                                              │     │
│   │   ┌──────────┐  ┌──────────┐  ┌──────────────────┐          │     │
│   │   │  tasks   │  │ projects │  │reddit_opportunities│          │     │
│   │   └──────────┘  └──────────┘  └──────────────────┘          │     │
│   │   ┌──────────┐  ┌──────────┐  ┌──────────────────┐          │     │
│   │   │app_logs  │  │ task_idempotency_keys          │          │     │
│   │   └──────────┘  └──────────┘  └──────────────────┘          │     │
│   │                                                              │     │
│   │   Location: ~/Library/Application Support/.../*.db          │     │
│   └──────────────────────────────────────────────────────────────┘     │
│                              │                                          │
│                              │ db::export module                        │
│                              ▼                                          │
│   ┌──────────────────────────────────────────────────────────────┐     │
│   │   COMMITTED CONTENT (JSON)                                   │     │
│   │                                                              │     │
│   │   {project_root}/                                            │     │
│   │   └── content_automation/                                    │     │
│   │       ├── articles.json          # Source of truth for        │     │
│   │       ├── task_list.json         # SEO content inventory      │     │
│   │       ├── gsc_collection.json    # URL inspection results     │     │
│   │       ├── gsc_summary.json       # Grouped analysis           │     │
│   │       ├── content_audit.json     # Health check results       │     │
│   │       ├── recommendations.json   # Review suggestions         │     │
│   │       └── reddit_config.md       # Search parameters          │     │
│   │                                                              │     │
│   │   Git-tracked: Yes (collaboration & history)                │     │
│   └──────────────────────────────────────────────────────────────┘     │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## SQLite (Runtime State)

### Location

```
macOS: ~/Library/Application Support/com.pageseeds.app/pageseeds.db
Linux: ~/.local/share/com.pageseeds.app/pageseeds.db
Windows: %APPDATA%\com.pageseeds.app\pageseeds.db
```

### Schema

#### `tasks` Table

```sql
CREATE TABLE tasks (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    task_type TEXT NOT NULL,
    phase TEXT NOT NULL,
    status TEXT NOT NULL,          -- todo | in_progress | review | done | cancelled
    priority TEXT NOT NULL,        -- high | medium | low
    execution_mode TEXT NOT NULL,  -- automatic | batchable | manual | spec
    agent_policy TEXT NOT NULL,    -- none | required | optional
    title TEXT,
    description TEXT,
    depends_on TEXT,               -- JSON array of task IDs
    artifacts TEXT,                -- JSON array of TaskArtifact
    run TEXT,                      -- JSON of TaskRun
    created_at TEXT NOT NULL,      -- RFC3339
    updated_at TEXT NOT NULL,      -- RFC3339
    FOREIGN KEY (project_id) REFERENCES projects(id)
);
```

#### `projects` Table

```sql
CREATE TABLE projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    path TEXT NOT NULL,            -- Absolute path to project root
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

#### `reddit_opportunities` Table

```sql
CREATE TABLE reddit_opportunities (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL,
    post_id TEXT NOT NULL UNIQUE,
    subreddit TEXT NOT NULL,
    title TEXT NOT NULL,
    url TEXT NOT NULL,
    selftext TEXT,
    author TEXT,
    created_utc INTEGER,
    score INTEGER,
    num_comments INTEGER,
    search_keyword TEXT,
    trigger_keyword TEXT,
    engagement_score REAL,
    accessibility_score REAL,
    relevance_score REAL,
    relevance_reason TEXT,
    pain_point TEXT,
    suggested_reply TEXT,
    reply_draft TEXT,
    reply_status TEXT NOT NULL,    -- pending | drafted | posted | skipped
    discovered_at TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES projects(id)
);
```

#### `app_logs` Table

```sql
CREATE TABLE app_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    level TEXT NOT NULL,           -- DEBUG | INFO | WARN | ERROR
    source TEXT NOT NULL,          -- frontend | backend | agent
    component TEXT NOT NULL,
    message TEXT NOT NULL,
    context TEXT,                  -- JSON object
    session_id TEXT,
    task_id TEXT
);
```

#### `task_idempotency_keys` Table

```sql
CREATE TABLE task_idempotency_keys (
    key TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    created_at TEXT NOT NULL,
    FOREIGN KEY (task_id) REFERENCES tasks(id) ON DELETE CASCADE
);
CREATE INDEX idx_idempotency_task ON task_idempotency_keys(task_id);
```

#### `article_evidence` Table (V49, issue #119)

Durable per-article catalog facts + optional embeddings. Mirrors `skill_embeddings`:
SHA-256 `content_hash` of full MDX skips re-embed; `word_count` is full-body via
`content::ops::count_words` (not first-200-words TF-IDF).

```sql
CREATE TABLE article_evidence (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id       TEXT NOT NULL,
    article_id       INTEGER NOT NULL,
    slug             TEXT NOT NULL,
    content_hash     TEXT NOT NULL,
    embedding_json   TEXT,              -- nullable: facts-only when Ollama missing
    model_name       TEXT,
    outline_text     TEXT,
    summary_text     TEXT,
    intent_card      TEXT,              -- nullable in v1 (no LLM extract required)
    word_count       INTEGER NOT NULL DEFAULT 0,
    h1               TEXT,
    title            TEXT,
    target_keyword   TEXT,
    top_queries_json TEXT NOT NULL DEFAULT '[]',
    updated_at       TEXT NOT NULL,
    UNIQUE(project_id, article_id)
);
```

**Invalidation:** re-read MDX → hash → if stored hash matches and embedding exists
(or embeddings unavailable), skip. Hash change clears/recomputes embedding when
Ollama is up.

**Degrade behavior:** if Ollama is unavailable, `index_stale` still upserts facts
with `embedding_json` NULL. There is **no** soft mega-cluster fallback — callers
must tolerate missing vectors (`nearest_neighbors` returns empty for unembedded
rows). Re-run `reindex_article_evidence` once Ollama + `nomic-embed-text` are
available to fill vectors (≥95% live coverage target when backend is up).

Module: `content/article_evidence.rs`. Commands: `reindex_article_evidence`,
`get_article_evidence_coverage`.

### Migrations

**Rule:** Never alter existing migration blocks. Always add new `MIGRATION_VN` constants.

```rust
// db/mod.rs

const MIGRATION_V1: &str = r#"
    CREATE TABLE IF NOT EXISTS tasks (...);
    CREATE TABLE IF NOT EXISTS projects (...);
"#;

const MIGRATION_V2: &str = r#"
    CREATE TABLE IF NOT EXISTS reddit_opportunities (...);
"#;

const MIGRATION_V3: &str = r#"
    CREATE TABLE IF NOT EXISTS app_logs (...);
"#;

pub fn init(db_path: &Path) -> Result<Connection> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch(MIGRATION_V1)?;
    conn.execute_batch(MIGRATION_V2)?;
    conn.execute_batch(MIGRATION_V3)?;
    // Future: conn.execute_batch(MIGRATION_V4)?;
    Ok(conn)
}
```

All migrations must be idempotent (`CREATE TABLE IF NOT EXISTS`, `ADD COLUMN IF NOT EXISTS`).

**Rule:** Every process/binary that opens `pageseeds.db` must open it via `db::init` (or `init_with_conn`). Raw `Connection::open` is only allowed downstream of a migrated connection — opening the DB without running migrations lets the schema drift behind the app and turns later writes into silent failures (issue #71).

---

## JSON Files (Committed Content)

### Project Structure

```
{project_root}/
├── content/                          # User's content files
│   ├── article-one.mdx
│   └── article-two.mdx
│
└── content_automation/               # SEO automation directory
    ├── manifest.json                 # Project configuration
    ├── articles.json                 # Content inventory
    ├── task_list.json                # Task definitions
    ├── reddit_config.md              # Reddit search params
    ├── SKILL.md                      # Agent instructions
    │
    └── artifacts/                    # Generated artifacts
        ├── gsc_collection.json
        ├── gsc_summary.json
        ├── content_audit.json
        └── recommendations.json
```

### articles.json

Source of truth for content inventory. Synced with MDX files on disk.

```json
{
  "articles": [
    {
      "id": 1,
      "title": "Article Title",
      "slug": "article-slug",
      "file": "content/article-slug.mdx",
      "status": "published",
      "target_keyword": "target keyword",
      "published_date": "2024-01-15",
      "first_published": "2024-01-15",
      "last_modified": "2024-03-20",
      "gsc": {
        "clicks": 150,
        "impressions": 3000,
        "ctr": 0.05,
        "position": 8.5
      },
      "content_health": {
        "score": 85,
        "checks_passed": 11,
        "checks_failed": 2
      }
    }
  ]
}
```

### task_list.json

Template tasks for the project.

```json
{
  "tasks": [
    {
      "task_type": "content_review",
      "phase": "verification",
      "title": "Monthly content review",
      "execution_mode": "manual"
    }
  ]
}
```

### manifest.json

Project configuration.

```json
{
  "name": "Project Name",
  "url": "https://example.com",
  "gsc_site": "https://example.com/",
  "sitemap": "https://example.com/sitemap.xml",
  "content_dir": "content",
  "automation_dir": "content_automation"
}
```

### gsc_collection.json

URL Inspection API results.

```json
{
  "meta": {
    "site_url": "https://example.com",
    "sitemap_url": "https://example.com/sitemap.xml",
    "collected_at": "2024-03-20T10:30:00Z",
    "total_urls": 150,
    "issues_found": 12
  },
  "counts": {
    "indexed_pass": 138,
    "not_indexed_crawled": 8,
    "robots_blocked": 4
  },
  "items": [
    {
      "url": "https://example.com/page",
      "verdict": "not_indexed",
      "coverage_state": "Crawled - currently not indexed",
      "reason_code": "not_indexed_crawled",
      "action": "Improve content quality and internal linking",
      "priority": 40
    }
  ]
}
```

---

## Data Flow

### Content Sync Flow

```
MDX files on disk  ◄────────►  articles.json  ◄────────►  SQLite tasks
     │                            │                           │
     │                            │                           │
     └────── content::ops ────────┘                           │
                                  └────── db::export ─────────┘
```

1. `content::ops::sync_articles()` reconciles MDX files with `articles.json`
2. `db::export::write_articles_to_repo()` writes `articles.json` after task changes

### Task Execution Flow

```
Task created in UI
       │
       ▼
┌──────────────┐
│ SQLite tasks │◄────── Source of truth for status
└──────────────┘
       │
       ▼
Task executed
       │
       ▼
Artifacts generated ──────► JSON files in automation/
       │
       ▼
Status updated in SQLite
```

---

## Export Module (`db/export.rs`)

Handles read/write of JSON files in the user's repo.

```rust
// Read articles from repo
pub fn read_articles_from_repo(automation_dir: &Path) -> Result<Vec<Article>>;

// Write articles to repo
pub fn write_articles_to_repo(conn: &Connection, automation_dir: &Path) -> Result<()>;

// Read task templates
pub fn read_task_templates(automation_dir: &Path) -> Result<Vec<TaskTemplate>>;
```

---

## Shared State (Tauri Managed State)

Three managed states declared in `lib.rs`:

| State | Type | Contents | Used By |
|-------|------|----------|---------|
| `AppState` | `Arc<Mutex<Connection>>` | SQLite connection | DB commands |
| `GscState` | `Mutex<Option<TokenState>>` | OAuth token cache | GSC commands |
| `SeoState` | `Mutex<HashMap<String, CachedSignature>>` | Ahrefs cache | SEO commands |

**Note:** `AppState` holds the main connection, but task execution opens **dedicated connections** per task for isolation.

---

## File Locations Reference

| Component | Path |
|-----------|------|
| SQLite init | `src-tauri/src/db/mod.rs` |
| Export functions | `src-tauri/src/db/export.rs` |
| Content operations | `src-tauri/src/content/ops.rs` |
| Project paths | `src-tauri/src/engine/project_paths.rs` |

---

## Dual-layer SEO measurement (CTR)

**BUSINESS RULE (issue #152):** closed-loop CTR measurement is two layers, not
per-fix review tasks.

| Layer | Store | Role |
|-------|--------|------|
| **Daily tape** | `gsc_page_daily` (append-only) | Per-page GSC series; 28-day windows for baseline/after metrics |
| **Change events** | `ctr_outcomes` | Sparse rows when a CTR fix ships (nested `fix_ctr_article` or Path B `fix-submit` kind=ctr) |

- Re-ship for the same article **supersedes** prior open/pending events.
- `deployed_at` is set only after live title verification, not at ship time.
- Do **not** spawn `ctr_outcome_review` from `after_task_success` (task type kept
  for legacy rows / handler registration only).
- Content outcome reviews (`content_outcome_review`) are a separate path and are
  not folded into this model.

---

## See Also

- [Workflow Engine](./WORKFLOW_ENGINE.md) — How tasks use this data
- [Workflow Engine](./WORKFLOW_ENGINE.md) — How runtime state is managed
