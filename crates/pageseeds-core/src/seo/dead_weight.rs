//! Dead-weight remediation: score low/zero-impression articles for winnability,
//! persist results to `article_metadata` (namespace `winnability`), and list from
//! cache without SEO provider calls.
//!
//! Human-gated only — no auto bulk noindex, no new task type, no migrations.
//! Live SERP scoring is capped and TTL-gated so weekly runs stay cheap.

use chrono::{DateTime, Duration, Utc};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::seo::provider::SeoDataProvider;
use crate::seo::serp_guard::{fetch_serp_features, is_budget_error, SerpSource};
use crate::seo::winnability::{assess, WinnabilityAssessment, WinnabilityBucket};

// ─── Constants ───────────────────────────────────────────────────────────────

/// `article_metadata` namespace for stored winnability scores.
pub const WINNABILITY_NAMESPACE: &str = "winnability";

/// Default local TTL for cached scores (days). Fresh scores are not re-fetched
/// on a default score run unless `--force`.
pub const DEFAULT_SCORE_TTL_DAYS: u64 = 60;

/// Default max live SERP assessments per score run (prevents unbounded fan-out).
pub const DEFAULT_MAX_LIVE_SCORES: usize = 25;

/// Default GSC impressions ceiling for "low/zero impression" candidates.
pub const DEFAULT_MAX_IMPRESSIONS: f64 = 10.0;

/// Provenance string written into stored scores from this scorer path.
pub const SCORE_SOURCE: &str = "score_zero_impression_articles";

// ─── Types ───────────────────────────────────────────────────────────────────

/// Persisted winnability score under `article_metadata` namespace `winnability`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredWinnabilityScore {
    pub keyword: String,
    pub bucket: String,
    pub risk_score: u32,
    pub reason: String,
    pub ai_overview_present: bool,
    pub featured_snippet_present: bool,
    pub authority_competitors: Vec<String>,
    /// RFC3339 timestamp when this score was written.
    pub scored_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impressions_at_score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Published article with low/zero GSC impressions and a target keyword.
#[derive(Debug, Clone)]
pub struct DeadWeightCandidate {
    pub article_id: i64,
    pub title: String,
    pub slug: String,
    pub target_keyword: String,
    pub keyword_difficulty: Option<String>,
    pub impressions: f64,
}

/// Options for a live score run.
#[derive(Debug, Clone)]
pub struct ScoreOptions {
    /// Max GSC impressions to include a published article (default 10).
    pub max_impressions: f64,
    /// Re-score even when a fresh cached score exists.
    pub force: bool,
    /// Local metadata TTL in days (default 60).
    pub ttl_days: u64,
    /// Max live SERP assessments this run (default 25).
    pub max_live: usize,
    /// SERP country code (default "us").
    pub country: String,
}

impl Default for ScoreOptions {
    fn default() -> Self {
        Self {
            max_impressions: DEFAULT_MAX_IMPRESSIONS,
            force: false,
            ttl_days: DEFAULT_SCORE_TTL_DAYS,
            max_live: DEFAULT_MAX_LIVE_SCORES,
            country: "us".to_string(),
        }
    }
}

/// One article entry in score-run / cache-list JSON.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoredArticleEntry {
    pub article_id: i64,
    pub title: String,
    pub slug: String,
    pub target_keyword: String,
    pub bucket: String,
    pub risk_score: u32,
    pub reason: String,
    pub ai_overview_present: bool,
    pub featured_snippet_present: bool,
    pub authority_competitors: Vec<String>,
    pub scored_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub impressions_at_score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

/// Bucket group in the score-run result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BucketGroup {
    pub count: usize,
    pub articles: Vec<ScoredArticleEntry>,
}

