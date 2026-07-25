//! TF-IDF vectorization helpers.

use super::*;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

// ═══════════════════════════════════════════════════════════════════════════════
// TF-IDF
// ═══════════════════════════════════════════════════════════════════════════════

/// Tokenize text into normalized terms suitable for TF-IDF.
pub(crate) fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty() && s.len() > 2)
        .map(|s| s.to_string())
        .collect()
}

/// Compute raw term frequencies for a token list.
pub(crate) fn compute_tf(tokens: &[String]) -> HashMap<String, f64> {
    let mut tf: HashMap<String, f64> = HashMap::new();
    if tokens.is_empty() {
        return tf;
    }
    for token in tokens {
        *tf.entry(token.clone()).or_insert(0.0) += 1.0;
    }
    let n = tokens.len() as f64;
    for count in tf.values_mut() {
        *count /= n;
    }
    tf
}

/// Compute inverse document frequency across the corpus.
pub(crate) fn compute_idf(documents: &[Vec<String>]) -> HashMap<String, f64> {
    let n = documents.len() as f64;
    let mut df: HashMap<String, f64> = HashMap::new();
    for doc in documents {
        let unique: HashSet<&String> = doc.iter().collect();
        for term in unique {
            *df.entry(term.clone()).or_insert(0.0) += 1.0;
        }
    }
    let mut idf: HashMap<String, f64> = HashMap::new();
    for (term, doc_count) in df {
        idf.insert(term, (n / doc_count).ln() + 1.0);
    }
    idf
}

/// Build a TF-IDF vector from term frequencies and IDF map.
pub(crate) fn build_tf_idf_vector(tf: &HashMap<String, f64>, idf: &HashMap<String, f64>) -> TfIdfVector {
    let mut weights: HashMap<String, f64> = HashMap::new();
    let mut norm_sq = 0.0;
    for (term, tf_val) in tf {
        let idf_val = idf.get(term).copied().unwrap_or(0.0);
        let w = tf_val * idf_val;
        weights.insert(term.clone(), w);
        norm_sq += w * w;
    }
    TfIdfVector {
        weights,
        norm: norm_sq.sqrt(),
    }
}

/// Compute cosine similarity between two TF-IDF vectors.
pub(crate) fn cosine_similarity(a: &TfIdfVector, b: &TfIdfVector) -> f64 {
    if a.norm == 0.0 || b.norm == 0.0 {
        return 0.0;
    }
    let mut dot = 0.0;
    for (term, w_a) in &a.weights {
        if let Some(w_b) = b.weights.get(term) {
            dot += w_a * w_b;
        }
    }
    dot / (a.norm * b.norm)
}
