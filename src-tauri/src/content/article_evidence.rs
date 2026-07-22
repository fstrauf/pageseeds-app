//! Durable per-article evidence catalog (facts + embeddings).
//!
//! Mirrors `engine/skills_search.rs`: SHA-256 content_hash skips re-embed,
//! embeddings stored as JSON `{ document, vec }`, degrades cleanly when Ollama
//! is unavailable (facts only; `embedding_json` NULL — no soft mega-cluster).
//!
//! Consumers (cluster desk, soft clusters) call `nearest_neighbors` / `get_row`
//! instead of recomputing first-200-words TF-IDF each audit.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};
use crate::models::article::Article;
use crate::rig::embeddings::EmbeddingBackend;

const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";
/// Max markdown headings included in outline_text / embed payload.
const MAX_OUTLINE_HEADINGS: usize = 16;
/// Max body characters appended to embed text (full-body word_count is separate).
const EMBED_BODY_EXCERPT_CHARS: usize = 1500;

// ─── Types ───────────────────────────────────────────────────────────────────

/// One row from `article_evidence`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleEvidence {
    pub project_id: String,
    pub article_id: i64,
    pub slug: String,
    pub content_hash: String,
    pub embedding_json: Option<String>,
    pub model_name: Option<String>,
    pub outline_text: Option<String>,
    pub summary_text: Option<String>,
    /// Nullable for v1 (no LLM extract required).
    pub intent_card: Option<String>,
    pub word_count: i64,
    pub h1: Option<String>,
    pub title: Option<String>,
    pub target_keyword: Option<String>,
    pub top_queries_json: String,
    pub updated_at: String,
}

/// Neighbor article ranked by cosine similarity on stored embeddings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Neighbor {
    pub slug: String,
    pub article_id: i64,
    pub title: Option<String>,
    pub similarity: f64,
}

/// Result of `index_stale`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexReport {
    pub total_articles: usize,
    /// Rows whose facts were written or updated this run.
    pub facts_upserted: usize,
    /// Articles skipped because content_hash matched (and embed state was sufficient).
    pub skipped_unchanged: usize,
    /// Articles that received a new embedding vector this run.
    pub embedded: usize,
    pub embeddings_available: bool,
    pub errors: Vec<String>,
}

/// Coverage of live articles vs evidence rows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageReport {
    pub total_live: usize,
    pub indexed: usize,
    pub with_embedding: usize,
    /// Live articles with no evidence row.
    pub missing: usize,
    /// Evidence rows with `embedding_json` NULL (facts-only / needs re-embed).
    pub stale: usize,
    pub pct_indexed: f64,
}

/// JSON-serializable embedding for SQLite (same shape as skill_embeddings).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEmbedding {
    document: String,
    vec: Vec<f64>,
}