/// Summary JSON for score / list operations (no provider calls when `from_cache`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScoreRunResult {
    pub scored: usize,
    pub skipped_fresh: usize,
    pub skipped_cap: usize,
    /// Successful SERP lookups that hit `serp_features_cache` (no network).
    #[serde(default)]
    pub cache_hits: usize,
    /// Successful SERP lookups that paid for a live DataForSEO call.
    #[serde(default)]
    pub live_calls: usize,
    /// Candidates skipped because the per-project daily SERP budget was hit.
    /// These are **not** persisted as Avoid.
    #[serde(default)]
    pub skipped_budget: usize,
    pub truncated: bool,
    pub ttl_days: u64,
    pub max_live: usize,
    pub from_cache: bool,
    pub avoid: BucketGroup,
    pub differentiate: BucketGroup,
    pub target: BucketGroup,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl ScoreRunResult {
    fn empty(from_cache: bool, ttl_days: u64, max_live: usize, message: Option<String>) -> Self {
        Self {
            scored: 0,
            skipped_fresh: 0,
            skipped_cap: 0,
            cache_hits: 0,
            live_calls: 0,
            skipped_budget: 0,
            truncated: false,
            ttl_days,
            max_live,
            from_cache,
            avoid: BucketGroup::default(),
            differentiate: BucketGroup::default(),
            target: BucketGroup::default(),
            message,
        }
    }

    fn push_entry(&mut self, mut entry: ScoredArticleEntry) {
        match entry.bucket.as_str() {
            "avoid" => {
                self.avoid.articles.push(entry);
                self.avoid.count = self.avoid.articles.len();
            }
            "differentiate" => {
                self.differentiate.articles.push(entry);
                self.differentiate.count = self.differentiate.articles.len();
            }
            "target" => {
                self.target.articles.push(entry);
                self.target.count = self.target.articles.len();
            }
            other => {
                // Unknown / future buckets still surface under avoid so
                // operators never lose the row.
                let other = other.to_string();
                entry.reason = format!("[unknown bucket `{other}`] {}", entry.reason);
                entry.bucket = "avoid".to_string();
                self.avoid.articles.push(entry);
                self.avoid.count = self.avoid.articles.len();
            }
        }
    }
}

// ─── Load candidates ─────────────────────────────────────────────────────────

/// Load published articles with GSC impressions ≤ `max_impressions` (or no GSC
/// metadata) that have a non-empty target keyword.
pub fn load_low_impression_candidates(
    conn: &Connection,
    project_id: &str,
    max_impressions: f64,
) -> Result<Vec<DeadWeightCandidate>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.title, a.url_slug, a.target_keyword, a.keyword_difficulty,
                COALESCE(json_extract(m.payload, '$.impressions'), 0) as impressions
         FROM articles a
         LEFT JOIN article_metadata m
           ON m.project_id = a.project_id
          AND m.article_id = a.id
          AND m.namespace = 'gsc'
         WHERE a.project_id = ?1
           AND a.status = 'published'
           AND (m.article_id IS NULL OR json_extract(m.payload, '$.impressions') <= ?2)
         ORDER BY a.id",
    )?;

    let max_imp_str = max_impressions.to_string();
    let rows = stmt.query_map(rusqlite::params![project_id, max_imp_str.as_str()], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, f64>(5)?,
        ))
    })?;

    let mut out = Vec::new();
    for row in rows {
        let (id, title, slug, kw, kd, impressions) = row?;
        let keyword = kw.unwrap_or_default();
        if keyword.trim().is_empty() {
            continue;
        }
        if impressions > max_impressions {
            continue;
        }
        out.push(DeadWeightCandidate {
            article_id: id,
            title,
            slug,
            target_keyword: keyword,
            keyword_difficulty: kd,
            impressions,
        });
    }
    Ok(out)
}

// ─── Freshness / load / persist ──────────────────────────────────────────────

/// True when `scored_at` parses as RFC3339 and is within `ttl_days` of `now`.
pub fn is_score_fresh(stored: &StoredWinnabilityScore, ttl_days: u64, now: DateTime<Utc>) -> bool {
    let Ok(scored_at) = DateTime::parse_from_rfc3339(&stored.scored_at) else {
        return false;
    };
    let scored_at = scored_at.with_timezone(&Utc);
    if scored_at > now {
        // Future timestamps treat as fresh (clock skew).
        return true;
    }
    let age = now - scored_at;
    age <= Duration::days(ttl_days as i64)
}

/// Load a stored winnability score for one article, if present and parseable.
pub fn load_stored_score(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
) -> Result<Option<StoredWinnabilityScore>> {
    let Some(payload) =
        crate::db::get_article_metadata(conn, project_id, article_id, WINNABILITY_NAMESPACE)?
    else {
        return Ok(None);
    };
    match serde_json::from_str::<StoredWinnabilityScore>(&payload) {
        Ok(score) => Ok(Some(score)),
        Err(e) => {
            log::warn!(
                "[dead_weight] corrupt winnability payload for article {}: {}",
                article_id,
                e
            );
            Ok(None)
        }
    }
}

