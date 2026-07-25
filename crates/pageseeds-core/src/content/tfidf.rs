/// TF-IDF topical similarity for content analysis.
///
/// Extracted from cannibalization_audit.rs for reuse in source candidate ranking,
/// article clustering, and any other domain that needs lightweight topical similarity.
use std::collections::{HashMap, HashSet};

// ═══════════════════════════════════════════════════════════════════════════════
// Stopwords
// ═══════════════════════════════════════════════════════════════════════════════

/// Comprehensive English stopword set for content similarity.
fn is_stop_word(word: &str) -> bool {
    matches!(
        word.to_lowercase().as_str(),
        "a" | "an"
            | "the"
            | "and"
            | "or"
            | "but"
            | "in"
            | "on"
            | "at"
            | "to"
            | "for"
            | "of"
            | "with"
            | "by"
            | "from"
            | "as"
            | "is"
            | "was"
            | "are"
            | "were"
            | "be"
            | "been"
            | "being"
            | "have"
            | "has"
            | "had"
            | "do"
            | "does"
            | "did"
            | "will"
            | "would"
            | "could"
            | "should"
            | "may"
            | "might"
            | "must"
            | "shall"
            | "can"
            | "need"
            | "it"
            | "its"
            | "this"
            | "that"
            | "these"
            | "those"
            | "i"
            | "you"
            | "he"
            | "she"
            | "we"
            | "they"
            | "them"
            | "their"
            | "what"
            | "which"
            | "who"
            | "when"
            | "where"
            | "why"
            | "how"
            | "all"
            | "any"
            | "both"
            | "each"
            | "few"
            | "more"
            | "most"
            | "other"
            | "some"
            | "such"
            | "no"
            | "nor"
            | "not"
            | "only"
            | "own"
            | "same"
            | "so"
            | "than"
            | "too"
            | "very"
            | "just"
            | "now"
            | "then"
            | "here"
            | "there"
            | "up"
            | "down"
            | "out"
            | "off"
            | "over"
            | "under"
            | "again"
            | "further"
            | "once"
            | "during"
            | "before"
            | "after"
            | "above"
            | "below"
            | "between"
            | "through"
            | "into"
            | "about"
    )
}

// ═══════════════════════════════════════════════════════════════════════════════
// Core TF-IDF
// ═══════════════════════════════════════════════════════════════════════════════

/// Tokenize text into normalized terms suitable for TF-IDF.
///
/// - Lowercases
/// - Splits on non-alphanumeric characters
/// - Filters out empty strings, short words (< 3 chars), and English stopwords
pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty() && s.len() > 2 && !is_stop_word(s))
        .map(|s| s.to_string())
        .collect()
}

