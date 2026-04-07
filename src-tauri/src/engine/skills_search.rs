/// Semantic search for skills using vector embeddings.
///
/// Uses Ollama for local embeddings (no API keys required).
/// Falls back gracefully if Ollama is not available.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::engine::skills::Skill;
use crate::error::Result;

// ─── Configuration ───────────────────────────────────────────────────────────

/// Default embedding model - small, fast, good quality for semantic search.
/// Requires: `ollama pull nomic-embed-text`
const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";

/// Ollama API endpoint for local embeddings
const OLLAMA_BASE_URL: &str = "http://localhost:11434";

/// Number of dimensions for nomic-embed-text
const EMBEDDING_DIMENSIONS: usize = 768;

// ─── Types ───────────────────────────────────────────────────────────────────

/// A skill with similarity score from semantic search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredSkill {
    pub skill: Skill,
    pub score: f32,
}

/// Status of the embedding service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingStatus {
    pub available: bool,
    pub model: String,
    pub error: Option<String>,
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Check if Ollama is available for embeddings.
pub async fn check_status() -> EmbeddingStatus {
    match check_ollama_health().await {
        Ok(true) => EmbeddingStatus {
            available: true,
            model: DEFAULT_EMBEDDING_MODEL.to_string(),
            error: None,
        },
        Ok(false) => EmbeddingStatus {
            available: false,
            model: DEFAULT_EMBEDDING_MODEL.to_string(),
            error: Some("Ollama server not responding".to_string()),
        },
        Err(e) => EmbeddingStatus {
            available: false,
            model: DEFAULT_EMBEDDING_MODEL.to_string(),
            error: Some(e.to_string()),
        },
    }
}

/// Index all skills for a project, creating embeddings for semantic search.
///
/// This is idempotent - only re-indexes skills that have changed.
/// 
/// Note: This function performs HTTP calls to Ollama and SQLite operations.
/// It should be called from within a spawn_blocking block when used in async context.
pub fn index_skills_blocking(
    conn: &Connection,
    project_id: &str,
    skills: &[Skill],
) -> Result<usize> {
    let rt = tokio::runtime::Handle::current();
    
    // Check Ollama health synchronously
    let health = rt.block_on(check_ollama_health())?;
    if !health {
        return Err(crate::error::Error::Other(
            "Ollama not available. Run: ollama pull nomic-embed-text && ollama serve".to_string(),
        ));
    }

    let mut indexed = 0;
    for skill in skills {
        let content_hash = hash_content(&skill.content);
        
        // Check if already indexed with same content
        let existing_hash: Option<String> = conn
            .query_row(
                "SELECT content_hash FROM skill_embeddings WHERE skill_name = ?1 AND project_id = ?2",
                [&skill.name, project_id],
                |row| row.get(0),
            )
            .optional()?;

        if existing_hash.as_ref() == Some(&content_hash) {
            continue; // Already up to date
        }

        // Generate embedding via Ollama (async call in sync context)
        let embedding = rt.block_on(generate_embedding(&skill.content))?;
        
        // Store in SQLite
        let embedding_bytes = serialize_embedding(&embedding);
        let now = chrono::Utc::now().to_rfc3339();
        
        conn.execute(
            r#"INSERT INTO skill_embeddings (skill_name, project_id, content_hash, embedding, model_name, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
               ON CONFLICT(skill_name) DO UPDATE SET
                   content_hash = excluded.content_hash,
                   embedding = excluded.embedding,
                   model_name = excluded.model_name,
                   updated_at = excluded.updated_at"#,
            rusqlite::params![
                &skill.name,
                project_id,
                &content_hash,
                &embedding_bytes,
                DEFAULT_EMBEDDING_MODEL,
                &now,
            ],
        )?;
        
        indexed += 1;
    }
    
    Ok(indexed)
}