/// Persist a winnability assessment under namespace `winnability`.
pub fn persist_score(
    conn: &Connection,
    project_id: &str,
    article_id: i64,
    assessment: &WinnabilityAssessment,
    impressions: f64,
    now: DateTime<Utc>,
) -> Result<()> {
    let stored = StoredWinnabilityScore {
        keyword: assessment.keyword.clone(),
        bucket: assessment.bucket.as_str().to_string(),
        risk_score: assessment.risk_score,
        reason: assessment.reason.clone(),
        ai_overview_present: assessment.ai_overview_present,
        featured_snippet_present: assessment.featured_snippet_present,
        authority_competitors: assessment.authority_competitors.clone(),
        scored_at: now.to_rfc3339(),
        impressions_at_score: Some(impressions),
        source: Some(SCORE_SOURCE.to_string()),
    };
    let payload = serde_json::to_string(&stored)?;
    crate::db::set_article_metadata(
        conn,
        project_id,
        article_id,
        WINNABILITY_NAMESPACE,
        &payload,
    )?;
    Ok(())
}

fn entry_from_stored(
    article_id: i64,
    title: &str,
    slug: &str,
    stored: &StoredWinnabilityScore,
) -> ScoredArticleEntry {
    ScoredArticleEntry {
        article_id,
        title: title.to_string(),
        slug: slug.to_string(),
        target_keyword: stored.keyword.clone(),
        bucket: stored.bucket.clone(),
        risk_score: stored.risk_score,
        reason: stored.reason.clone(),
        ai_overview_present: stored.ai_overview_present,
        featured_snippet_present: stored.featured_snippet_present,
        authority_competitors: stored.authority_competitors.clone(),
        scored_at: stored.scored_at.clone(),
        impressions_at_score: stored.impressions_at_score,
        source: stored.source.clone(),
    }
}

fn entry_from_assessment(
    candidate: &DeadWeightCandidate,
    assessment: &WinnabilityAssessment,
    scored_at: &str,
    impressions: f64,
) -> ScoredArticleEntry {
    ScoredArticleEntry {
        article_id: candidate.article_id,
        title: candidate.title.clone(),
        slug: candidate.slug.clone(),
        target_keyword: assessment.keyword.clone(),
        bucket: assessment.bucket.as_str().to_string(),
        risk_score: assessment.risk_score,
        reason: assessment.reason.clone(),
        ai_overview_present: assessment.ai_overview_present,
        featured_snippet_present: assessment.featured_snippet_present,
        authority_competitors: assessment.authority_competitors.clone(),
        scored_at: scored_at.to_string(),
        impressions_at_score: Some(impressions),
        source: Some(SCORE_SOURCE.to_string()),
    }
}

// ─── List from cache ─────────────────────────────────────────────────────────

/// List last stored winnability scores for the project with **zero** SEO
/// provider calls. Groups by bucket.
pub fn list_from_cache(conn: &Connection, project_id: &str) -> Result<ScoreRunResult> {
    let mut stmt = conn.prepare(
        "SELECT m.article_id, m.payload, a.title, a.url_slug
         FROM article_metadata m
         INNER JOIN articles a
           ON a.project_id = m.project_id AND a.id = m.article_id
         WHERE m.project_id = ?1 AND m.namespace = ?2
         ORDER BY m.article_id",
    )?;

    let rows = stmt.query_map(
        rusqlite::params![project_id, WINNABILITY_NAMESPACE],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )?;

    let mut result = ScoreRunResult::empty(
        true,
        DEFAULT_SCORE_TTL_DAYS,
        DEFAULT_MAX_LIVE_SCORES,
        None,
    );

    for row in rows {
        let (article_id, payload, title, slug) = row?;
        let stored: StoredWinnabilityScore = match serde_json::from_str(&payload) {
            Ok(s) => s,
            Err(e) => {
                log::warn!(
                    "[dead_weight] skip corrupt winnability row article {}: {}",
                    article_id,
                    e
                );
                continue;
            }
        };
        result.push_entry(entry_from_stored(article_id, &title, &slug, &stored));
    }

    if result.avoid.count + result.differentiate.count + result.target.count == 0 {
        result.message = Some("no cached winnability scores found".to_string());
    }

    Ok(result)
}

// ─── Score + persist ─────────────────────────────────────────────────────────