/// Extracted durable facts from an MDX file (before embed).
#[derive(Debug, Clone)]
struct ArticleFacts {
    content_hash: String,
    word_count: i64,
    h1: Option<String>,
    title: String,
    target_keyword: Option<String>,
    outline_text: String,
    embed_text: String,
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Index all live articles that are missing or whose content_hash changed.
///
/// When Ollama is unavailable: still upserts facts with `embedding_json` NULL.
/// Does **not** invent fallback clusters.
pub async fn index_stale(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
) -> Result<IndexReport> {
    let embeddings_available = crate::rig::embeddings::check_ollama_health(None)
        .await
        .unwrap_or(false);

    let backend = if embeddings_available {
        Some(EmbeddingBackend::default_ollama())
    } else {
        None
    };

    index_stale_with_backend(conn, project_id, project_path, backend.as_ref()).await
}

/// Same as `index_stale` but with an injectable backend (tests pass `None`).
pub async fn index_stale_with_backend(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    backend: Option<&EmbeddingBackend>,
) -> Result<IndexReport> {
    let embeddings_available = backend.is_some();
    let articles = load_live_articles(conn, project_id, project_path)?;
    let content_dir = resolve_content_dir(project_path)?;

    let mut report = IndexReport {
        total_articles: articles.len(),
        facts_upserted: 0,
        skipped_unchanged: 0,
        embedded: 0,
        embeddings_available,
        errors: Vec::new(),
    };

    for article in &articles {
        match index_one_article(conn, project_id, article, &content_dir, backend).await {
            Ok(IndexOneOutcome::Skipped) => report.skipped_unchanged += 1,
            Ok(IndexOneOutcome::FactsOnly) => report.facts_upserted += 1,
            Ok(IndexOneOutcome::Embedded) => {
                report.facts_upserted += 1;
                report.embedded += 1;
            }
            Ok(IndexOneOutcome::EmbedOnly) => {
                report.embedded += 1;
            }
            Err(e) => {
                report.errors.push(format!(
                    "slug={}: {}",
                    article.url_slug,
                    e
                ));
            }
        }
    }

    Ok(report)
}

/// Best-effort reindex of a single article after write/sync.
///
/// Updates facts always; clears embedding when content_hash changes.
/// Never fails the parent write — logs a warning on error.
/// Embeddings are filled by a later full `index_stale` when Ollama is available.
pub fn maybe_reindex_article(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    slug: &str,
) {
    if let Err(e) = reindex_article_facts(conn, project_id, project_path, slug) {
        log::warn!(
            "[article_evidence] maybe_reindex_article failed project={} slug={}: {}",
            project_id,
            slug,
            e
        );
    }
}

/// Load one evidence row by slug.
pub fn get_row(conn: &Connection, project_id: &str, slug: &str) -> Result<Option<ArticleEvidence>> {
    let mut stmt = conn.prepare(
        r#"SELECT project_id, article_id, slug, content_hash, embedding_json, model_name,
                  outline_text, summary_text, intent_card, word_count, h1, title,
                  target_keyword, top_queries_json, updated_at
           FROM article_evidence
           WHERE project_id = ?1 AND slug = ?2"#,
    )?;
    let row = stmt
        .query_row(rusqlite::params![project_id, slug], map_evidence_row)
        .optional()?;
    Ok(row)
}

/// Load one evidence row by article_id.
pub fn get_row_by_article_id(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
) -> Result<Option<ArticleEvidence>> {
    let mut stmt = conn.prepare(
        r#"SELECT project_id, article_id, slug, content_hash, embedding_json, model_name,
                  outline_text, summary_text, intent_card, word_count, h1, title,
                  target_keyword, top_queries_json, updated_at
           FROM article_evidence
           WHERE project_id = ?1 AND article_id = ?2"#,
    )?;
    let row = stmt
        .query_row(rusqlite::params![project_id, article_id], map_evidence_row)
        .optional()?;
    Ok(row)
}

/// Cosine nearest neighbors using stored embedding vectors (no TF-IDF, no live embed).
///
/// Returns empty vec if the query article has no embedding or none of the peers do.
pub fn nearest_neighbors(
    conn: &Connection,
    project_id: &str,
    slug: &str,
    limit: usize,
    min_similarity: f64,
) -> Result<Vec<Neighbor>> {
    let query = match get_row(conn, project_id, slug)? {
        Some(row) => row,
        None => return Ok(vec![]),
    };
    let query_vec = match parse_embedding_vec(query.embedding_json.as_deref()) {
        Some(v) => v,
        None => return Ok(vec![]),
    };

    let mut stmt = conn.prepare(
        r#"SELECT article_id, slug, title, embedding_json
           FROM article_evidence
           WHERE project_id = ?1 AND slug != ?2 AND embedding_json IS NOT NULL"#,
    )?;

    let rows = stmt.query_map(rusqlite::params![project_id, slug], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
        ))
    })?;

    let mut scored: Vec<Neighbor> = Vec::new();
    for row in rows {
        let (article_id, peer_slug, title, emb_json) = row?;
        let Some(vec) = parse_embedding_vec(emb_json.as_deref()) else {
            continue;
        };
        let sim = cosine_similarity(&query_vec, &vec);
        if sim >= min_similarity {
            scored.push(Neighbor {
                slug: peer_slug,
                article_id,
                title,
                similarity: sim,
            });
        }
    }

    scored.sort_by(|a, b| {
        b.similarity
            .partial_cmp(&a.similarity)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(limit);
    Ok(scored)
}

/// Coverage of live articles vs the evidence table.
///
/// Does not re-read MDX files. `stale` counts rows with NULL embeddings
/// (facts-only / awaiting embed). Redirected slugs are not filtered here
/// (callers with a project path can pre-filter via `load_redirect_source_slugs`).
pub fn coverage(conn: &Connection, project_id: &str) -> Result<CoverageReport> {
    let total_live: i64 = conn.query_row(
        "SELECT COUNT(*) FROM articles WHERE project_id = ?1",
        [project_id],
        |row| row.get(0),
    )?;

    let indexed: i64 = conn.query_row(
        r#"SELECT COUNT(*) FROM article_evidence e
           INNER JOIN articles a ON a.id = e.article_id AND a.project_id = e.project_id
           WHERE e.project_id = ?1"#,
        [project_id],
        |row| row.get(0),
    )?;

    let with_embedding: i64 = conn.query_row(
        r#"SELECT COUNT(*) FROM article_evidence e
           INNER JOIN articles a ON a.id = e.article_id AND a.project_id = e.project_id
           WHERE e.project_id = ?1 AND e.embedding_json IS NOT NULL AND e.embedding_json != ''"#,
        [project_id],
        |row| row.get(0),
    )?;

    let stale: i64 = conn.query_row(
        r#"SELECT COUNT(*) FROM article_evidence e
           INNER JOIN articles a ON a.id = e.article_id AND a.project_id = e.project_id
           WHERE e.project_id = ?1 AND (e.embedding_json IS NULL OR e.embedding_json = '')"#,
        [project_id],
        |row| row.get(0),
    )?;

    let total_live = total_live as usize;
    let indexed = indexed as usize;
    let missing = total_live.saturating_sub(indexed);
    let pct_indexed = if total_live == 0 {
        100.0
    } else {
        (indexed as f64 / total_live as f64) * 100.0
    };

    Ok(CoverageReport {
        total_live,
        indexed,
        with_embedding: with_embedding as usize,
        missing,
        stale: stale as usize,
        pct_indexed,
    })
}

// ─── Internals ───────────────────────────────────────────────────────────────

enum IndexOneOutcome {
    Skipped,
    FactsOnly,
    Embedded,
    EmbedOnly,
}

async fn index_one_article(
    conn: &Connection,
    project_id: &str,
    article: &Article,
    content_dir: &Path,
    backend: Option<&EmbeddingBackend>,
) -> Result<IndexOneOutcome> {
    let path = article_file_path(content_dir, &article.file);
    let raw = std::fs::read_to_string(&path).map_err(|e| {
        Error::Other(format!(
            "read {}: {}",
            path.display(),
            e
        ))
    })?;

    let facts = extract_facts(&raw, article)?;
    let existing = get_row_by_article_id(conn, project_id, article.id)?;
    let embeddings_available = backend.is_some();

    let has_embedding = existing
        .as_ref()
        .and_then(|r| r.embedding_json.as_ref())
        .map(|j| !j.is_empty())
        .unwrap_or(false);

    let hash_matches = existing
        .as_ref()
        .map(|r| r.content_hash == facts.content_hash)
        .unwrap_or(false);

    // Unchanged content_hash skips re-embed when we already have a vector,
    // or when embeddings are unavailable (facts already stored).
    if hash_matches && (has_embedding || !embeddings_available) {
        return Ok(IndexOneOutcome::Skipped);
    }

    // Hash same, no embedding, backend available → embed only (keep facts).
    if hash_matches && !has_embedding {
        if let Some(backend) = backend {
            let emb = embed_text(backend, &facts.embed_text).await?;
            let json = serde_json::to_string(&emb)
                .map_err(|e| Error::Other(format!("embed json: {e}")))?;
            let now = chrono::Utc::now().to_rfc3339();
            conn.execute(
                r#"UPDATE article_evidence
                   SET embedding_json = ?1, model_name = ?2, updated_at = ?3
                   WHERE project_id = ?4 AND article_id = ?5"#,
                rusqlite::params![
                    json,
                    DEFAULT_EMBEDDING_MODEL,
                    now,
                    project_id,
                    article.id
                ],
            )?;
            return Ok(IndexOneOutcome::EmbedOnly);
        }
    }

    // Facts changed (or first index): upsert facts; embed if available.
    let embedding_json = if let Some(backend) = backend {
        let emb = embed_text(backend, &facts.embed_text).await?;
        Some(
            serde_json::to_string(&emb)
                .map_err(|e| Error::Other(format!("embed json: {e}")))?,
        )
    } else {
        None
    };

    upsert_evidence(
        conn,
        project_id,
        article.id,
        &article.url_slug,
        &facts,
        embedding_json.as_deref(),
        if embedding_json.is_some() {
            Some(DEFAULT_EMBEDDING_MODEL)
        } else {
            None
        },
    )?;

    if embedding_json.is_some() {
        Ok(IndexOneOutcome::Embedded)
    } else {
        Ok(IndexOneOutcome::FactsOnly)
    }
}

fn reindex_article_facts(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
    slug: &str,
) -> Result<()> {
    let articles = crate::engine::task_store::list_articles(conn, project_id)?;
    let article = articles
        .into_iter()
        .find(|a| a.url_slug == slug)
        .ok_or_else(|| Error::Other(format!("article slug not found: {slug}")))?;

    let content_dir = resolve_content_dir(project_path)?;
    let path = article_file_path(&content_dir, &article.file);
    let raw = std::fs::read_to_string(&path)?;
    let facts = extract_facts(&raw, &article)?;

    let existing = get_row_by_article_id(conn, project_id, article.id)?;
    let hash_matches = existing
        .as_ref()
        .map(|r| r.content_hash == facts.content_hash)
        .unwrap_or(false);

    // Preserve embedding only when content is unchanged.
    let (embedding_json, model_name) = if hash_matches {
        (
            existing.as_ref().and_then(|r| r.embedding_json.clone()),
            existing.as_ref().and_then(|r| r.model_name.clone()),
        )
    } else {
        (None, None)
    };

    upsert_evidence(
        conn,
        project_id,
        article.id,
        &article.url_slug,
        &facts,
        embedding_json.as_deref(),
        model_name.as_deref(),
    )?;
    Ok(())
}

fn upsert_evidence(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    slug: &str,
    facts: &ArticleFacts,
    embedding_json: Option<&str>,
    model_name: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        r#"INSERT INTO article_evidence (
               project_id, article_id, slug, content_hash, embedding_json, model_name,
               outline_text, summary_text, intent_card, word_count, h1, title,
               target_keyword, top_queries_json, updated_at
           ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, NULL, ?8, ?9, ?10, ?11, '[]', ?12)
           ON CONFLICT(project_id, article_id) DO UPDATE SET
               slug = excluded.slug,
               content_hash = excluded.content_hash,
               embedding_json = excluded.embedding_json,
               model_name = excluded.model_name,
               outline_text = excluded.outline_text,
               word_count = excluded.word_count,
               h1 = excluded.h1,
               title = excluded.title,
               target_keyword = excluded.target_keyword,
               updated_at = excluded.updated_at"#,
        rusqlite::params![
            project_id,
            article_id,
            slug,
            &facts.content_hash,
            embedding_json,
            model_name,
            &facts.outline_text,
            facts.word_count,
            facts.h1.as_deref(),
            &facts.title,
            facts.target_keyword.as_deref(),
            now,
        ],
    )?;
    Ok(())
}

