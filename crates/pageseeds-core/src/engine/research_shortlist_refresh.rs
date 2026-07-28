//! Research shortlist side effects for Path B `research-context` (issues #192 / #258).
//!
//! Owns territory refresh and strategy re-annotation writes. Pure strategy
//! package reads stay in [`super::research_package::build_research_strategy_package`].

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::db::research_shortlist;

/// Default max age for territory-sourced shortlist rows before refresh (issue #192).
pub const RESEARCH_SHORTLIST_MAX_AGE_DAYS: i64 = 7;

/// Stable reason strings for shortlist refresh (issue #192).
pub mod shortlist_refresh_reason {
    pub const EMPTY: &str = "empty";
    pub const STALE: &str = "stale";
    pub const SKIPPED_FRESH: &str = "skipped_fresh";
    pub const FAILED: &str = "failed";
}

/// Result of [`ensure_research_shortlist_fresh`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortlistRefreshResult {
    /// True when territory analysis was invoked (even if it synced 0 rows).
    pub shortlist_refreshed: bool,
    /// Stable: `empty` | `stale` | `skipped_fresh` | `failed`.
    pub shortlist_refresh_reason: String,
    /// Territory diagnostics when a refresh ran (or failed after attempt).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub territory: Option<serde_json::Value>,
    /// Error message when reason is `failed`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Ensure `research_shortlist` is filled via territory analysis when empty or stale.
///
/// Heuristic (no dedicated sync table):
/// - **empty:** zero rows for `project_id` → run territory
/// - **stale:** no fresh `source='territory_analysis'` rows within `max_age_days` → run
/// - **fresh:** non-empty territory rows within max age → skip
///
/// Side effects only happen here (and in territory upsert). Prefer
/// [`super::research_package::build_research_context`] for the full CLI envelope.
pub fn ensure_research_shortlist_fresh(
    conn: &Connection,
    project_id: &str,
    max_age_days: i64,
) -> ShortlistRefreshResult {
    if project_id.trim().is_empty() {
        return ShortlistRefreshResult {
            shortlist_refreshed: false,
            shortlist_refresh_reason: shortlist_refresh_reason::FAILED.to_string(),
            territory: None,
            error: Some("project_id is required".to_string()),
        };
    }

    let reason = match shortlist_freshness_reason(conn, project_id, max_age_days) {
        Ok(r) => r,
        Err(e) => {
            return ShortlistRefreshResult {
                shortlist_refreshed: false,
                shortlist_refresh_reason: shortlist_refresh_reason::FAILED.to_string(),
                territory: None,
                error: Some(e),
            };
        }
    };

    if reason == shortlist_refresh_reason::SKIPPED_FRESH {
        return ShortlistRefreshResult {
            shortlist_refreshed: false,
            shortlist_refresh_reason: reason.to_string(),
            territory: None,
            error: None,
        };
    }

    match crate::engine::exec::keywords::run_territory_analysis(conn, project_id) {
        Ok(diag) => ShortlistRefreshResult {
            shortlist_refreshed: true,
            shortlist_refresh_reason: reason.to_string(),
            territory: Some(diag.to_output_json()),
            error: None,
        },
        Err(e) => ShortlistRefreshResult {
            shortlist_refreshed: false,
            shortlist_refresh_reason: shortlist_refresh_reason::FAILED.to_string(),
            territory: None,
            error: Some(e.to_string()),
        },
    }
}

/// Decide whether the shortlist needs a territory refresh.
///
/// Returns one of: `empty`, `stale`, `skipped_fresh`.
fn shortlist_freshness_reason(
    conn: &Connection,
    project_id: &str,
    max_age_days: i64,
) -> Result<&'static str, String> {
    let count = research_shortlist::count_entries(conn, project_id).map_err(|e| e.to_string())?;
    if count == 0 {
        return Ok(shortlist_refresh_reason::EMPTY);
    }

    let max_added = research_shortlist::max_territory_added_at(conn, project_id)
        .map_err(|e| e.to_string())?;

    match max_added {
        Some(ts) if territory_added_at_is_fresh(&ts, max_age_days) => {
            Ok(shortlist_refresh_reason::SKIPPED_FRESH)
        }
        Some(_) => Ok(shortlist_refresh_reason::STALE),
        // Rows exist but none from territory_analysis → filler never ran.
        None => Ok(shortlist_refresh_reason::STALE),
    }
}