/// Outcome of assessing one candidate that needs a (re)score.
///
/// Shared by the injected-assessor path and the live provider path so inventory
/// rules (freshness / force / max_live / stale-cache surface / persist / buckets)
/// live in a single loop.
enum AssessOutcome {
    /// Persist this assessment. `source` drives `cache_hits` / `live_calls`
    /// counters when set (provider path); injected assessors pass `None`.
    Scored {
        assessment: WinnabilityAssessment,
        source: Option<SerpSource>,
    },
    /// Daily SERP budget hit — **not** Avoid, do not persist. Core loop sets
    /// budget-exhausted and skips the rest of the batch without further network.
    BudgetSkip,
}

/// Single scoring loop: freshness / force / max_live / budget / persist / buckets.
///
/// `assess` is only invoked for candidates that still need a new score (not
/// fresh, under cap, budget not yet exhausted). After the first `BudgetSkip`,
/// remaining candidates are budget-skipped without calling `assess`.
fn score_candidates_loop(
    conn: &Connection,
    project_id: &str,
    opts: &ScoreOptions,
    now: DateTime<Utc>,
    mut assess: impl FnMut(&DeadWeightCandidate) -> AssessOutcome,
) -> Result<ScoreRunResult> {
    let candidates =
        load_low_impression_candidates(conn, project_id, opts.max_impressions)?;

    if candidates.is_empty() {
        return Ok(ScoreRunResult::empty(
            false,
            opts.ttl_days,
            opts.max_live,
            Some(
                "no low-impression published articles with target keywords found"
                    .to_string(),
            ),
        ));
    }

    let mut result = ScoreRunResult::empty(false, opts.ttl_days, opts.max_live, None);
    let mut assess_count = 0usize;
    let mut budget_exhausted = false;

    for candidate in &candidates {
        let stored = load_stored_score(conn, project_id, candidate.article_id)?;

        if !opts.force {
            if let Some(ref s) = stored {
                if is_score_fresh(s, opts.ttl_days, now) {
                    result.skipped_fresh += 1;
                    result.push_entry(entry_from_stored(
                        candidate.article_id,
                        &candidate.title,
                        &candidate.slug,
                        s,
                    ));
                    continue;
                }
            }
        }

        // After first budget hit: skip rest without network; surface stale cache.
        if budget_exhausted {
            result.skipped_budget += 1;
            if let Some(ref s) = stored {
                result.push_entry(entry_from_stored(
                    candidate.article_id,
                    &candidate.title,
                    &candidate.slug,
                    s,
                ));
            }
            continue;
        }

        if assess_count >= opts.max_live {
            result.skipped_cap += 1;
            // Surface stale cache if present so inventory is still useful.
            if let Some(ref s) = stored {
                result.push_entry(entry_from_stored(
                    candidate.article_id,
                    &candidate.title,
                    &candidate.slug,
                    s,
                ));
            }
            continue;
        }

        match assess(candidate) {
            AssessOutcome::BudgetSkip => {
                budget_exhausted = true;
                result.skipped_budget += 1;
                if let Some(ref s) = stored {
                    result.push_entry(entry_from_stored(
                        candidate.article_id,
                        &candidate.title,
                        &candidate.slug,
                        s,
                    ));
                }
            }
            AssessOutcome::Scored {
                assessment,
                source,
            } => {
                match source {
                    Some(SerpSource::Cache) => result.cache_hits += 1,
                    Some(SerpSource::Live) => result.live_calls += 1,
                    None => {}
                }
                persist_score(
                    conn,
                    project_id,
                    candidate.article_id,
                    &assessment,
                    candidate.impressions,
                    now,
                )?;
                assess_count += 1;
                result.scored += 1;
                result.push_entry(entry_from_assessment(
                    candidate,
                    &assessment,
                    &now.to_rfc3339(),
                    candidate.impressions,
                ));
            }
        }
    }

    result.truncated = result.skipped_cap > 0 || result.skipped_budget > 0;
    Ok(result)
}

/// Score low-impression candidates with an injected assessor (unit-testable;
/// no live SERP). Persists new scores; includes still-fresh cached scores so
/// bucket inventory reflects the full candidate set without re-paying SERP.
///
/// Cap (`max_live`) applies only to **new** live assessments.
/// No budget path — assessor always yields a score.
pub fn score_and_persist(
    conn: &Connection,
    project_id: &str,
    opts: &ScoreOptions,
    mut assess_fn: impl FnMut(&DeadWeightCandidate) -> WinnabilityAssessment,
) -> Result<ScoreRunResult> {
    let now = Utc::now();
    score_and_persist_at(conn, project_id, opts, now, &mut assess_fn)
}

