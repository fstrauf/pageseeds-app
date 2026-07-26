//! DataForSEO SERP cost guards: keyword+locale cache (TTL) + per-project daily live cap.
//!
//! # NOTE
//! GSC owns post-ship outcomes; DataForSEO SERP is for research/diagnostics only.
//! Defaults: daily live cap 50/project, cache TTL 14 days. Optional global_settings
//! overrides later.
//!
//! Product call sites must use [`fetch_serp_features`] — never call
//! `provider.serp_features` directly (except inside this module and trait impls).

use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;

use crate::content::keyword_match::normalize_keyword;
use crate::error::{Error, Result};
use crate::seo::keywords::SerpFeaturesResult;
use crate::seo::provider::SeoDataProvider;

/// Default max live DataForSEO SERP calls per project per UTC day.
pub const DEFAULT_DAILY_LIVE_CAP: u32 = 50;

/// Default SERP features cache TTL in days (keyword_norm + country).
pub const DEFAULT_CACHE_TTL_DAYS: u64 = 14;

/// Whether the SERP payload came from SQLite cache or a live provider call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SerpSource {
    Cache,
    Live,
}

/// Guarded SERP features lookup result.
#[derive(Debug, Clone)]
pub struct SerpFeaturesLookup {
    pub features: SerpFeaturesResult,
    pub source: SerpSource,
}

/// True when `err` is a daily SERP live-call budget exhaustion signal.
///
/// Callers must soft-degrade (skip winnability / skip score) — never map budget
/// to Avoid / risk 99 via [`crate::seo::dead_weight::serp_error_assessment`].
pub fn is_budget_error(err: &Error) -> bool {
    matches!(err, Error::SerpBudgetExceeded { .. })
}

/// Fetch SERP features with cache + per-project daily live-call cap.
///
/// Order (fixed):
/// 1. Normalize keyword + country
/// 2. Cache hit within TTL → return cached (no network, no counter bump)
/// 3. Cap exceeded for project + UTC today → [`Error::SerpBudgetExceeded`]
/// 4. Live fetch → on success write cache + increment daily live count
pub async fn fetch_serp_features(
    conn: &Connection,
    project_id: &str,
    provider: &dyn SeoDataProvider,
    keyword: &str,
    country: &str,
) -> Result<SerpFeaturesLookup> {
    fetch_serp_features_at(
        conn,
        project_id,
        provider,
        keyword,
        country,
        Utc::now(),
        DEFAULT_DAILY_LIVE_CAP,
        DEFAULT_CACHE_TTL_DAYS,
    )
    .await
}

/// Same as [`fetch_serp_features`] with explicit clock and limits (unit tests).
pub async fn fetch_serp_features_at(
    conn: &Connection,
    project_id: &str,
    provider: &dyn SeoDataProvider,
    keyword: &str,
    country: &str,
    now: DateTime<Utc>,
    daily_cap: u32,
    cache_ttl_days: u64,
) -> Result<SerpFeaturesLookup> {
    let keyword_norm = normalize_keyword(keyword);
    let country_norm = country.trim().to_lowercase();
    if keyword_norm.is_empty() {
        return Err(Error::Validation(
            "SERP features lookup requires a non-empty keyword".to_string(),
        ));
    }

    if let Some(cached) = load_cache(conn, &keyword_norm, &country_norm)? {
        if is_cache_fresh(&cached.fetched_at, cache_ttl_days, now) {
            return Ok(SerpFeaturesLookup {
                features: cached.features,
                source: SerpSource::Cache,
            });
        }
    }

    let day = now.format("%Y-%m-%d").to_string();
    let used = daily_live_calls(conn, project_id, &day)?;
    if used >= daily_cap {
        return Err(Error::SerpBudgetExceeded {
            project_id: project_id.to_string(),
            day,
            cap: daily_cap,
        });
    }

    let features = provider.serp_features(keyword, &country_norm).await?;
    write_cache(conn, &keyword_norm, &country_norm, &features, now)?;
    increment_daily_live(conn, project_id, &day)?;
    log::info!(
        "[serp_guard] live SERP for project={} keyword_norm={} country={} day={} calls={}/{}",
        project_id,
        keyword_norm,
        country_norm,
        day,
        used + 1,
        daily_cap
    );

    Ok(SerpFeaturesLookup {
        features,
        source: SerpSource::Live,
    })
}

