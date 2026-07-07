//! Data structures for cannibalization clustering.

use super::*;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

// ═══════════════════════════════════════════════════════════════════════════════
// Data structures
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