async fn embed_text(backend: &EmbeddingBackend, text: &str) -> Result<StoredEmbedding> {
    let embedding = backend
        .embed_text(text)
        .await
        .map_err(|e| Error::Other(format!("embedding failed: {e}")))?;
    Ok(StoredEmbedding {
        document: embedding.document,
        vec: embedding.vec,
    })
}

fn extract_facts(raw: &str, article: &Article) -> Result<ArticleFacts> {
    let content_hash = hash_content(raw);
    let (fm, body) = crate::content::frontmatter::split_mdx(raw)
        .map(|(f, b)| (Some(f), b))
        .unwrap_or((None, raw));

    let title = fm
        .and_then(|f| extract_fm_value(f, "title"))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| article.title.clone());

    let h1 = first_h1(body);
    let word_count = crate::content::ops::count_words(body) as i64;
    let outline_text = build_outline(body, MAX_OUTLINE_HEADINGS);
    let target_keyword = article.target_keyword.clone().filter(|k| !k.is_empty());

    let mut embed_parts: Vec<String> = Vec::new();
    if !title.is_empty() {
        embed_parts.push(format!("Title: {title}"));
    }
    if let Some(ref h) = h1 {
        embed_parts.push(format!("H1: {h}"));
    }
    if let Some(ref kw) = target_keyword {
        embed_parts.push(format!("Keyword: {kw}"));
    }
    if !outline_text.is_empty() {
        embed_parts.push(format!("Outline:\n{outline_text}"));
    }
    let excerpt = body.chars().take(EMBED_BODY_EXCERPT_CHARS).collect::<String>();
    if !excerpt.trim().is_empty() {
        embed_parts.push(format!("Body:\n{}", excerpt.trim()));
    }
    let embed_text = embed_parts.join("\n\n");

    Ok(ArticleFacts {
        content_hash,
        word_count,
        h1,
        title,
        target_keyword,
        outline_text,
        embed_text,
    })
}

