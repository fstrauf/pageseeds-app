use chrono::Utc;
use rusqlite::{params, Connection};

use crate::error::Result;
use crate::models::reddit::{MigrationResult, RedditOpportunity, RedditStats};

pub fn upsert_opportunity(conn: &Connection, opp: &RedditOpportunity) -> Result<()> {
    let pain_points_json = serde_json::to_string(&opp.key_pain_points).unwrap_or_default();
    let now = Utc::now().to_rfc3339();

    conn.execute(
        r#"INSERT INTO reddit_opportunities (
            post_id, title, selftext, url, subreddit, author, posted_date, upvotes, comment_count,
            relevance_score, engagement_score, accessibility_score, final_score,
            severity, why_relevant, key_pain_points, website_fit, mention_stance, product_name,
            reply_status, reply_text, reply_url, reply_upvotes, reply_replies, posted_at,
            project_id, created_at, updated_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26,?27,?28)
        ON CONFLICT(post_id) DO UPDATE SET
            title               = excluded.title,
            selftext            = excluded.selftext,
            url                 = excluded.url,
            subreddit           = excluded.subreddit,
            author              = excluded.author,
            upvotes             = excluded.upvotes,
            comment_count       = excluded.comment_count,
            relevance_score     = excluded.relevance_score,
            engagement_score    = excluded.engagement_score,
            accessibility_score = excluded.accessibility_score,
            final_score         = excluded.final_score,
            severity            = excluded.severity,
            why_relevant        = excluded.why_relevant,
            key_pain_points     = excluded.key_pain_points,
            website_fit         = excluded.website_fit,
            mention_stance      = excluded.mention_stance,
            product_name        = excluded.product_name,
            reply_text          = excluded.reply_text,
            -- Revive stale rows on rediscovery (stale -> pending); every other
            -- status (pending/posted/skipped) is preserved as-is.
            reply_status        = CASE
                WHEN reddit_opportunities.reply_status = 'stale' THEN 'pending'
                ELSE reddit_opportunities.reply_status
            END,
            updated_at          = excluded.updated_at"#,
        params![
            opp.post_id, opp.title, opp.selftext, opp.url, opp.subreddit, opp.author,
            opp.posted_date, opp.upvotes, opp.comment_count,
            opp.relevance_score, opp.engagement_score, opp.accessibility_score, opp.final_score,
            opp.severity, opp.why_relevant, pain_points_json, opp.website_fit, opp.mention_stance, opp.product_name,
            opp.reply_status, opp.reply_text, opp.reply_url,
            opp.reply_upvotes, opp.reply_replies, opp.posted_at,
            opp.project_id, now.clone(), now,
        ],
    )?;
    Ok(())
}

pub fn get_opportunity(conn: &Connection, post_id: &str) -> Result<RedditOpportunity> {
    let opp = conn.query_row(
        "SELECT * FROM reddit_opportunities WHERE post_id=?1",
        params![post_id],
        row_to_opportunity,
    )?;
    Ok(opp)
}

pub fn list_opportunities(
    conn: &Connection,
    project_id: &str,
    status: Option<&str>,
) -> Result<Vec<RedditOpportunity>> {
    if let Some(s) = status {
        let mut stmt = conn.prepare(
            "SELECT * FROM reddit_opportunities WHERE project_id=?1 AND reply_status=?2 ORDER BY final_score DESC, created_at DESC",
        )?;
        let opps = stmt
            .query_map(params![project_id, s], row_to_opportunity)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(opps)
    } else {
        let mut stmt = conn.prepare(
            "SELECT * FROM reddit_opportunities WHERE project_id=?1 ORDER BY final_score DESC, created_at DESC",
        )?;
        let opps = stmt
            .query_map(params![project_id], row_to_opportunity)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(opps)
    }
}

pub fn mark_posted(
    conn: &Connection,
    post_id: &str,
    reply_text: &str,
    reply_url: &str,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE reddit_opportunities SET reply_status='posted', reply_text=?1, reply_url=?2, posted_at=?3, updated_at=?4 WHERE post_id=?5",
        params![reply_text, reply_url, now.clone(), now, post_id],
    )?;
    Ok(())
}

pub fn mark_skipped(conn: &Connection, post_id: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE reddit_opportunities SET reply_status='skipped', updated_at=?1 WHERE post_id=?2",
        params![now, post_id],
    )?;
    Ok(())
}

/// Mark all pending rows for a project as 'stale' instead of deleting them.
/// Stale rows are hidden from the default feed but recoverable (a re-discovered
/// post is flipped back to 'pending' by `upsert_opportunity`).
pub fn mark_pending_stale(conn: &Connection, project_id: &str) -> Result<usize> {
    let now = Utc::now().to_rfc3339();
    let n = conn.execute(
        "UPDATE reddit_opportunities SET reply_status='stale', updated_at=?1 WHERE project_id=?2 AND reply_status='pending'",
        params![now, project_id],
    )?;
    Ok(n)
}

/// Persist a user-edited draft reply back to the opportunity row.
pub fn update_reply_text(conn: &Connection, post_id: &str, reply_text: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE reddit_opportunities SET reply_text=?1, updated_at=?2 WHERE post_id=?3",
        params![reply_text, now, post_id],
    )?;
    Ok(())
}