/// Compute raw term frequencies for a token list.
pub fn compute_tf(tokens: &[String]) -> HashMap<String, f64> {
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
pub fn compute_idf(documents: &[Vec<String>]) -> HashMap<String, f64> {
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

/// A TF-IDF vector with precomputed norm for fast cosine similarity.
#[derive(Debug, Clone)]
pub struct TfIdfVector {
    weights: HashMap<String, f64>,
    norm: f64,
}

impl TfIdfVector {
    pub fn weights(&self) -> &HashMap<String, f64> {
        &self.weights
    }

    pub fn norm(&self) -> f64 {
        self.norm
    }
}

/// Build a TF-IDF vector from term frequencies and IDF map.
pub fn build_tf_idf_vector(tf: &HashMap<String, f64>, idf: &HashMap<String, f64>) -> TfIdfVector {
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
/// Returns a value in [0.0, 1.0].
pub fn cosine_similarity(a: &TfIdfVector, b: &TfIdfVector) -> f64 {
    if a.norm == 0.0 || b.norm == 0.0 {
        return 0.0;
    }
    let mut dot = 0.0;
    for (term, w_a) in &a.weights {
        if let Some(w_b) = b.weights.get(term) {
            dot += w_a * w_b;
        }
    }
    let sim = dot / (a.norm * b.norm);
    // Clamp to [0, 1] to avoid floating-point drift
    sim.clamp(0.0, 1.0)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Convenience APIs
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute TF-IDF similarity between two raw text strings.
///
/// This is a one-off convenience. For batch comparisons (e.g. ranking 100
/// candidates against one target), build the vectors once and reuse them.
pub fn similarity_between_texts(a: &str, b: &str) -> f64 {
    let tokens_a = tokenize(a);
    let tokens_b = tokenize(b);
    let idf = compute_idf(&[tokens_a.clone(), tokens_b.clone()]);
    let tf_a = compute_tf(&tokens_a);
    let tf_b = compute_tf(&tokens_b);
    let vec_a = build_tf_idf_vector(&tf_a, &idf);
    let vec_b = build_tf_idf_vector(&tf_b, &idf);
    cosine_similarity(&vec_a, &vec_b)
}

/// A scored candidate for ranking.
#[derive(Debug, Clone)]
pub struct ScoredCandidate<T> {
    pub item: T,
    pub similarity: f64,
}

/// Rank candidates by TF-IDF cosine similarity to a target text.
///
/// `target_text` is the query text (e.g. target article title + keyword).
/// `candidates` is a list of (id, text) pairs where text is the candidate's
/// combined title + keyword + excerpt.
///
/// Returns candidates sorted by similarity descending.
pub fn rank_candidates_by_similarity<T: Clone>(
    target_text: &str,
    candidates: &[(T, String)],
) -> Vec<ScoredCandidate<T>> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let target_tokens = tokenize(target_text);
    let candidate_tokens: Vec<Vec<String>> =
        candidates.iter().map(|(_, text)| tokenize(text)).collect();

    // Build corpus: target + all candidates
    let mut all_docs: Vec<Vec<String>> = Vec::with_capacity(candidate_tokens.len() + 1);
    all_docs.push(target_tokens.clone());
    all_docs.extend(candidate_tokens.clone());

    let idf = compute_idf(&all_docs);
    let target_tf = compute_tf(&target_tokens);
    let target_vec = build_tf_idf_vector(&target_tf, &idf);

    let mut scored: Vec<ScoredCandidate<T>> = Vec::with_capacity(candidates.len());
    for (i, (item, _)) in candidates.iter().enumerate() {
        let tf = compute_tf(&candidate_tokens[i]);
        let vec = build_tf_idf_vector(&tf, &idf);
        let similarity = cosine_similarity(&target_vec, &vec);
        scored.push(ScoredCandidate {
            item: item.clone(),
            similarity,
        });
    }

    scored.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap());
    scored
}

// ═══════════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_filters_stopwords_and_short_words() {
        let text = "The quick brown fox jumps over the lazy dog";
        let tokens = tokenize(text);
        assert!(!tokens.contains(&"the".to_string()));
        assert!(!tokens.contains(&"over".to_string()));
        assert!(tokens.contains(&"dog".to_string())); // "dog" is >2 chars, not a stopword
        assert!(tokens.contains(&"quick".to_string()));
        assert!(tokens.contains(&"brown".to_string()));
        assert!(tokens.contains(&"jumps".to_string()));
    }

    #[test]
    fn cosine_similarity_identical_texts() {
        let text = "machine learning algorithms for data analysis";
        let tokens = tokenize(text);
        let idf = compute_idf(&[tokens.clone()]);
        let tf = compute_tf(&tokens);
        let vec = build_tf_idf_vector(&tf, &idf);
        let sim = cosine_similarity(&vec, &vec);
        assert!(
            (sim - 1.0).abs() < 0.001,
            "identical vectors should have similarity 1.0"
        );
    }

    #[test]
    fn cosine_similarity_unrelated_texts() {
        let a = tokenize("machine learning neural networks");
        let b = tokenize("chocolate cake baking recipe");
        let idf = compute_idf(&[a.clone(), b.clone()]);
        let vec_a = build_tf_idf_vector(&compute_tf(&a), &idf);
        let vec_b = build_tf_idf_vector(&compute_tf(&b), &idf);
        let sim = cosine_similarity(&vec_a, &vec_b);
        assert!(
            sim < 0.1,
            "unrelated texts should have low similarity, got {}",
            sim
        );
    }

    #[test]
    fn cosine_similarity_related_texts() {
        let a = tokenize("machine learning algorithms for natural language processing");
        let b = tokenize("deep learning models for text classification");
        let idf = compute_idf(&[a.clone(), b.clone()]);
        let vec_a = build_tf_idf_vector(&compute_tf(&a), &idf);
        let vec_b = build_tf_idf_vector(&compute_tf(&b), &idf);
        let sim = cosine_similarity(&vec_a, &vec_b);
        assert!(
            sim > 0.05,
            "related texts should have moderate similarity, got {}",
            sim
        );
    }

    #[test]
    fn similarity_between_texts_range() {
        let sim = similarity_between_texts("hello world", "hello world");
        assert!((sim - 1.0).abs() < 0.001);

        let sim = similarity_between_texts("hello world", "completely different topic");
        assert!(sim < 0.2);
    }

    #[test]
    fn rank_candidates_by_similarity_orders_correctly() {
        let target = "machine learning";
        let candidates: Vec<(i64, String)> = vec![
            (1, "baking chocolate cake".to_string()),
            (2, "deep learning neural networks".to_string()),
            (3, "supervised machine learning algorithms".to_string()),
        ];

        let ranked = rank_candidates_by_similarity(target, &candidates);
        assert_eq!(ranked.len(), 3);
        // Most similar should be #3 (explicitly mentions machine learning)
        assert_eq!(ranked[0].item, 3);
        // Second should be #2 (learning is shared)
        assert_eq!(ranked[1].item, 2);
        // Least similar should be #1 (baking)
        assert_eq!(ranked[2].item, 1);
    }

    #[test]
    fn rank_candidates_empty_input() {
        let ranked: Vec<ScoredCandidate<i64>> = rank_candidates_by_similarity("test", &[]);
        assert!(ranked.is_empty());
    }
}
