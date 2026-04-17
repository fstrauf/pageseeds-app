use async_trait::async_trait;
use crate::error::Result;
use crate::seo::keywords::{KeywordIdeasResult, KeywordDifficultyResult};
use crate::seo::intent::IntentClassification;

/// Unified interface for SEO data backends.
#[async_trait]
pub trait SeoDataProvider: Send + Sync {
    /// Generate keyword ideas (regular + question) for a seed keyword.
    async fn keyword_ideas(&self, keyword: &str, country: &str, search_engine: &str) -> Result<KeywordIdeasResult>;

    /// Get keyword difficulty + SERP overview.
    async fn keyword_difficulty(&self, keyword: &str, country: &str) -> Result<KeywordDifficultyResult>;

    /// Batch keyword difficulty for multiple keywords.
    async fn batch_keyword_difficulty(
        &self,
        keywords: &[String],
        country: &str,
    ) -> Result<Vec<KeywordDifficultyResult>>;

    /// Classify search intent for keywords.
    async fn search_intent(&self, keywords: &[String]) -> Result<Vec<IntentClassification>>;

    /// Provider name for display ("ahrefs" | "dataforseo").
    fn name(&self) -> &'static str;
}
