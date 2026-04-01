/// Typed contracts for the keyword research workflow.
///
/// These types enforce the data flow between steps:
///   Step 1 (agentic): seed_extraction → SeedExtractionOutput
///   Step 2 (deterministic): ahrefs_pipeline → KeywordPipelineOutput  
///   Step 3 (agentic): final_selection → ResearchFinalOutput

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Output from Step 1: research_seed_extraction
/// 
/// The LLM reads the project brief and extracts 3-4 research themes.
/// Contract: MUST return valid JSON with {"themes": [...]}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SeedExtractionOutput {
    /// 3-4 seed themes for Ahrefs keyword ideas API
    /// Each theme should be 1-3 words maximum
    pub themes: Vec<String>,
}

/// A scored keyword from the Ahrefs pipeline
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ScoredKeyword {
    /// The keyword phrase
    pub keyword: String,
    /// Monthly search volume (if available)
    pub volume: Option<i64>,
    /// Keyword difficulty score 0-100 (if available)
    pub kd: Option<f64>,
    /// Search intent classification (if available)
    pub intent: Option<String>,
    /// Top-ranking page traffic estimate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traffic: Option<f64>,
    /// Whether we have complete data for this keyword
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_data: Option<bool>,
}

/// Output from Step 2: research_ahrefs_pipeline
///
/// The deterministic step calls Ahrefs API for each theme,
/// deduplicates, fetches KD scores, and returns structured data.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct KeywordPipelineOutput {
    /// All keywords found with their scores
    pub keywords: Vec<ScoredKeyword>,
    /// The themes that were used for research
    pub themes: Vec<String>,
    /// Total candidates before filtering
    pub total_candidates: usize,
    /// Number of keywords with full data (KD + volume)
    pub with_data_count: usize,
}

/// A selected keyword candidate for final output
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SelectedKeyword {
    /// The keyword phrase
    pub keyword: String,
    /// Monthly search volume
    pub volume: i64,
    /// Keyword difficulty score 0-100
    pub difficulty: i64,
    /// Why this keyword was selected
    pub selection_reason: String,
    /// Recommended article title
    pub recommended_title: String,
}

/// A landing page candidate (for commercial research)
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct LandingPageCandidate {
    /// The keyword phrase
    pub keyword: String,
    /// Monthly search volume
    pub estimated_volume: i64,
    /// Keyword difficulty score 0-100
    pub estimated_kd: i64,
    /// Search intent (transactional/commercial)
    pub intent: String,
    /// Type of landing page
    pub landing_page_type: String, // alternative|use_case|category|comparison|feature
    /// Opportunity score (high/medium/low)
    pub opportunity_score: String,
    /// Why this keyword deserves a landing page
    pub opportunity_reason: String,
    /// Suggested landing page title
    pub proposed_title: String,
}

/// Output from Step 3: research_final_selection
///
/// The LLM selects the best candidates from the structured data.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ResearchFinalOutput {
    /// Selected informational keywords (for research_keywords task)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub results: Vec<SelectedKeyword>,
    /// Selected landing page candidates (for research_landing_pages task)
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub landing_page_candidates: Vec<LandingPageCandidate>,
}

impl ResearchFinalOutput {
    /// Create an empty output for initialization
    pub fn empty() -> Self {
        Self {
            results: Vec::new(),
            landing_page_candidates: Vec::new(),
        }
    }

    /// Check if this output has any results
    pub fn has_results(&self) -> bool {
        !self.results.is_empty() || !self.landing_page_candidates.is_empty()
    }

    /// Get count of total selected items
    pub fn total_selected(&self) -> usize {
        self.results.len() + self.landing_page_candidates.len()
    }
}

/// Helper to parse step output with clear error messages
pub fn parse_seed_extraction(json_str: &str) -> Result<SeedExtractionOutput, String> {
    serde_json::from_str(json_str)
        .map_err(|e| format!("Failed to parse seed extraction output: {}", e))
}

/// Helper to parse step output with clear error messages
pub fn parse_keyword_pipeline(json_str: &str) -> Result<KeywordPipelineOutput, String> {
    serde_json::from_str(json_str)
        .map_err(|e| format!("Failed to parse keyword pipeline output: {}", e))
}

/// Helper to parse step output with clear error messages
pub fn parse_research_final(json_str: &str) -> Result<ResearchFinalOutput, String> {
    serde_json::from_str(json_str)
        .map_err(|e| format!("Failed to parse research final output: {}", e))
}