// ─── Cache / usage helpers ───────────────────────────────────────────────────

struct CachedSerp {
    features: SerpFeaturesResult,
    fetched_at: String,
}

fn is_cache_fresh(fetched_at: &str, ttl_days: u64, now: DateTime<Utc>) -> bool {
    let Ok(parsed) = DateTime::parse_from_rfc3339(fetched_at) else {
        return false;
    };
    let fetched = parsed.with_timezone(&Utc);
    if fetched > now {
        return true;
    }
    now - fetched <= Duration::days(ttl_days as i64)
}

fn load_cache(
    conn: &Connection,
    keyword_norm: &str,
    country: &str,
) -> Result<Option<CachedSerp>> {
    let mut stmt = conn.prepare(
        "SELECT payload_json, fetched_at FROM serp_features_cache
         WHERE keyword_norm = ?1 AND country = ?2",
    )?;
    let mut rows = stmt.query(rusqlite::params![keyword_norm, country])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };
    let payload: String = row.get(0)?;
    let fetched_at: String = row.get(1)?;
    let features: SerpFeaturesResult = serde_json::from_str(&payload)?;
    Ok(Some(CachedSerp {
        features,
        fetched_at,
    }))
}

fn write_cache(
    conn: &Connection,
    keyword_norm: &str,
    country: &str,
    features: &SerpFeaturesResult,
    now: DateTime<Utc>,
) -> Result<()> {
    let payload = serde_json::to_string(features)?;
    conn.execute(
        "INSERT INTO serp_features_cache (keyword_norm, country, payload_json, fetched_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(keyword_norm, country) DO UPDATE SET
           payload_json = excluded.payload_json,
           fetched_at = excluded.fetched_at",
        rusqlite::params![keyword_norm, country, payload, now.to_rfc3339()],
    )?;
    Ok(())
}

fn daily_live_calls(conn: &Connection, project_id: &str, day: &str) -> Result<u32> {
    let mut stmt = conn.prepare(
        "SELECT live_calls FROM serp_daily_usage WHERE project_id = ?1 AND day = ?2",
    )?;
    let mut rows = stmt.query(rusqlite::params![project_id, day])?;
    match rows.next()? {
        Some(row) => {
            let n: i64 = row.get(0)?;
            Ok(n.max(0) as u32)
        }
        None => Ok(0),
    }
}

