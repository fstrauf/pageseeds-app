/// Semantic search for skills using vector embeddings.
///
/// Replaces manual Ollama HTTP + raw f32 blob math with rig's
/// `EmbeddingModel`, `EmbeddingsBuilder`, and `VectorStoreIndex`.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use rig::{
    client::EmbeddingsClient,
    embeddings::{Embedding, EmbeddingsBuilder},
    vector_store::VectorStoreIndex,
    vector_store::in_memory_store::InMemoryVectorStore,
    vector_store::request::VectorSearchRequest,
    OneOrMany,
};

use crate::engine::skills::Skill;
use crate::error::Result;
use crate::rig::embeddings::EmbeddingBackend;

// ─── Configuration ───────────────────────────────────────────────────────────

const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";

// ─── Types ───────────────────────────────────────────────────────────────────

/// A skill with similarity score from semantic search.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredSkill {
    pub skill: Skill,
    pub score: f64,
}

/// Status of the embedding service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingStatus {
    pub available: bool,
    pub model: String,
    pub error: Option<String>,
}

/// JSON-serializable embedding for SQLite storage.
/// Replaces raw f32 byte blobs with structured JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredEmbedding {
    document: String,
    vec: Vec<f64>,
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Check if the embedding backend is available.
pub async fn check_status() -> EmbeddingStatus {
    match crate::rig::embeddings::check_ollama_health(None).await {
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
/// Uses rig's `EmbeddingsBuilder` to batch-generate embeddings via the
/// configured `EmbeddingBackend`. Results are persisted as JSON in SQLite
/// (no manual f32 byte serialization).
pub async fn index_skills(
    conn: &Connection,
    project_id: &str,
    skills: &[Skill],
) -> Result<usize> {
    if skills.is_empty() {
        return Ok(0);
    }

    let backend = EmbeddingBackend::default_ollama();

    // Check backend health
    let health = crate::rig::embeddings::check_ollama_health(None).await
        .map_err(|e| crate::error::Error::Other(e))?;
    if !health {
        return Err(crate::error::Error::Other(
            "Ollama not available. Run: ollama pull nomic-embed-text && ollama serve".to_string(),
        ));
    }

    // Generate embeddings using rig's EmbeddingsBuilder
    let embeddings = match &backend {
        EmbeddingBackend::Ollama { client, model, ndims } => {
            let m = client.embedding_model_with_ndims(model.clone(), *ndims);
            EmbeddingsBuilder::new(m)
                .documents(skills.to_vec())
                .map_err(|e| crate::error::Error::Other(format!("Embed error: {}", e)))?
                .build()
                .await
                .map_err(|e| crate::error::Error::Other(format!("Embedding generation failed: {}", e)))?
        }
        EmbeddingBackend::OpenAi { client, model } => {
            let m = client.embedding_model(model.clone());
            EmbeddingsBuilder::new(m)
                .documents(skills.to_vec())
                .map_err(|e| crate::error::Error::Other(format!("Embed error: {}", e)))?
                .build()
                .await
                .map_err(|e| crate::error::Error::Other(format!("Embedding generation failed: {}", e)))?
        }
    };

    let mut indexed = 0;
    for (skill, embedding) in embeddings.into_iter() {
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

        // Store embedding as JSON (no raw f32 byte serialization)
        let first = embedding.first_ref();
        let stored = StoredEmbedding {
            document: first.document.clone(),
            vec: first.vec.clone(),
        };
        let json = serde_json::to_string(&stored)
            .map_err(|e| crate::error::Error::Other(format!("JSON serialization failed: {}", e)))?;

        let now = chrono::Utc::now().to_rfc3339();

        conn.execute(
            r#"INSERT INTO skill_embeddings (skill_name, project_id, content_hash, embedding_json, model_name, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
               ON CONFLICT(skill_name) DO UPDATE SET
                   content_hash = excluded.content_hash,
                   embedding_json = excluded.embedding_json,
                   model_name = excluded.model_name,
                   updated_at = excluded.updated_at"#,
            rusqlite::params![
                &skill.name,
                project_id,
                &content_hash,
                &json,
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
/// Loads embeddings from SQLite JSON, reconstructs `Embedding` objects,
/// builds an `InMemoryVectorStore`, and queries via `VectorStoreIndex`.
pub async fn search_skills(
    conn: &Connection,
    project_id: &str,
    query: &str,
    limit: usize,
    all_skills: &[Skill],
) -> Result<Vec<ScoredSkill>> {
    // Check if we have any indexed skills for this project
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM skill_embeddings WHERE project_id = ?1",
        [project_id],
        |row| row.get(0),
    )?;

    if count == 0 {
        // No indexed skills — fall back to returning all skills un-scored
        return Ok(all_skills
            .iter()
            .map(|s| ScoredSkill {
                skill: s.clone(),
                score: 0.0,
            })
            .collect());
    }

    // Load all indexed embeddings for this project
    let mut stmt = conn.prepare(
        "SELECT skill_name, embedding_json FROM skill_embeddings WHERE project_id = ?1"
    )?;

    let rows = stmt.query_map([project_id], |row| {
        let name: String = row.get(0)?;
        let json: String = row.get(1)?;
        let stored: StoredEmbedding = serde_json::from_str(&json)
            .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e)))?;
        let embedding = Embedding {
            document: stored.document,
            vec: stored.vec,
        };
        Ok((name, embedding))
    })?;

    // Build InMemoryVectorStore from loaded embeddings
    let mut store = InMemoryVectorStore::<Skill>::default();
    let mut docs: Vec<(String, Skill, OneOrMany<Embedding>)> = Vec::new();

    for row in rows {
        let (name, embedding) = row?;
        if let Some(skill) = all_skills.iter().find(|s| s.name == name) {
            docs.push((name.clone(), skill.clone(), OneOrMany::one(embedding)));
        }
    }

    store.add_documents_with_ids(docs);

    // Create backend and index, then search
    let backend = EmbeddingBackend::default_ollama();

    let results = match &backend {
        EmbeddingBackend::Ollama { client, model, ndims } => {
            let m = client.embedding_model_with_ndims(model.clone(), *ndims);
            let index = store.index(m);
            let req = VectorSearchRequest::builder()
                .query(query)
                .samples(limit as u64)
                .build()
                .map_err(|e| crate::error::Error::Other(format!("Search request error: {}", e)))?;
            index.top_n::<Skill>(req).await
                .map_err(|e| crate::error::Error::Other(format!("Search failed: {}", e)))?
        }
        EmbeddingBackend::OpenAi { client, model } => {
            let m = client.embedding_model(model.clone());
            let index = store.index(m);
            let req = VectorSearchRequest::builder()
                .query(query)
                .samples(limit as u64)
                .build()
                .map_err(|e| crate::error::Error::Other(format!("Search request error: {}", e)))?;
            index.top_n::<Skill>(req).await
                .map_err(|e| crate::error::Error::Other(format!("Search failed: {}", e)))?
        }
    };

    Ok(results
        .into_iter()
        .map(|(score, _id, skill)| ScoredSkill { skill, score })
        .collect())
}

/// Remove all skill embeddings for a project (e.g. when re-indexing from scratch).
pub fn clear_index(conn: &Connection, project_id: &str) -> Result<usize> {
    let count = conn.execute(
        "DELETE FROM skill_embeddings WHERE project_id = ?1",
        [project_id],
    )?;
    Ok(count)
}

// ─── Internal Helpers ────────────────────────────────────────────────────────

/// Compute SHA256 hash of content for change detection.
fn hash_content(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    format!("{:x}", hasher.finalize())
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
    fn test_stored_embedding_roundtrip() {
        let original = StoredEmbedding {
            document: "test doc".to_string(),
            vec: vec![0.1, 0.2, 0.3, 0.4],
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: StoredEmbedding = serde_json::from_str(&json).unwrap();

        assert_eq!(original.document, deserialized.document);
        assert_eq!(original.vec, deserialized.vec);
    }
}
