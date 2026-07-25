//! Data structures for cannibalization clustering and evidence shortlist.

use std::collections::HashMap;

// ═══════════════════════════════════════════════════════════════════════════════
// Evidence shortlist types (issue #117 / #121)
// ═══════════════════════════════════════════════════════════════════════════════

/// Evidence lane that authorizes a merge shortlist candidate.
///
/// Soft TF-IDF clusters are exploratory only and are **not** a lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EvidenceLane {
    ExactKeyword,
    SharedQuery,
    NearDupe,
}

impl EvidenceLane {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ExactKeyword => "exact_keyword",
            Self::SharedQuery => "shared_query",
            Self::NearDupe => "near_dupe",
        }
    }

    /// Skill / rich-candidate `candidate_type` derived from the lane.
    pub(crate) fn candidate_type(self) -> &'static str {
        match self {
            Self::ExactKeyword => "exact_keyword_dupe",
            Self::SharedQuery => "shared_query",
            Self::NearDupe => "near_dupe",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "exact_keyword" => Some(Self::ExactKeyword),
            "shared_query" => Some(Self::SharedQuery),
            "near_dupe" => Some(Self::NearDupe),
            _ => None,
        }
    }
}

/// Typed merge shortlist candidate. `lane` is the single source of truth;
/// `candidate_type` and dual-name aliases are derived at serialize time.
#[derive(Debug, Clone)]
pub(crate) struct MergeCandidate {
    pub(crate) candidate_id: String,
    pub(crate) lane: EvidenceLane,
    pub(crate) theme: String,
    /// Compact page objects (id, url, title, metrics, …). Kept as Value to
    /// avoid over-typing the page shell.
    pub(crate) pages: Vec<serde_json::Value>,
    /// Canonical shared-query list (exact keyword, GSC query, or empty).
    pub(crate) shared_queries: Vec<String>,
    pub(crate) total_impressions: f64,
    pub(crate) page_count: usize,
    /// Max pairwise similarity when known (near_dupe); None otherwise.
    pub(crate) max_pairwise_sim: Option<f64>,
    pub(crate) best_performer: Option<serde_json::Value>,
}

impl MergeCandidate {
    /// Rich candidate artifact for analyze / skills.
    ///
    /// Serializes dual names as aliases of the same fields:
    /// - `candidate_type` ← `lane.candidate_type()`
    /// - `top_shared_queries` ← `shared_queries`
    /// - `pair_similarity` ← `max_pairwise_sim` (near_dupe only)
    pub(crate) fn to_rich_json(&self) -> serde_json::Value {
        let sim = self.max_pairwise_sim.unwrap_or(0.0);
        let mut obj = serde_json::json!({
            "candidate_id": self.candidate_id,
            "lane": self.lane.as_str(),
            "candidate_type": self.lane.candidate_type(),
            "theme": self.theme,
            "pages": self.pages,
            "shared_queries": self.shared_queries,
            "top_shared_queries": self.shared_queries,
            "shared_query_count": self.shared_queries.len(),
            "total_impressions": self.total_impressions,
            "page_count": self.page_count,
            "max_pairwise_sim": sim,
        });
        if let Some(map) = obj.as_object_mut() {
            if self.lane == EvidenceLane::NearDupe {
                map.insert("pair_similarity".to_string(), serde_json::json!(sim));
            }
            if let Some(bp) = &self.best_performer {
                map.insert("best_performer".to_string(), bp.clone());
            }
        }
        obj
    }

    /// #117 ID-based evidence shortlist entry.
    pub(crate) fn to_evidence_json(&self) -> serde_json::Value {
        let page_ids: Vec<i64> = self
            .pages
            .iter()
            .filter_map(|p| p["id"].as_i64())
            .collect();
        serde_json::json!({
            "candidate_id": self.candidate_id,
            "lane": self.lane.as_str(),
            "pages": page_ids,
            "shared_queries": self.shared_queries,
            "max_pairwise_sim": self.max_pairwise_sim.unwrap_or(0.0),
            "total_impressions": self.total_impressions,
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Clustering data structures
// ═══════════════════════════════════════════════════════════════════════════════

#[derive(Debug)]
pub(crate) struct ArticleRecord {
    pub(crate) id: i64,
    pub(crate) url_slug: String,
    pub(crate) title: String,
    pub(crate) h1: String,
    pub(crate) target_keyword: String,
    pub(crate) first_200_words: String,
    pub(crate) file: String,
    pub(crate) gsc: serde_json::Value,
    pub(crate) tokens: Vec<String>,
    pub(crate) incoming_links: usize,
    pub(crate) outgoing_links: usize,
    pub(crate) published_date: String,
    pub(crate) word_count: usize,
    pub(crate) page_type: Option<String>,
}

#[derive(Debug)]
pub(crate) struct TfIdfVector {
    pub(crate) weights: HashMap<String, f64>,
    pub(crate) norm: f64,
}

#[derive(Debug)]
pub(crate) struct Cluster {
    pub(crate) cluster_id: String,
    pub(crate) theme: String,
    pub(crate) candidate_intent: String,
    pub(crate) total_impressions: f64,
    pub(crate) total_clicks: f64,
    pub(crate) avg_position: f64,
    pub(crate) shared_query_count: usize,
    pub(crate) hub_exists: bool,
    pub(crate) page_ids: Vec<i64>,
    pub(crate) top_shared_queries: Vec<String>,
}

// ─── Union-Find for connected-component clustering ────────────────────────────

pub(crate) struct UnionFind {
    pub(crate) parent: Vec<usize>,
    pub(crate) rank: Vec<usize>,
}

impl UnionFind {
    pub(crate) fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    pub(crate) fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    pub(crate) fn union(&mut self, x: usize, y: usize) {
        let rx = self.find(x);
        let ry = self.find(y);
        if rx == ry {
            return;
        }
        match self.rank[rx].cmp(&self.rank[ry]) {
            std::cmp::Ordering::Less => self.parent[rx] = ry,
            std::cmp::Ordering::Greater => self.parent[ry] = rx,
            std::cmp::Ordering::Equal => {
                self.parent[ry] = rx;
                self.rank[rx] += 1;
            }
        }
    }
}