fn increment_daily_live(conn: &Connection, project_id: &str, day: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO serp_daily_usage (project_id, day, live_calls)
         VALUES (?1, ?2, 1)
         ON CONFLICT(project_id, day) DO UPDATE SET
           live_calls = live_calls + 1",
        rusqlite::params![project_id, day],
    )?;
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seo::intent::IntentClassification;
    use crate::seo::keywords::{KeywordDifficultyResult, KeywordIdeasResult};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct MockSerpProvider {
        calls: Arc<AtomicUsize>,
        fail: bool,
    }

    impl MockSerpProvider {
        fn new(calls: Arc<AtomicUsize>) -> Self {
            Self {
                calls,
                fail: false,
            }
        }
    }

    #[async_trait]
    impl SeoDataProvider for MockSerpProvider {
        async fn keyword_ideas(
            &self,
            _keyword: &str,
            _country: &str,
            _search_engine: &str,
        ) -> Result<KeywordIdeasResult> {
            Err(Error::Other("stub".into()))
        }

        async fn keyword_difficulty(
            &self,
            _keyword: &str,
            _country: &str,
        ) -> Result<KeywordDifficultyResult> {
            Err(Error::Other("stub".into()))
        }

        async fn batch_keyword_difficulty(
            &self,
            _keywords: &[String],
            _country: &str,
        ) -> Result<Vec<KeywordDifficultyResult>> {
            Err(Error::Other("stub".into()))
        }

        async fn search_intent(
            &self,
            _keywords: &[String],
        ) -> Result<Vec<IntentClassification>> {
            Err(Error::Other("stub".into()))
        }

        async fn serp_features(
            &self,
            keyword: &str,
            _country: &str,
        ) -> Result<SerpFeaturesResult> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                return Err(Error::Other("provider down".into()));
            }
            Ok(SerpFeaturesResult {
                keyword: keyword.to_string(),
                ai_overview_present: false,
                featured_snippet_present: true,
                people_also_ask_present: false,
                organic_results: vec![],
            })
        }

        fn name(&self) -> &'static str {
            "mock"
        }
    }

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES ('proj1', 'Test', '/tmp', 1)",
            [],
        )
        .unwrap();
        conn
    }

    #[tokio::test]
    async fn cache_hit_does_not_call_provider() {
        let conn = test_db();
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = MockSerpProvider::new(calls.clone());
        let now = Utc::now();

        let first = fetch_serp_features_at(
            &conn,
            "proj1",
            &provider,
            "Theta Decay",
            "US",
            now,
            50,
            14,
        )
        .await
        .unwrap();
        assert_eq!(first.source, SerpSource::Live);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(first.features.featured_snippet_present);

        let second = fetch_serp_features_at(
            &conn,
            "proj1",
            &provider,
            "  theta   decay  ",
            "us",
            now + Duration::hours(1),
            50,
            14,
        )
        .await
        .unwrap();
        assert_eq!(second.source, SerpSource::Cache);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "cache hit must not call provider"
        );
        assert_eq!(
            daily_live_calls(&conn, "proj1", &now.format("%Y-%m-%d").to_string()).unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn cap_exceeded_returns_budget_error_without_provider_call() {
        let conn = test_db();
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = MockSerpProvider::new(calls.clone());
        let now = Utc::now();
        let day = now.format("%Y-%m-%d").to_string();

        // Pre-fill daily usage to the cap.
        conn.execute(
            "INSERT INTO serp_daily_usage (project_id, day, live_calls) VALUES ('proj1', ?1, 2)",
            rusqlite::params![day],
        )
        .unwrap();

        let err = fetch_serp_features_at(
            &conn,
            "proj1",
            &provider,
            "unique keyword never cached",
            "us",
            now,
            2, // cap already reached
            14,
        )
        .await
        .unwrap_err();

        assert!(is_budget_error(&err));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        match err {
            Error::SerpBudgetExceeded { cap, .. } => assert_eq!(cap, 2),
            other => panic!("expected SerpBudgetExceeded, got {other}"),
        }
    }

    #[tokio::test]
    async fn under_cap_live_then_second_keyword_uses_cache() {
        let conn = test_db();
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = MockSerpProvider::new(calls.clone());
        let now = Utc::now();

        let a = fetch_serp_features_at(
            &conn,
            "proj1",
            &provider,
            "wheel strategy",
            "us",
            now,
            50,
            14,
        )
        .await
        .unwrap();
        assert_eq!(a.source, SerpSource::Live);

        let b = fetch_serp_features_at(
            &conn,
            "proj1",
            &provider,
            "wheel strategy",
            "us",
            now,
            50,
            14,
        )
        .await
        .unwrap();
        assert_eq!(b.source, SerpSource::Cache);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn next_utc_day_resets_cap() {
        let conn = test_db();
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = MockSerpProvider::new(calls.clone());
        let day1 = DateTime::parse_from_rfc3339("2026-07-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let day2 = DateTime::parse_from_rfc3339("2026-07-02T01:00:00Z")
            .unwrap()
            .with_timezone(&Utc);

        // Cap of 1 on day1.
        let first = fetch_serp_features_at(
            &conn,
            "proj1",
            &provider,
            "day one keyword",
            "us",
            day1,
            1,
            14,
        )
        .await
        .unwrap();
        assert_eq!(first.source, SerpSource::Live);

        let blocked = fetch_serp_features_at(
            &conn,
            "proj1",
            &provider,
            "day one other",
            "us",
            day1,
            1,
            14,
        )
        .await
        .unwrap_err();
        assert!(is_budget_error(&blocked));

        // New UTC day allows another live call (different keyword, empty cache).
        let next = fetch_serp_features_at(
            &conn,
            "proj1",
            &provider,
            "day two keyword",
            "us",
            day2,
            1,
            14,
        )
        .await
        .unwrap();
        assert_eq!(next.source, SerpSource::Live);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn is_budget_error_only_matches_serp_budget() {
        assert!(is_budget_error(&Error::SerpBudgetExceeded {
            project_id: "p".into(),
            day: "2026-01-01".into(),
            cap: 50,
        }));
        assert!(!is_budget_error(&Error::Other("provider down".into())));
        assert!(!is_budget_error(&Error::Validation("x".into())));
    }
}