/// Search skills by semantic similarity to a query.
///
/// Returns the top N most relevant skills sorted by similarity score.
/// 
/// Note: This function performs HTTP calls to Ollama and SQLite operations.
/// It should be called from within a spawn_blocking block when used in async context.
pub fn search_skills_blocking(
    conn: &Connection,
    project_id: &str,
    query: &str,
    limit: usize,
    all_skills: &[Skill],
) -> Result<Vec<ScoredSkill>> {
    let rt = tokio::runtime::Handle::current();
    
    // Check if we have any indexed skills for this project
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM skill_embeddings WHERE project_id = ?1",
        [project_id],
        |row| row.get(0),
    )?;

    if count == 0 {
        // No indexed skills - fall back to returning all skills un-scored
        return Ok(all_skills
            .iter()
            .map(|s| ScoredSkill {
                skill: s.clone(),
                score: 0.0,
            })
            .collect());
    }

    // Generate query embedding (async call in sync context)
    let query_embedding = rt.block_on(generate_embedding(query))?;
    
    // Load all indexed embeddings for this project
    let mut stmt = conn.prepare(
        "SELECT skill_name, embedding FROM skill_embeddings WHERE project_id = ?1"
    )?;
    
    let rows = stmt.query_map([project_id], |row| {
        let name: String = row.get(0)?;
        let bytes: Vec<u8> = row.get(1)?;
        let embedding = deserialize_embedding(&bytes);
        Ok((name, embedding))
    })?;

    let mut scored: Vec<(String, f32)> = Vec::new();
    for row in rows {
        let (name, embedding) = row?;
        let score = cosine_similarity(&query_embedding, &embedding);
        scored.push((name, score));
    }
    
    // Sort by score descending and take top N
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(limit);
    
    // Build result with full skill data
    let mut results = Vec::new();
    for (name, score) in scored {
        if let Some(skill) = all_skills.iter().find(|s| s.name == name) {
            results.push(ScoredSkill {
                skill: skill.clone(),
                score,
            });
        }
    }
    
    Ok(results)
}

/// Remove all skill embeddings for a project (e.g., when re-indexing from scratch).
pub fn clear_index(conn: &Connection, project_id: &str) -> Result<usize> {
    let count = conn.execute(
        "DELETE FROM skill_embeddings WHERE project_id = ?1",
        [project_id],
    )?;
    Ok(count)
}

// ─── Internal Helpers ────────────────────────────────────────────────────────

/// Check if Ollama server is running.
async fn check_ollama_health() -> Result<bool> {
    let client = reqwest::Client::new();
    match client.get(format!("{}/api/tags", OLLAMA_BASE_URL)).send().await {
        Ok(resp) => Ok(resp.status().is_success()),
        Err(_) => Ok(false),
    }
}

/// Generate embedding vector for text using Ollama.
async fn generate_embedding(text: &str) -> Result<Vec<f32>> {
    let client = reqwest::Client::new();
    
    // Truncate very long texts (nomic-embed-text has 8192 token limit)
    let truncated = if text.len() > 30000 {
        &text[..30000]
    } else {
        text
    };
    
    let request = serde_json::json!({
        "model": DEFAULT_EMBEDDING_MODEL,
        "prompt": truncated,
    });
    
    let response = client
        .post(format!("{}/api/embeddings", OLLAMA_BASE_URL))
        .json(&request)
        .send()
        .await
        .map_err(|e| crate::error::Error::Other(format!("Ollama request failed: {}", e)))?;
    
    if !response.status().is_success() {
        return Err(crate::error::Error::Other(
            format!("Ollama returned error: {}", response.status())
        ));
    }
    
    let json: serde_json::Value = response.json().await
        .map_err(|e| crate::error::Error::Other(format!("Failed to parse Ollama response: {}", e)))?;
    
    let embedding = json["embedding"]
        .as_array()
        .ok_or_else(|| crate::error::Error::Other("Invalid embedding format from Ollama".to_string()))?
        .iter()
        .filter_map(|v| v.as_f64().map(|f| f as f32))
        .collect::<Vec<f32>>();
    
    if embedding.len() != EMBEDDING_DIMENSIONS {
        return Err(crate::error::Error::Other(
            format!("Expected {} dimensions, got {}", EMBEDDING_DIMENSIONS, embedding.len())
        ));
    }
    
    Ok(embedding)
}

/// Compute SHA256 hash of content for change detection.
fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Serialize embedding vector to bytes for SQLite storage.
fn serialize_embedding(embedding: &[f32]) -> Vec<u8> {
    embedding.iter()
        .flat_map(|f| f.to_le_bytes())
        .collect()
}

/// Deserialize embedding vector from bytes.
fn deserialize_embedding(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4)
        .map(|chunk| {
            let arr: [u8; 4] = chunk.try_into().unwrap_or([0; 4]);
            f32::from_le_bytes(arr)
        })
        .collect()
}

/// Compute cosine similarity between two vectors.
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    
    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    
    dot_product / (norm_a * norm_b)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_hash_content() {
        let h1 = hash_content("hello");
        let h2 = hash_content("hello");
        let h3 = hash_content("world");
        
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }
    
    #[test]
    fn test_embedding_serialization() {
        let original = vec![1.0f32, 2.0, 3.0, 4.0];
        let bytes = serialize_embedding(&original);
        let deserialized = deserialize_embedding(&bytes);
        
        assert_eq!(original, deserialized);
    }
    
    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0f32, 0.0, 0.0];
        let b = vec![1.0f32, 0.0, 0.0];
        let c = vec![0.0f32, 1.0, 0.0];
        
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);
        assert!(cosine_similarity(&a, &c).abs() < 0.001);
    }
}
