//! Embedding provider abstraction — wraps rig's `EmbeddingModel` for
//! Ollama and OpenAI with enum dispatch.

use rig::{
    client::{EmbeddingsClient, Nothing, ProviderClient},
    embeddings::{Embedding, EmbeddingError, EmbeddingModel},
    providers::{ollama, openai},
};

/// Configured embedding backend.
///
/// Created from settings (embedding_provider, model, api_key, base_url).
/// Does not implement `EmbeddingModel` directly — callers pattern-match
/// to build `EmbeddingsBuilder` or `InMemoryVectorIndex` with the concrete
/// type, because `EmbeddingModel` has associated types that prevent boxing.
#[derive(Clone)]
#[allow(dead_code)]
pub enum EmbeddingBackend {
    Ollama {
        client: ollama::Client,
        model: String,
        ndims: usize,
    },
    OpenAi {
        client: openai::Client,
        model: String,
    },
}

#[allow(dead_code)]
impl EmbeddingBackend {
    /// Default Ollama backend for local embeddings.
    pub fn default_ollama() -> Self {
        let client = ollama::Client::from_val(Nothing);
        Self::Ollama {
            client,
            model: "nomic-embed-text".to_string(),
            ndims: 768,
        }
    }

    /// Create an Ollama backend with a custom base URL.
    pub fn ollama(base_url: &str, model: &str, ndims: usize) -> Result<Self, String> {
        let client = ollama::Client::builder()
            .api_key(Nothing)
            .base_url(base_url)
            .build()
            .map_err(|e| format!("Failed to create Ollama client: {}", e))?;
        Ok(Self::Ollama {
            client,
            model: model.to_string(),
            ndims,
        })
    }

    /// Create an OpenAI backend.
    pub fn openai(api_key: &str, model: &str) -> Result<Self, String> {
        let client = openai::Client::new(api_key)
            .map_err(|e| format!("Failed to create OpenAI client: {}", e))?;
        Ok(Self::OpenAi {
            client,
            model: model.to_string(),
        })
    }

    /// Number of dimensions for this backend's model.
    pub fn ndims(&self) -> usize {
        match self {
            Self::Ollama { ndims, .. } => *ndims,
            Self::OpenAi { client, model } => {
                let m = client.embedding_model(model.clone());
                m.ndims()
            }
        }
    }

    /// Embed a single text.
    pub async fn embed_text(&self, text: &str) -> Result<Embedding, EmbeddingError> {
        match self {
            Self::Ollama {
                client,
                model,
                ndims,
            } => {
                let m = client.embedding_model_with_ndims(model.clone(), *ndims);
                m.embed_text(text).await
            }
            Self::OpenAi { client, model } => {
                let m = client.embedding_model(model.clone());
                m.embed_text(text).await
            }
        }
    }

    /// Embed multiple texts.
    pub async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Embedding>, EmbeddingError> {
        match self {
            Self::Ollama {
                client,
                model,
                ndims,
            } => {
                let m = client.embedding_model_with_ndims(model.clone(), *ndims);
                m.embed_texts(texts).await
            }
            Self::OpenAi { client, model } => {
                let m = client.embedding_model(model.clone());
                m.embed_texts(texts).await
            }
        }
    }
}

// ─── Health checks ───────────────────────────────────────────────────────────

#[allow(dead_code)]
/// Check if the Ollama server is reachable.
pub async fn check_ollama_health(base_url: Option<&str>) -> Result<bool, String> {
    let url = base_url.unwrap_or("http://localhost:11434");
    let client = reqwest::Client::new();
    match client.get(format!("{}/api/tags", url)).send().await {
        Ok(resp) => Ok(resp.status().is_success()),
        Err(_) => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_ollama_ndims() {
        let backend = EmbeddingBackend::default_ollama();
        assert_eq!(backend.ndims(), 768);
    }

    #[test]
    fn test_ollama_backend_creation() {
        let backend = EmbeddingBackend::ollama("http://localhost:11434", "nomic-embed-text", 768);
        assert!(backend.is_ok());
        let backend = backend.unwrap();
        assert_eq!(backend.ndims(), 768);
    }
}

#[allow(dead_code)]
/// Check if the OpenAI API is reachable.
pub async fn check_openai_health(api_key: &str) -> Result<bool, String> {
    let client = reqwest::Client::new();
    match client
        .get("https://api.openai.com/v1/models")
        .bearer_auth(api_key)
        .send()
        .await
    {
        Ok(resp) => Ok(resp.status().is_success()),
        Err(_) => Ok(false),
    }
}
