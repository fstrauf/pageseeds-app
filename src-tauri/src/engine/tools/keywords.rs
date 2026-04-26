/// Keyword research tools wrapping Ahrefs API
use super::{Tool, ToolResult};
use serde::Deserialize;
use serde_json::json;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;

use crate::config::env_resolver::EnvResolver;
use crate::seo::keywords::{get_keyword_ideas, get_keyword_difficulty};

/// Tool: Generate keyword ideas from a seed keyword
pub struct KeywordGeneratorTool;

#[derive(Debug, Deserialize)]
pub struct KeywordGeneratorArgs {
    /// Seed keyword (1-3 words recommended)
    pub keyword: String,
    /// Country code (default: "us")
    pub country: Option<String>,
    /// Search engine (default: "Google")
    pub search_engine: Option<String>,
}

impl Tool for KeywordGeneratorTool {
    fn name(&self) -> &str {
        "keyword_generator"
    }

    fn description(&self) -> &str {
        "Generate keyword ideas from a seed keyword using Ahrefs API. \
         Returns related keywords and question-based keywords with search volume estimates. \
         Best for: Expanding seed themes into keyword opportunities. \
         Note: Use sparingly (max 3 calls per research task)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
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
        })
    }

    fn execute(&self, params: Value) -> Pin<Box<dyn Future<Output = ToolResult> + Send + '_>> {
        Box::pin(async move {
            let args: KeywordGeneratorArgs = match serde_json::from_value(params) {
                Ok(a) => a,
                Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
            };

            // Get CAPSOLVER_API_KEY from environment
            // Note: This is a global lookup - in production we might want to pass project_path
            let env = EnvResolver::new(".").build_env(HashMap::new());
            let capsolver_key = match env.get("CAPSOLVER_API_KEY") {
                Some(k) if !k.is_empty() => k.clone(),
                _ => return ToolResult::error("CAPSOLVER_API_KEY not configured"),
            };

            let country = args.country.as_deref().unwrap_or("us");
            let search_engine = args.search_engine.as_deref().unwrap_or("Google");

            log::info!(
                "[KeywordGeneratorTool] Generating ideas for '{}' (country: {})",
                args.keyword, country
            );

            match get_keyword_ideas(&capsolver_key, &args.keyword, country, search_engine).await {
                Ok(result) => {
                    // Convert to JSON-friendly format
                    let ideas: Vec<Value> = result
                        .ideas
                        .into_iter()
                        .map(|i| {
                            json!({
                                "keyword": i.keyword,
                                "idea_type": i.idea_type,
                                "volume": i.volume,
                                "difficulty": i.difficulty,
                                "country": i.country,
                            })
                        })
                        .collect();

                    let question_ideas: Vec<Value> = result
                        .question_ideas
                        .into_iter()
                        .map(|i| {
                            json!({
                                "keyword": i.keyword,
                                "idea_type": i.idea_type,
                                "volume": i.volume,
                                "difficulty": i.difficulty,
                                "country": i.country,
                            })
                        })
                        .collect();

                    ToolResult::success(json!({
                        "keyword": result.keyword,
                        "country": result.country,
                        "search_engine": result.search_engine,
                        "ideas": ideas,
                        "question_ideas": question_ideas,
                    }))
                }
                Err(e) => {
                    log::error!("[KeywordGeneratorTool] Ahrefs API error: {}", e);
                    ToolResult::error(format!("Ahrefs API error: {}", e))
                }
            }
        })
    }
}

/// Tool: Check keyword difficulty for a specific keyword
pub struct KeywordDifficultyTool;

#[derive(Debug, Deserialize)]
pub struct KeywordDifficultyArgs {
    /// Keyword to analyze
    pub keyword: String,
    /// Country code (default: "us")
    pub country: Option<String>,
}

impl Tool for KeywordDifficultyTool {
    fn name(&self) -> &str {
        "keyword_difficulty"
    }

    fn description(&self) -> &str {
        "Check keyword difficulty (KD) score for a specific keyword using Ahrefs API. \
         Returns KD score (0-100), SERP overview, and top-ranking pages. \
         Best for: Validating keyword competitiveness before targeting. \
         Note: Use sparingly (max 10 calls per research task)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
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
        })
    }

    fn execute(&self, params: Value) -> Pin<Box<dyn Future<Output = ToolResult> + Send + '_>> {
        Box::pin(async move {
            let args: KeywordDifficultyArgs = match serde_json::from_value(params) {
                Ok(a) => a,
                Err(e) => return ToolResult::error(format!("Invalid parameters: {}", e)),
            };

            // Get CAPSOLVER_API_KEY
            let env = EnvResolver::new(".").build_env(HashMap::new());
            let capsolver_key = match env.get("CAPSOLVER_API_KEY") {
                Some(k) if !k.is_empty() => k.clone(),
                _ => return ToolResult::error("CAPSOLVER_API_KEY not configured"),
            };

            let country = args.country.as_deref().unwrap_or("us");

            log::info!(
                "[KeywordDifficultyTool] Checking difficulty for '{}' (country: {})",
                args.keyword, country
            );

            match get_keyword_difficulty(&capsolver_key, &args.keyword, country).await {
                Ok(result) => {
                    let serp: Vec<Value> = result
                        .serp
                        .into_iter()
                        .map(|s| {
                            json!({
                                "title": s.title,
                                "url": s.url,
                                "domain": s.domain,
                                "position": s.position,
                            })
                        })
                        .collect();

                    ToolResult::success(json!({
                        "keyword": result.keyword,
                        "difficulty": result.difficulty,
                        "shortage": result.shortage,
                        "country": country,
                        "last_update": result.last_update,
                        "serp": serp,
                    }))
                }
                Err(e) => {
                    log::error!("[KeywordDifficultyTool] Ahrefs API error: {}", e);
                    ToolResult::error(format!("Ahrefs API error: {}", e))
                }
            }
        })
    }
}