fn load_live_articles(
    conn: &Connection,
    project_id: &str,
    project_path: &Path,
) -> Result<Vec<Article>> {
    let redirected = crate::content::redirects::load_redirect_source_slugs(
        project_path
            .to_str()
            .unwrap_or(""),
    );
    let articles = crate::engine::task_store::list_articles(conn, project_id)?;
    Ok(articles
        .into_iter()
        .filter(|a| {
            let slug = crate::content::slug::normalize_url_slug(&a.url_slug);
            !redirected.contains(&slug)
        })
        .collect())
}

fn resolve_content_dir(project_path: &Path) -> Result<PathBuf> {
    let automation_dir = project_path.join(".github").join("automation");
    crate::content::ops::resolve_content_dir(&automation_dir, project_path)
        .map_err(Error::Other)
}

fn article_file_path(content_dir: &Path, file_ref: &str) -> PathBuf {
    let basename = Path::new(file_ref)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(file_ref);
    content_dir.join(basename)
}

fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn first_h1(body: &str) -> Option<String> {
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            // Single-# heading only (not ##)
            let text = rest.trim();
            if !text.is_empty() {
                return Some(text.to_string());
            }
        }
    }
    None
}

fn build_outline(body: &str, max_headings: usize) -> String {
    let mut headings = Vec::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            let level = trimmed.chars().take_while(|c| *c == '#').count();
            if level >= 1 && level <= 6 {
                let text = trimmed[level..].trim();
                if !text.is_empty() {
                    headings.push(format!("{} {}", "#".repeat(level), text));
                    if headings.len() >= max_headings {
                        break;
                    }
                }
            }
        }
    }
    headings.join("\n")
}