pub fn get_statistics(conn: &Connection, project_id: &str) -> Result<RedditStats> {
    let mut by_status = std::collections::HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT reply_status, COUNT(*) FROM reddit_opportunities WHERE project_id=?1 GROUP BY reply_status",
        )?;
        let iter = stmt.query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for pair in iter {
            let (k, v) = pair?;
            by_status.insert(k, v);
        }
    }

    let mut pending_by_severity = std::collections::HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT severity, COUNT(*) FROM reddit_opportunities WHERE project_id=?1 AND reply_status='pending' GROUP BY severity",
        )?;
        let iter = stmt.query_map(params![project_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        for pair in iter {
            let (k, v) = pair?;
            pending_by_severity.insert(k, v);
        }
    }

    let (avg_score, max_score): (f64, f64) = conn
        .query_row(
            "SELECT COALESCE(AVG(final_score), 0.0), COALESCE(MAX(final_score), 0.0) FROM reddit_opportunities WHERE project_id=?1 AND reply_status='pending'",
            params![project_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap_or((0.0, 0.0));

    let total: i64 = by_status.values().sum();
    Ok(RedditStats {
        total_opportunities: total,
        by_status,
        pending_by_severity,
        average_score: avg_score,
        max_score,
    })
}

/// Import opportunities from the legacy Python `client_ops.db`.
pub fn migrate_from_client_ops(
    conn: &Connection,
    project_id: &str,
    source_path: &std::path::Path,
) -> Result<MigrationResult> {
    let src = rusqlite::Connection::open(source_path)?;

    let table_exists: bool = src
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='reddit_opportunity'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !table_exists {
        return Ok(MigrationResult {
            migrated: 0,
            skipped: 0,
            errors: vec!["Source has no reddit_opportunity table".to_string()],
        });
    }

    let mut stmt = src.prepare(
        "SELECT post_id, title, url, subreddit, author, posted_date, upvotes, comment_count,
                relevance_score, engagement_score, accessibility_score, final_score,
                severity, why_relevant, key_pain_points, website_fit,
                reply_status, reply_text, reply_url, reply_upvotes, reply_replies, posted_at
         FROM reddit_opportunity",
    )?;

    type Row = (
        String,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        Option<String>,
    );

    let rows: Vec<Row> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
                row.get(8)?,
                row.get(9)?,
                row.get(10)?,
                row.get(11)?,
                row.get(12)?,
                row.get(13)?,
                row.get(14)?,
                row.get(15)?,
                row.get(16)?,
                row.get(17)?,
                row.get(18)?,
                row.get(19)?,
                row.get(20)?,
                row.get(21)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let now = Utc::now().to_rfc3339();
    let mut migrated = 0usize;
    let mut skipped = 0usize;
    let mut errors = Vec::new();

    for r in rows {
        let opp = RedditOpportunity {
            post_id: r.0.clone(),
            title: r.1,
            selftext: None,
            url: r.2,
            subreddit: r.3,
            author: r.4,
            posted_date: r.5,
            upvotes: r.6,
            comment_count: r.7,
            relevance_score: r.8,
            engagement_score: r.9,
            accessibility_score: r.10,
            final_score: r.11,
            severity: r.12,
            why_relevant: r.13,
            key_pain_points: r
                .14
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default(),
            website_fit: r.15,
            mention_stance: None,
            product_name: None,
            reply_status: r.16.unwrap_or_else(|| "pending".to_string()),
            reply_text: r.17,
            reply_url: r.18,
            reply_upvotes: r.19,
            reply_replies: r.20,
            posted_at: r.21,
            project_id: project_id.to_string(),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        match upsert_opportunity(conn, &opp) {
            Ok(_) => migrated += 1,
            Err(e) => {
                skipped += 1;
                errors.push(format!("{}: {}", r.0, e));
            }
        }
    }

    Ok(MigrationResult {
        migrated,
        skipped,
        errors,
    })
}

fn row_to_opportunity(row: &rusqlite::Row<'_>) -> rusqlite::Result<RedditOpportunity> {
    let pain_points_raw: Option<String> = row.get("key_pain_points")?;
    let key_pain_points = pain_points_raw
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    Ok(RedditOpportunity {
        post_id: row.get("post_id")?,
        title: row.get("title")?,
        selftext: row.get("selftext").unwrap_or(None),
        url: row.get("url")?,
        subreddit: row.get("subreddit")?,
        author: row.get("author")?,
        posted_date: row.get("posted_date")?,
        upvotes: row.get("upvotes")?,
        comment_count: row.get("comment_count")?,
        relevance_score: row.get("relevance_score")?,
        engagement_score: row.get("engagement_score")?,
        accessibility_score: row.get("accessibility_score")?,
        final_score: row.get("final_score")?,
        severity: row.get("severity")?,
        why_relevant: row.get("why_relevant")?,
        key_pain_points,
        website_fit: row.get("website_fit")?,
        mention_stance: row.get("mention_stance").unwrap_or(None),
        product_name: row.get("product_name").unwrap_or(None),
        reply_status: row.get("reply_status")?,
        reply_text: row.get("reply_text")?,
        reply_url: row.get("reply_url")?,
        reply_upvotes: row.get("reply_upvotes")?,
        reply_replies: row.get("reply_replies")?,
        posted_at: row.get("posted_at")?,
        project_id: row.get("project_id")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}
