//! Keyword research tools using rig's native `Tool` trait.
//!
//! These tools wrap the Ahrefs API (via CapSolver) and can be attached to a
//! rig `Agent` for multi-turn tool-calling keyword research.

use rig::completion::ToolDefinition;
use rig::tool::Tool;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::env_resolver::EnvResolver;
use crate::seo::keywords::{get_keyword_ideas, get_keyword_difficulty};

// ─── Keyword Generator Tool ─────────────────────────────────────────────────

/// Arguments for the keyword_generator tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct KeywordGeneratorArgs {
    /// Seed keyword (1-3 words recommended)
    pub keyword: String,
    /// Country code (default: "us")
    #[serde(default)]
    pub country: Option<String>,
    /// Search engine (default: "Google")
    #[serde(default)]
    pub search_engine: Option<String>,
}

/// Output for the keyword_generator tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct KeywordGeneratorOutput {
    pub keyword: String,
    pub country: String,
    pub search_engine: String,
    pub ideas: Vec<KeywordIdea>,
    pub question_ideas: Vec<KeywordIdea>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct KeywordIdea {
    pub keyword: String,
    pub idea_type: String,
    pub volume: Option<String>,
    pub difficulty: Option<String>,
    pub country: Option<String>,
}

/// Error type for keyword tools.
#[derive(Debug, thiserror::Error)]
pub enum KeywordToolError {
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Configuration error: {0}")]
    ConfigError(String),
}

/// Generate keyword ideas from a seed keyword using Ahrefs API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordGeneratorTool;

impl Tool for KeywordGeneratorTool {
    const NAME: &'static str = "keyword_generator";
    type Error = KeywordToolError;
    type Args = KeywordGeneratorArgs;
    type Output = KeywordGeneratorOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Generate keyword ideas from a seed keyword using Ahrefs API. \
                Returns related keywords and question-based keywords with search volume estimates. \
                Best for: Expanding seed themes into keyword opportunities. \
                Note: Use sparingly (max 3 calls per research task)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "keyword": {
                        "type": "string",
                        "description": "Seed keyword to expand (1-3 words recommended, e.g., 'coffee roaster')"
                    },
                    "country": {
                        "type": "string",
                        "description": "Country code for search volume (default: 'us')",
                        "default": "us"
                    },
                    "search_engine": {
                        "type": "string",
                        "description": "Search engine (default: 'Google')",
                        "default": "Google"
                    }
                },
                "required": ["keyword"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let env = EnvResolver::new(".").build_env(std::collections::HashMap::new());
        let capsolver_key = env
            .get("CAPSOLVER_API_KEY")
            .filter(|k| !k.is_empty())
            .cloned()
            .ok_or_else(|| KeywordToolError::ConfigError("CAPSOLVER_API_KEY not configured".to_string()))?;

        let country = args.country.as_deref().unwrap_or("us");
        let search_engine = args.search_engine.as_deref().unwrap_or("Google");

        log::info!(
            "[KeywordGeneratorTool] Generating ideas for '{}' (country: {})",
            args.keyword,
            country
        );

        let result = get_keyword_ideas(&capsolver_key, &args.keyword, country, search_engine)
            .await
            .map_err(|e| KeywordToolError::ApiError(e.to_string()))?;

        let ideas = result
            .ideas
            .into_iter()
            .map(|i| KeywordIdea {
                keyword: i.keyword,
                idea_type: i.idea_type,
                volume: i.volume,
                difficulty: i.difficulty,
                country: i.country,
            })
            .collect();

        let question_ideas = result
            .question_ideas
            .into_iter()
            .map(|i| KeywordIdea {
                keyword: i.keyword,
                idea_type: i.idea_type,
                volume: i.volume,
                difficulty: i.difficulty,
                country: i.country,
            })
            .collect();

        Ok(KeywordGeneratorOutput {
            keyword: result.keyword,
            country: result.country,
            search_engine: result.search_engine,
            ideas,
            question_ideas,
        })
    }
}

// ─── Keyword Difficulty Tool ────────────────────────────────────────────────

/// Arguments for the keyword_difficulty tool.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct KeywordDifficultyArgs {
    /// Keyword to analyze
    pub keyword: String,
    /// Country code (default: "us")
    #[serde(default)]
    pub country: Option<String>,
}

/// Output for the keyword_difficulty tool.
#[derive(Debug, Serialize, JsonSchema)]
pub struct KeywordDifficultyOutput {
    pub keyword: String,
    pub difficulty: Option<f64>,
    pub shortage: Option<f64>,
    pub country: String,
    pub last_update: String,
    pub serp: Vec<SerpEntry>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct SerpEntry {
    pub title: String,
    pub url: String,
    pub domain: String,
    pub position: i64,
}

/// Check keyword difficulty (KD) score for a specific keyword using Ahrefs API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordDifficultyTool;

impl Tool for KeywordDifficultyTool {
    const NAME: &'static str = "keyword_difficulty";
    type Error = KeywordToolError;
    type Args = KeywordDifficultyArgs;
    type Output = KeywordDifficultyOutput;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Check keyword difficulty (KD) score for a specific keyword using Ahrefs API. \
                Returns KD score (0-100), SERP overview, and top-ranking pages. \
                Best for: Validating keyword competitiveness before targeting. \
                Note: Use sparingly (max 10 calls per research task)."
                .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "keyword": {
                        "type": "string",
                        "description": "Keyword to check difficulty for (e.g., 'best home coffee roaster')"
                    },
                    "country": {
                        "type": "string",
                        "description": "Country code (default: 'us')",
                        "default": "us"
                    }
                },
                "required": ["keyword"]
            }),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let env = EnvResolver::new(".").build_env(std::collections::HashMap::new());
        let capsolver_key = env
            .get("CAPSOLVER_API_KEY")
            .filter(|k| !k.is_empty())
            .cloned()
            .ok_or_else(|| KeywordToolError::ConfigError("CAPSOLVER_API_KEY not configured".to_string()))?;

        let country = args.country.as_deref().unwrap_or("us");

        log::info!(
            "[KeywordDifficultyTool] Checking difficulty for '{}' (country: {})",
            args.keyword,
            country
        );

        let result = get_keyword_difficulty(&capsolver_key, &args.keyword, country)
            .await
            .map_err(|e| KeywordToolError::ApiError(e.to_string()))?;

        let serp = result
            .serp
            .into_iter()
            .map(|s| SerpEntry {
                title: s.title,
                url: s.url,
                domain: s.domain,
                position: s.position,
            })
            .collect();

        Ok(KeywordDifficultyOutput {
            keyword: result.keyword,
            difficulty: result.difficulty,
            shortage: result.shortage,
            country: country.to_string(),
            last_update: result.last_update,
            serp,
        })
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Build a boxed tool set suitable for passing to `Agent::builder().tools(...)`.
pub fn boxed_keyword_tools() -> Vec<Box<dyn rig::tool::ToolDyn>> {
    vec![
        Box::new(KeywordGeneratorTool),
        Box::new(KeywordDifficultyTool),
    ]
}