fn extract_fm_value(frontmatter: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(&prefix) {
            let v = rest.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

fn parse_embedding_vec(json: Option<&str>) -> Option<Vec<f64>> {
    let json = json.filter(|s| !s.is_empty())?;
    let stored: StoredEmbedding = serde_json::from_str(json).ok()?;
    if stored.vec.is_empty() {
        None
    } else {
        Some(stored.vec)
    }
}

fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

fn map_evidence_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArticleEvidence> {
    Ok(ArticleEvidence {
        project_id: row.get(0)?,
        article_id: row.get(1)?,
        slug: row.get(2)?,
        content_hash: row.get(3)?,
        embedding_json: row.get(4)?,
        model_name: row.get(5)?,
        outline_text: row.get(6)?,
        summary_text: row.get(7)?,
        intent_card: row.get(8)?,
        word_count: row.get(9)?,
        h1: row.get(10)?,
        title: row.get(11)?,
        target_keyword: row.get(12)?,
        top_queries_json: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("{}_{}", prefix, nanos))
    }

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn setup_project(conn: &Connection, project_id: &str, path: &Path) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES (?1, ?2, ?3, 1, 'workspace')",
            [project_id, "Test Project", path.to_str().unwrap()],
        )
        .unwrap();
    }

    fn insert_article(
        conn: &Connection,
        project_id: &str,
        id: i64,
        slug: &str,
        title: &str,
        file: &str,
        keyword: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO articles (
                id, title, url_slug, file, target_keyword, status,
                content_gaps_addressed, project_id, word_count
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'published', '[]', ?6, 0)",
            rusqlite::params![id, title, slug, file, keyword, project_id],
        )
        .unwrap();
    }

    fn write_workspace(dir: &Path, content_dir_rel: &str) {
        let auto = dir.join(".github").join("automation");
        std::fs::create_dir_all(&auto).unwrap();
        std::fs::write(
            auto.join("seo_workspace.json"),
            format!(r#"{{"content_dir":"{}"}}"#, content_dir_rel),
        )
        .unwrap();
    }

    fn write_mdx(path: &Path, title: &str, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let content = format!(
            "---\ntitle: \"{}\"\ndate: \"2024-01-01\"\n---\n\n{}\n",
            title, body
        );
        std::fs::write(path, content).unwrap();
    }

    fn seed_embedding(
        conn: &Connection,
        project_id: &str,
        article_id: i64,
        slug: &str,
        title: &str,
        hash: &str,
        vec: Vec<f64>,
        word_count: i64,
    ) {
        let stored = StoredEmbedding {
            document: format!("doc:{slug}"),
            vec,
        };
        let json = serde_json::to_string(&stored).unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            r#"INSERT INTO article_evidence (
                   project_id, article_id, slug, content_hash, embedding_json, model_name,
                   outline_text, word_count, h1, title, target_keyword, top_queries_json, updated_at
               ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', ?7, NULL, ?8, NULL, '[]', ?9)"#,
            rusqlite::params![
                project_id,
                article_id,
                slug,
                hash,
                json,
                DEFAULT_EMBEDDING_MODEL,
                word_count,
                title,
                now
            ],
        )
        .unwrap();
    }

    #[test]
    fn hash_content_stable() {
        assert_eq!(hash_content("hello"), hash_content("hello"));
        assert_ne!(hash_content("hello"), hash_content("world"));
    }

    #[test]
    fn cosine_similarity_identical_is_one() {
        let v = vec![1.0, 2.0, 3.0];
        let sim = cosine_similarity(&v, &v);
        assert!((sim - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_similarity_orthogonal_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!((cosine_similarity(&a, &b) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn first_h1_takes_first_single_hash_only() {
        let body = "## Not H1\n\n# Real Title\n\n# Second H1\n";
        assert_eq!(first_h1(body).as_deref(), Some("Real Title"));
    }

    #[test]
    fn word_count_uses_full_body_not_first_200() {
        let dir = unique_temp_dir("ps_ae_wc");
        let content_dir = dir.join("content");
        write_workspace(&dir, "content");

        // Build a body with well over 200 words.
        let words: Vec<&str> = (0..250).map(|_| "word").collect();
        let long_body = format!("# Full Body Title\n\n{}", words.join(" "));
        write_mdx(&content_dir.join("001_long.mdx"), "Long Article", &long_body);

        let conn = in_memory_db();
        setup_project(&conn, "p1", &dir);
        insert_article(
            &conn,
            "p1",
            1,
            "long",
            "Long Article",
            "./content/001_long.mdx",
            Some("test keyword"),
        );

        let raw = std::fs::read_to_string(content_dir.join("001_long.mdx")).unwrap();
        let article = crate::engine::task_store::list_articles(&conn, "p1")
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let facts = extract_facts(&raw, &article).unwrap();

        let (_, body) = crate::content::frontmatter::split_mdx(&raw).unwrap();
        let expected = crate::content::ops::count_words(body) as i64;
        assert_eq!(facts.word_count, expected);
        assert!(facts.word_count > 200, "expected full-body count > 200, got {}", facts.word_count);
        assert_eq!(facts.h1.as_deref(), Some("Full Body Title"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn index_stale_hash_skip_without_ollama() {
        let dir = unique_temp_dir("ps_ae_skip");
        let content_dir = dir.join("content");
        write_workspace(&dir, "content");
        write_mdx(
            &content_dir.join("001_a.mdx"),
            "Article A",
            "# Heading A\n\nSome body text here for indexing.",
        );

        let conn = in_memory_db();
        setup_project(&conn, "p1", &dir);
        insert_article(
            &conn,
            "p1",
            1,
            "article-a",
            "Article A",
            "./content/001_a.mdx",
            None,
        );

        let rt = tokio::runtime::Runtime::new().unwrap();
        let report1 = rt
            .block_on(index_stale_with_backend(&conn, "p1", &dir, None))
            .unwrap();
        assert_eq!(report1.total_articles, 1);
        assert_eq!(report1.facts_upserted, 1);
        assert_eq!(report1.embedded, 0);
        assert!(!report1.embeddings_available);
        assert!(report1.errors.is_empty());

        let row = get_row(&conn, "p1", "article-a").unwrap().unwrap();
        assert!(row.embedding_json.is_none());
        assert!(row.word_count > 0);
        assert_eq!(row.h1.as_deref(), Some("Heading A"));

        // Second run with same content → full skip
        let report2 = rt
            .block_on(index_stale_with_backend(&conn, "p1", &dir, None))
            .unwrap();
        assert_eq!(report2.skipped_unchanged, 1);
        assert_eq!(report2.facts_upserted, 0);
        assert_eq!(report2.embedded, 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nearest_neighbors_ranks_by_cosine() {
        let conn = in_memory_db();
        let dir = unique_temp_dir("ps_ae_nn");
        setup_project(&conn, "p1", &dir);
        insert_article(&conn, "p1", 1, "alpha", "Alpha", "./content/a.mdx", None);
        insert_article(&conn, "p1", 2, "beta", "Beta", "./content/b.mdx", None);
        insert_article(&conn, "p1", 3, "gamma", "Gamma", "./content/c.mdx", None);

        // alpha and beta similar; gamma orthogonal-ish
        seed_embedding(&conn, "p1", 1, "alpha", "Alpha", "h1", vec![1.0, 0.0, 0.0], 100);
        seed_embedding(&conn, "p1", 2, "beta", "Beta", "h2", vec![0.9, 0.1, 0.0], 100);
        seed_embedding(&conn, "p1", 3, "gamma", "Gamma", "h3", vec![0.0, 0.0, 1.0], 100);

        let neighbors = nearest_neighbors(&conn, "p1", "alpha", 5, 0.0).unwrap();
        assert_eq!(neighbors.len(), 2);
        assert_eq!(neighbors[0].slug, "beta");
        assert!(neighbors[0].similarity > neighbors[1].similarity);
        assert_eq!(neighbors[1].slug, "gamma");

        // min_similarity filters gamma
        let tight = nearest_neighbors(&conn, "p1", "alpha", 5, 0.5).unwrap();
        assert_eq!(tight.len(), 1);
        assert_eq!(tight[0].slug, "beta");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn nearest_neighbors_empty_without_embedding() {
        let conn = in_memory_db();
        let dir = unique_temp_dir("ps_ae_nn_empty");
        setup_project(&conn, "p1", &dir);
        insert_article(&conn, "p1", 1, "alpha", "Alpha", "./content/a.mdx", None);
        // Row with facts only, no embedding
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            r#"INSERT INTO article_evidence (
                   project_id, article_id, slug, content_hash, embedding_json,
                   word_count, title, top_queries_json, updated_at
               ) VALUES ('p1', 1, 'alpha', 'h', NULL, 10, 'Alpha', '[]', ?1)"#,
            [&now],
        )
        .unwrap();

        let neighbors = nearest_neighbors(&conn, "p1", "alpha", 5, 0.0).unwrap();
        assert!(neighbors.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn coverage_counts_indexed_and_stale() {
        let conn = in_memory_db();
        let dir = unique_temp_dir("ps_ae_cov");
        setup_project(&conn, "p1", &dir);
        insert_article(&conn, "p1", 1, "a", "A", "./content/a.mdx", None);
        insert_article(&conn, "p1", 2, "b", "B", "./content/b.mdx", None);
        insert_article(&conn, "p1", 3, "c", "C", "./content/c.mdx", None);

        seed_embedding(&conn, "p1", 1, "a", "A", "h1", vec![1.0, 0.0], 50);
        // facts-only row for b
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            r#"INSERT INTO article_evidence (
                   project_id, article_id, slug, content_hash, embedding_json,
                   word_count, title, top_queries_json, updated_at
               ) VALUES ('p1', 2, 'b', 'h2', NULL, 20, 'B', '[]', ?1)"#,
            [&now],
        )
        .unwrap();
        // c missing entirely

        let cov = coverage(&conn, "p1").unwrap();
        assert_eq!(cov.total_live, 3);
        assert_eq!(cov.indexed, 2);
        assert_eq!(cov.with_embedding, 1);
        assert_eq!(cov.missing, 1);
        assert_eq!(cov.stale, 1);
        assert!((cov.pct_indexed - 66.666).abs() < 0.1);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn maybe_reindex_updates_facts_and_clears_stale_embedding() {
        let dir = unique_temp_dir("ps_ae_maybe");
        let content_dir = dir.join("content");
        write_workspace(&dir, "content");
        write_mdx(
            &content_dir.join("001_x.mdx"),
            "Original",
            "# Original H1\n\nOriginal body words here.",
        );

        let conn = in_memory_db();
        setup_project(&conn, "p1", &dir);
        insert_article(
            &conn,
            "p1",
            1,
            "x",
            "Original",
            "./content/001_x.mdx",
            None,
        );

        // Seed with embedding for old hash
        seed_embedding(
            &conn,
            "p1",
            1,
            "x",
            "Original",
            "old-hash",
            vec![1.0, 0.0],
            5,
        );

        // Change file content
        write_mdx(
            &content_dir.join("001_x.mdx"),
            "Updated",
            "# Updated H1\n\nUpdated body with more words for counting.",
        );

        maybe_reindex_article(&conn, "p1", &dir, "x");

        let row = get_row(&conn, "p1", "x").unwrap().unwrap();
        assert_eq!(row.h1.as_deref(), Some("Updated H1"));
        assert_eq!(row.title.as_deref(), Some("Updated"));
        assert!(row.embedding_json.is_none(), "stale embedding must clear on hash change");
        assert_ne!(row.content_hash, "old-hash");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stored_embedding_roundtrip() {
        let original = StoredEmbedding {
            document: "test".to_string(),
            vec: vec![0.1, 0.2, 0.3],
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: StoredEmbedding = serde_json::from_str(&json).unwrap();
        assert_eq!(original.vec, back.vec);
    }
}