fn territory_added_at_is_fresh(added_at: &str, max_age_days: i64) -> bool {
    use chrono::{DateTime, Duration, Utc};
    match DateTime::parse_from_rfc3339(added_at) {
        Ok(dt) => {
            let age = Utc::now().signed_duration_since(dt.with_timezone(&Utc));
            age <= Duration::days(max_age_days)
        }
        // Unparseable timestamp → treat as stale so ensure re-runs.
        Err(_) => false,
    }
}

/// Re-annotate `strategy_cluster` / `strategy_status` on existing shortlist rows
/// from live `project.md` strategy (issue #258 approach A).
///
/// Cheap: no territory re-run. Empty/missing strategy is a no-op (leaves columns).
/// Returns the number of rows whose annotation changed.
///
/// Called from [`super::research_package::build_research_context`] after ensure
/// so strategy edits surface without waiting for the territory TTL.
pub fn reannotate_shortlist_strategy(
    conn: &Connection,
    project_id: &str,
) -> Result<usize, String> {
    if project_id.trim().is_empty() {
        return Ok(0);
    }
    let strategy = crate::strategy::load_for_project(conn, project_id);
    if strategy.is_empty() {
        return Ok(0);
    }

    let entries = research_shortlist::list_entries(conn, project_id, None)
        .map_err(|e| e.to_string())?;
    let mut updated = 0usize;

    for entry in entries {
        let Some(id) = entry.id else { continue };
        let (new_cluster, new_status) = match crate::strategy::match_cluster(&strategy, &entry.theme)
        {
            Some((name, status)) => (Some(name.to_string()), Some(status.as_str().to_string())),
            None => (None, None),
        };
        if entry.strategy_cluster == new_cluster && entry.strategy_status == new_status {
            continue;
        }
        research_shortlist::update_strategy_annotation(
            conn,
            id,
            new_cluster.as_deref(),
            new_status.as_deref(),
        )
        .map_err(|e| e.to_string())?;
        updated += 1;
    }

    if updated > 0 {
        log::info!(
            "[research_shortlist_refresh] reannotated {} shortlist row(s) with live content_strategy for project {}",
            updated,
            project_id
        );
    }
    Ok(updated)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::research_package::build_research_strategy_package;

    /// Full schema so territory analysis can load articles + GSC tape.
    fn ensure_fixture_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn
    }

    fn insert_ensure_project(conn: &Connection, id: &str) {
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES (?1, 'Test', '/tmp/research-package-test', 1, 'workspace')",
            rusqlite::params![id],
        )
        .unwrap();
    }

    fn insert_ensure_article(
        conn: &Connection,
        project_id: &str,
        id: i64,
        slug: &str,
        keyword: &str,
    ) {
        conn.execute(
            "INSERT INTO articles (
                id, project_id, title, url_slug, file, status, target_keyword,
                content_gaps_addressed, target_volume, word_count, review_count, content_hash
             ) VALUES (?1, ?2, ?3, ?4, ?5, 'published', ?6, '[]', 0, 100, 0, 'hash')",
            rusqlite::params![
                id,
                project_id,
                format!("Title {id}"),
                slug,
                format!("content/{slug}.mdx"),
                keyword
            ],
        )
        .unwrap();
    }

    fn insert_gsc_for_slugs(conn: &Connection, project_id: &str, rows: &[(&str, f64)]) {
        use chrono::{Duration, Utc};
        let end = Utc::now().date_naive() - Duration::days(1);
        let d1 = (end - Duration::days(2)).format("%Y-%m-%d").to_string();
        let metrics: Vec<_> = rows
            .iter()
            .map(|(slug, imp)| crate::models::gsc::PageDailyMetrics {
                page: format!("https://example.com/blog/{slug}"),
                date: d1.clone(),
                clicks: 1.0,
                impressions: *imp,
                ctr: 0.01,
                position: 8.0,
            })
            .collect();
        crate::db::insert_gsc_page_daily_snapshots(conn, project_id, &metrics).unwrap();
    }

    #[test]
    fn ensure_empty_shortlist_runs_territory_and_surfaces_diagnostics() {
        let conn = ensure_fixture_db();
        insert_ensure_project(&conn, "proj1");
        // Mid-coverage (3 articles) so territory always has something to sync
        // without needing the 5k open-territory impression bar.
        insert_ensure_article(&conn, "proj1", 1, "mid-a", "mid theme");
        insert_ensure_article(&conn, "proj1", 2, "mid-b", "mid theme");
        insert_ensure_article(&conn, "proj1", 3, "mid-c", "mid theme");
        insert_gsc_for_slugs(
            &conn,
            "proj1",
            &[("mid-a", 100.0), ("mid-b", 150.0), ("mid-c", 200.0)],
        );

        assert_eq!(research_shortlist::count_entries(&conn, "proj1").unwrap(), 0);

        let refresh = ensure_research_shortlist_fresh(
            &conn,
            "proj1",
            RESEARCH_SHORTLIST_MAX_AGE_DAYS,
        );
        assert!(refresh.shortlist_refreshed, "empty shortlist must refresh");
        assert_eq!(
            refresh.shortlist_refresh_reason,
            shortlist_refresh_reason::EMPTY
        );
        assert!(refresh.error.is_none());
        let territory = refresh.territory.expect("territory diagnostics required");
        // Either rows synced, or honest skip_reasons — never silent mystery empty.
        let synced = territory["synced_to_shortlist"].as_u64().unwrap_or(0);
        let skip = territory["skip_reasons"]
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        assert!(
            synced > 0 || skip > 0,
            "refresh must leave rows or skip_reasons; got {territory}"
        );
        if synced > 0 {
            assert!(
                research_shortlist::count_entries(&conn, "proj1").unwrap() > 0,
                "synced themes must land in research_shortlist"
            );
        }

        // Pure build still works after ensure.
        let pkg = build_research_strategy_package(&conn, "proj1").unwrap();
        if synced > 0 {
            assert!(!pkg.shortlist.is_empty());
        }
    }

    #[test]
    fn ensure_empty_project_still_returns_skip_reasons() {
        // No articles → territory runs, syncs 0, skip_reasons non-empty.
        let conn = ensure_fixture_db();
        insert_ensure_project(&conn, "proj1");

        let refresh = ensure_research_shortlist_fresh(
            &conn,
            "proj1",
            RESEARCH_SHORTLIST_MAX_AGE_DAYS,
        );
        assert!(refresh.shortlist_refreshed);
        assert_eq!(
            refresh.shortlist_refresh_reason,
            shortlist_refresh_reason::EMPTY
        );
        let territory = refresh.territory.expect("diagnostics");
        let reasons = territory["skip_reasons"]
            .as_array()
            .expect("skip_reasons array");
        assert!(
            !reasons.is_empty(),
            "zero-theme refresh must not be silent empty"
        );
        assert_eq!(territory["synced_to_shortlist"], 0);
    }

    #[test]
    fn ensure_fresh_territory_shortlist_skips() {
        let conn = ensure_fixture_db();
        insert_ensure_project(&conn, "proj1");
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, health_status, added_at)
             VALUES ('proj1', 'existing theme', '[]', 'territory_analysis', 'pending', 'high', 'unproven', ?1)",
            rusqlite::params![now],
        )
        .unwrap();

        let refresh = ensure_research_shortlist_fresh(
            &conn,
            "proj1",
            RESEARCH_SHORTLIST_MAX_AGE_DAYS,
        );
        assert!(!refresh.shortlist_refreshed);
        assert_eq!(
            refresh.shortlist_refresh_reason,
            shortlist_refresh_reason::SKIPPED_FRESH
        );
        assert!(refresh.territory.is_none());
        assert_eq!(research_shortlist::count_entries(&conn, "proj1").unwrap(), 1);
    }

    #[test]
    fn ensure_stale_territory_shortlist_refreshes() {
        let conn = ensure_fixture_db();
        insert_ensure_project(&conn, "proj1");
        // Old territory row (> 7 days).
        let old = (chrono::Utc::now() - chrono::Duration::days(14)).to_rfc3339();
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, health_status, added_at)
             VALUES ('proj1', 'old theme', '[]', 'territory_analysis', 'pending', 'medium', 'unproven', ?1)",
            rusqlite::params![old],
        )
        .unwrap();
        insert_ensure_article(&conn, "proj1", 1, "mid-a", "fresh mid");
        insert_ensure_article(&conn, "proj1", 2, "mid-b", "fresh mid");
        insert_ensure_article(&conn, "proj1", 3, "mid-c", "fresh mid");
        insert_gsc_for_slugs(
            &conn,
            "proj1",
            &[("mid-a", 50.0), ("mid-b", 50.0), ("mid-c", 50.0)],
        );

        let refresh = ensure_research_shortlist_fresh(
            &conn,
            "proj1",
            RESEARCH_SHORTLIST_MAX_AGE_DAYS,
        );
        assert!(refresh.shortlist_refreshed);
        assert_eq!(
            refresh.shortlist_refresh_reason,
            shortlist_refresh_reason::STALE
        );
        assert!(refresh.territory.is_some());
    }

    #[test]
    fn ensure_failed_on_empty_project_id() {
        let conn = ensure_fixture_db();
        let refresh = ensure_research_shortlist_fresh(&conn, "", 7);
        assert!(!refresh.shortlist_refreshed);
        assert_eq!(
            refresh.shortlist_refresh_reason,
            shortlist_refresh_reason::FAILED
        );
        assert!(refresh.error.is_some());
    }

    /// Full schema + temp project.md so strategy re-annotate can resolve path.
    fn strategy_fixture_db(project_md: &str) -> (Connection, std::path::PathBuf) {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "pageseeds-shortlist-reannotate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let automation = dir.join(".github").join("automation");
        std::fs::create_dir_all(&automation).unwrap();
        std::fs::write(automation.join("project.md"), project_md).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('proj1', 'Test', ?1, 1, 'workspace')",
            rusqlite::params![dir.to_string_lossy()],
        )
        .unwrap();
        (conn, dir)
    }

    #[test]
    fn reannotate_shortlist_strategy_updates_stale_columns_without_ttl() {
        let (conn, dir) = strategy_fixture_db(
            r#"# Test

## Content Clusters

### Cluster 1: SEO Fundamentals (ACTIVE)
- technical seo

### Cluster 2: Old Services (LEGACY)
- web design packages
"#,
        );
        // Stale annotation as if strategy was edited after territory write.
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, health_status,
              strategy_cluster, strategy_status, added_at)
             VALUES ('proj1', 'technical seo', '[]', 'territory_analysis', 'pending', 'high',
                     'unproven', NULL, NULL, ?1)",
            rusqlite::params![chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, health_status,
              strategy_cluster, strategy_status, added_at)
             VALUES ('proj1', 'web design packages', '[]', 'territory_analysis', 'pending', 'medium',
                     'unproven', 'Old Name', 'active', ?1)",
            rusqlite::params![chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();

        let n = reannotate_shortlist_strategy(&conn, "proj1").unwrap();
        assert_eq!(n, 2);

        let rows = research_shortlist::list_entries(&conn, "proj1", None).unwrap();
        let by = |t: &str| rows.iter().find(|e| e.theme == t).unwrap();
        assert_eq!(
            by("technical seo").strategy_cluster.as_deref(),
            Some("SEO Fundamentals")
        );
        assert_eq!(by("technical seo").strategy_status.as_deref(), Some("active"));
        assert_eq!(
            by("web design packages").strategy_cluster.as_deref(),
            Some("Old Services")
        );
        assert_eq!(
            by("web design packages").strategy_status.as_deref(),
            Some("legacy")
        );

        // Idempotent second pass.
        assert_eq!(reannotate_shortlist_strategy(&conn, "proj1").unwrap(), 0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reannotate_empty_strategy_is_noop() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, active, project_mode)
             VALUES ('proj1', 'Test', '/tmp/no-project-md-here', 1, 'workspace')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO research_shortlist
             (project_id, theme, seeds, source, status, priority, health_status,
              strategy_cluster, strategy_status, added_at)
             VALUES ('proj1', 'theme', '[]', 'test', 'pending', 'medium', 'unproven',
                     'Keep Me', 'active', ?1)",
            rusqlite::params![chrono::Utc::now().to_rfc3339()],
        )
        .unwrap();

        assert_eq!(reannotate_shortlist_strategy(&conn, "proj1").unwrap(), 0);
        let rows = research_shortlist::list_entries(&conn, "proj1", None).unwrap();
        assert_eq!(rows[0].strategy_cluster.as_deref(), Some("Keep Me"));
        assert_eq!(rows[0].strategy_status.as_deref(), Some("active"));
    }
}