/// Same as [`score_and_persist`] with an explicit `now` (tests inject clock).
pub fn score_and_persist_at(
    conn: &Connection,
    project_id: &str,
    opts: &ScoreOptions,
    now: DateTime<Utc>,
    assess_fn: &mut impl FnMut(&DeadWeightCandidate) -> WinnabilityAssessment,
) -> Result<ScoreRunResult> {
    score_candidates_loop(conn, project_id, opts, now, |candidate| {
        AssessOutcome::Scored {
            assessment: assess_fn(candidate),
            source: None,
        }
    })
}

/// Real provider SERP failure → Avoid with risk_score 99 (stable CLI shape).
///
/// **Do not** pass budget errors here — use [`is_budget_error`] and skip without
/// persisting Avoid. Budget is not competitive risk.
pub fn serp_error_assessment(keyword: &str, err: impl std::fmt::Display) -> WinnabilityAssessment {
    WinnabilityAssessment {
        keyword: keyword.to_string(),
        bucket: WinnabilityBucket::Avoid,
        ai_overview_present: false,
        featured_snippet_present: false,
        authority_competitors: vec![],
        risk_score: 99,
        reason: format!("SERP lookup failed: {err}"),
    }
}

/// Live path via [`fetch_serp_features`] (cache + daily project cap).
///
/// Call from a **synchronous** context with a Tokio runtime handle (e.g. CLI
/// after `Runtime::new()`). Do not nest this inside an outer `runtime.block_on`
/// of a future that itself calls `Handle::block_on` — that panics.
///
/// Budget soft-fail: increments `skipped_budget` and does **not** persist Avoid.
/// After the first budget skip, remaining candidates are also budget-skipped
/// (no pointless guard loop). Cache hits still assess free of charge.
pub fn score_and_persist_with_provider(
    conn: &Connection,
    project_id: &str,
    provider: &dyn SeoDataProvider,
    opts: &ScoreOptions,
    runtime: &tokio::runtime::Handle,
) -> Result<ScoreRunResult> {
    let now = Utc::now();
    score_candidates_loop(conn, project_id, opts, now, |candidate| {
        let lookup = match runtime.block_on(fetch_serp_features(
            conn,
            project_id,
            provider,
            &candidate.target_keyword,
            &opts.country,
        )) {
            Ok(lookup) => lookup,
            Err(e) if is_budget_error(&e) => {
                log::warn!(
                    "[dead_weight] SERP daily live-call budget hit for project {}: {}. \
                     Remaining candidates skipped without Avoid.",
                    project_id,
                    e
                );
                return AssessOutcome::BudgetSkip;
            }
            Err(e) => {
                // Real provider failure — existing Avoid mapping (not budget).
                return AssessOutcome::Scored {
                    assessment: serp_error_assessment(&candidate.target_keyword, &e),
                    source: None,
                };
            }
        };

        let kd = candidate
            .keyword_difficulty
            .as_deref()
            .and_then(|s| s.parse::<f64>().ok());
        let assessment = assess(
            &candidate.target_keyword,
            &lookup.features,
            kd,
            None,
        );
        AssessOutcome::Scored {
            assessment,
            source: Some(lookup.source),
        }
    })
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::seo::intent::IntentClassification;
    use crate::seo::keywords::{KeywordDifficultyResult, KeywordIdeasResult, SerpFeaturesResult};
    use crate::seo::winnability::WinnabilityBucket;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn in_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::init_with_conn(&conn).unwrap();
        conn.execute(
            "INSERT INTO projects (id, name, path, active) VALUES ('proj1', 'Test', '/tmp', 1)",
            [],
        )
        .unwrap();
        conn
    }

    /// Minimal provider that always "succeeds" SERP — used only when budget allows.
    struct CountingSerpProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl SeoDataProvider for CountingSerpProvider {
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
            Ok(SerpFeaturesResult {
                keyword: keyword.to_string(),
                ai_overview_present: false,
                featured_snippet_present: false,
                people_also_ask_present: false,
                organic_results: vec![],
            })
        }

        fn name(&self) -> &'static str {
            "mock"
        }
    }

    fn insert_article(
        conn: &Connection,
        id: i64,
        slug: &str,
        title: &str,
        keyword: &str,
        status: &str,
        kd: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO articles (
                id, project_id, title, url_slug, file, status, target_keyword,
                keyword_difficulty, content_gaps_addressed, target_volume,
                word_count, review_count, content_hash
             ) VALUES (?1, 'proj1', ?2, ?3, ?4, ?5, ?6, ?7, '[]', 0, 500, 0, 'hash')",
            rusqlite::params![
                id,
                title,
                slug,
                format!("{}.mdx", slug),
                status,
                keyword,
                kd,
            ],
        )
        .unwrap();
    }

    fn set_gsc_impressions(conn: &Connection, article_id: i64, impressions: f64) {
        let payload = serde_json::json!({ "impressions": impressions }).to_string();
        crate::db::set_article_metadata(conn, "proj1", article_id, "gsc", &payload).unwrap();
    }

    fn make_assessment(keyword: &str, bucket: WinnabilityBucket, risk: u32) -> WinnabilityAssessment {
        WinnabilityAssessment {
            keyword: keyword.to_string(),
            bucket,
            ai_overview_present: bucket != WinnabilityBucket::Target,
            featured_snippet_present: false,
            authority_competitors: if bucket == WinnabilityBucket::Avoid {
                vec!["investopedia.com".into()]
            } else {
                vec![]
            },
            risk_score: risk,
            reason: format!("test reason for {keyword}"),
        }
    }

    #[test]
    fn persist_and_load_round_trip() {
        let conn = in_memory_db();
        insert_article(
            &conn,
            1,
            "theta-guide",
            "Theta Guide",
            "theta decay",
            "published",
            Some("20"),
        );
        let assessment = make_assessment("theta decay", WinnabilityBucket::Target, 1);
        let now = Utc::now();
        persist_score(&conn, "proj1", 1, &assessment, 3.0, now).unwrap();

        let loaded = load_stored_score(&conn, "proj1", 1).unwrap().expect("stored");
        assert_eq!(loaded.keyword, "theta decay");
        assert_eq!(loaded.bucket, "target");
        assert_eq!(loaded.risk_score, 1);
        assert_eq!(loaded.impressions_at_score, Some(3.0));
        assert_eq!(loaded.source.as_deref(), Some(SCORE_SOURCE));
        assert!(is_score_fresh(&loaded, DEFAULT_SCORE_TTL_DAYS, now));
    }

    #[test]
    fn list_from_cache_groups_buckets_and_empty() {
        let conn = in_memory_db();
        let empty = list_from_cache(&conn, "proj1").unwrap();
        assert!(empty.from_cache);
        assert_eq!(empty.avoid.count + empty.differentiate.count + empty.target.count, 0);
        assert!(empty.message.is_some());

        insert_article(&conn, 1, "a", "A", "kw a", "published", None);
        insert_article(&conn, 2, "b", "B", "kw b", "published", None);
        insert_article(&conn, 3, "c", "C", "kw c", "published", None);
        let now = Utc::now();
        persist_score(
            &conn,
            "proj1",
            1,
            &make_assessment("kw a", WinnabilityBucket::Avoid, 5),
            0.0,
            now,
        )
        .unwrap();
        persist_score(
            &conn,
            "proj1",
            2,
            &make_assessment("kw b", WinnabilityBucket::Differentiate, 2),
            1.0,
            now,
        )
        .unwrap();
        persist_score(
            &conn,
            "proj1",
            3,
            &make_assessment("kw c", WinnabilityBucket::Target, 0),
            2.0,
            now,
        )
        .unwrap();

        let listed = list_from_cache(&conn, "proj1").unwrap();
        assert!(listed.from_cache);
        assert_eq!(listed.scored, 0);
        assert_eq!(listed.avoid.count, 1);
        assert_eq!(listed.differentiate.count, 1);
        assert_eq!(listed.target.count, 1);
        assert_eq!(listed.avoid.articles[0].slug, "a");
        assert_eq!(listed.differentiate.articles[0].slug, "b");
        assert_eq!(listed.target.articles[0].slug, "c");
    }

    #[test]
    fn ttl_skips_fresh_unless_force() {
        let conn = in_memory_db();
        insert_article(&conn, 1, "fresh", "Fresh", "fresh kw", "published", None);
        set_gsc_impressions(&conn, 1, 0.0);

        let now = Utc::now();
        persist_score(
            &conn,
            "proj1",
            1,
            &make_assessment("fresh kw", WinnabilityBucket::Target, 0),
            0.0,
            now,
        )
        .unwrap();

        let mut calls = 0usize;
        let opts = ScoreOptions {
            force: false,
            max_live: 25,
            ..Default::default()
        };
        let result = score_and_persist_at(&conn, "proj1", &opts, now, &mut |_| {
            calls += 1;
            make_assessment("fresh kw", WinnabilityBucket::Avoid, 9)
        })
        .unwrap();

        assert_eq!(calls, 0, "fresh score must not re-assess");
        assert_eq!(result.scored, 0);
        assert_eq!(result.skipped_fresh, 1);
        assert_eq!(result.target.count, 1);
        assert!(!result.truncated);

        // Force re-scores.
        let mut force_calls = 0usize;
        let force_opts = ScoreOptions {
            force: true,
            ..Default::default()
        };
        let forced = score_and_persist_at(&conn, "proj1", &force_opts, now, &mut |_| {
            force_calls += 1;
            make_assessment("fresh kw", WinnabilityBucket::Avoid, 9)
        })
        .unwrap();
        assert_eq!(force_calls, 1);
        assert_eq!(forced.scored, 1);
        assert_eq!(forced.skipped_fresh, 0);
        assert_eq!(forced.avoid.count, 1);

        let reloaded = load_stored_score(&conn, "proj1", 1).unwrap().unwrap();
        assert_eq!(reloaded.bucket, "avoid");
        assert_eq!(reloaded.risk_score, 9);
    }

    #[test]
    fn cap_limits_live_assessments() {
        let conn = in_memory_db();
        for (id, slug) in [(1, "one"), (2, "two"), (3, "three")] {
            insert_article(
                &conn,
                id,
                slug,
                slug,
                &format!("kw {slug}"),
                "published",
                None,
            );
            set_gsc_impressions(&conn, id, 0.0);
        }

        // Stale scores (or none) so all three need live assessment.
        let now = Utc::now();
        let stale = now - Duration::days(90);
        for id in [1i64, 2] {
            persist_score(
                &conn,
                "proj1",
                id,
                &make_assessment("old", WinnabilityBucket::Target, 0),
                0.0,
                stale,
            )
            .unwrap();
        }

        let mut calls = 0usize;
        let opts = ScoreOptions {
            force: false,
            max_live: 2,
            ttl_days: 60,
            ..Default::default()
        };
        let result = score_and_persist_at(&conn, "proj1", &opts, now, &mut |c| {
            calls += 1;
            make_assessment(&c.target_keyword, WinnabilityBucket::Differentiate, 2)
        })
        .unwrap();

        assert_eq!(calls, 2);
        assert_eq!(result.scored, 2);
        assert_eq!(result.skipped_cap, 1);
        assert!(result.truncated);
        assert_eq!(result.max_live, 2);
        // Two newly scored + one stale cache still listed (article 1 or 2 stale
        // was re-scored; article without score or third may be cap-skipped).
        let total = result.avoid.count + result.differentiate.count + result.target.count;
        assert!(total >= 2);
    }

    #[test]
    fn is_score_fresh_edge_cases() {
        let now = Utc::now();
        let fresh = StoredWinnabilityScore {
            keyword: "k".into(),
            bucket: "target".into(),
            risk_score: 0,
            reason: "r".into(),
            ai_overview_present: false,
            featured_snippet_present: false,
            authority_competitors: vec![],
            scored_at: now.to_rfc3339(),
            impressions_at_score: None,
            source: None,
        };
        assert!(is_score_fresh(&fresh, 60, now));
        assert!(is_score_fresh(&fresh, 60, now + Duration::days(30)));
        assert!(!is_score_fresh(&fresh, 60, now + Duration::days(61)));

        let bad = StoredWinnabilityScore {
            scored_at: "not-a-date".into(),
            ..fresh.clone()
        };
        assert!(!is_score_fresh(&bad, 60, now));

        let boundary = StoredWinnabilityScore {
            scored_at: (now - Duration::days(60)).to_rfc3339(),
            ..fresh
        };
        assert!(is_score_fresh(&boundary, 60, now));
    }

    #[test]
    fn load_candidates_filters_keyword_status_impressions() {
        let conn = in_memory_db();
        insert_article(&conn, 1, "pub-low", "Pub Low", "kw1", "published", None);
        insert_article(&conn, 2, "pub-high", "Pub High", "kw2", "published", None);
        insert_article(&conn, 3, "draft", "Draft", "kw3", "draft", None);
        insert_article(&conn, 4, "no-kw", "No Kw", "", "published", None);
        set_gsc_impressions(&conn, 1, 5.0);
        set_gsc_impressions(&conn, 2, 100.0);
        // article 4 has no keyword — excluded
        // article 3 draft — excluded
        // article 1 low impr — included
        // article with no gsc (use id 5)
        insert_article(&conn, 5, "no-gsc", "No Gsc", "kw5", "published", None);

        let cands = load_low_impression_candidates(&conn, "proj1", 10.0).unwrap();
        let slugs: Vec<_> = cands.iter().map(|c| c.slug.as_str()).collect();
        assert!(slugs.contains(&"pub-low"));
        assert!(slugs.contains(&"no-gsc"));
        assert!(!slugs.contains(&"pub-high"));
        assert!(!slugs.contains(&"draft"));
        assert!(!slugs.contains(&"no-kw"));
    }

    #[test]
    fn score_run_includes_fresh_cached_in_buckets() {
        let conn = in_memory_db();
        insert_article(&conn, 1, "cached", "Cached", "kw cached", "published", None);
        insert_article(&conn, 2, "new", "New", "kw new", "published", None);
        set_gsc_impressions(&conn, 1, 0.0);
        set_gsc_impressions(&conn, 2, 0.0);

        let now = Utc::now();
        persist_score(
            &conn,
            "proj1",
            1,
            &make_assessment("kw cached", WinnabilityBucket::Avoid, 5),
            0.0,
            now,
        )
        .unwrap();

        let mut calls = 0usize;
        let result = score_and_persist_at(
            &conn,
            "proj1",
            &ScoreOptions::default(),
            now,
            &mut |c| {
                calls += 1;
                make_assessment(&c.target_keyword, WinnabilityBucket::Target, 0)
            },
        )
        .unwrap();

        assert_eq!(calls, 1);
        assert_eq!(result.scored, 1);
        assert_eq!(result.skipped_fresh, 1);
        assert_eq!(result.avoid.count, 1);
        assert_eq!(result.target.count, 1);
        assert!(!result.from_cache);
    }

    /// Budget must soft-skip — never persist Avoid / risk 99 via serp_error_assessment.
    #[test]
    fn budget_error_does_not_persist_avoid() {
        let conn = in_memory_db();
        for (id, slug) in [(1i64, "one"), (2, "two"), (3, "three")] {
            insert_article(
                &conn,
                id,
                slug,
                slug,
                &format!("kw {slug}"),
                "published",
                None,
            );
            set_gsc_impressions(&conn, id, 0.0);
        }

        let day = Utc::now().format("%Y-%m-%d").to_string();
        // Exhaust daily live cap (default 50) so every live fetch budgets out.
        conn.execute(
            "INSERT INTO serp_daily_usage (project_id, day, live_calls) VALUES ('proj1', ?1, 50)",
            rusqlite::params![day],
        )
        .unwrap();

        let calls = Arc::new(AtomicUsize::new(0));
        let provider = CountingSerpProvider {
            calls: calls.clone(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = score_and_persist_with_provider(
            &conn,
            "proj1",
            &provider,
            &ScoreOptions::default(),
            rt.handle(),
        )
        .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0, "budget must not call provider");
        assert_eq!(result.scored, 0);
        assert!(result.skipped_budget >= 3);
        assert_eq!(result.avoid.count, 0);
        assert_eq!(result.target.count, 0);
        assert_eq!(result.differentiate.count, 0);
        assert_eq!(result.live_calls, 0);

        // Nothing persisted under winnability.
        for id in [1i64, 2, 3] {
            assert!(
                load_stored_score(&conn, "proj1", id).unwrap().is_none(),
                "article {id} must not get Avoid from budget"
            );
        }
    }

    #[test]
    fn serp_error_assessment_is_avoid_99_for_real_errors_only() {
        let a = serp_error_assessment("kw", "timeout");
        assert_eq!(a.bucket, WinnabilityBucket::Avoid);
        assert_eq!(a.risk_score, 99);
        assert!(a.reason.contains("timeout"));
        // Budget errors must be matched via is_budget_error, not this helper.
        let budget = Error::SerpBudgetExceeded {
            project_id: "p".into(),
            day: "2026-01-01".into(),
            cap: 50,
        };
        assert!(crate::seo::serp_guard::is_budget_error(&budget));
    }
}
