use crate::error::Result;
use crate::seo::intent::{classify_batch_by_pattern, IntentClassification};
use crate::seo::keywords::{
    get_keyword_difficulty, get_keyword_ideas, KeywordDifficultyResult, KeywordIdeasResult,
    SerpFeaturesResult,
};
use crate::seo::provider::SeoDataProvider;
use async_trait::async_trait;

/// Ahrefs SEO data provider implementation.
pub struct AhrefsProvider {
    capsolver_key: String,
}

impl AhrefsProvider {
    pub fn new(capsolver_key: String) -> Self {
        Self { capsolver_key }
    }
}

#[async_trait]
impl SeoDataProvider for AhrefsProvider {
    async fn keyword_ideas(
        &self,
        keyword: &str,
        country: &str,
        search_engine: &str,
    ) -> Result<KeywordIdeasResult> {
        get_keyword_ideas(&self.capsolver_key, keyword, country, search_engine).await
    }

    async fn keyword_difficulty(
        &self,
        keyword: &str,
        country: &str,
    ) -> Result<KeywordDifficultyResult> {
        get_keyword_difficulty(&self.capsolver_key, keyword, country).await
    }

    async fn batch_keyword_difficulty(
        &self,
        keywords: &[String],
        country: &str,
    ) -> Result<Vec<KeywordDifficultyResult>> {
        let mut results = Vec::with_capacity(keywords.len());
        for keyword in keywords {
            match self.keyword_difficulty(keyword, country).await {
                Ok(result) => results.push(result),
                Err(e) => {
                    log::warn!(
                        "[AhrefsProvider] Failed to get difficulty for '{}': {}",
                        keyword,
                        e
                    );
                    // Push a placeholder with None difficulty to maintain index alignment
                    results.push(KeywordDifficultyResult {
                        keyword: keyword.clone(),
                        difficulty: None,
                        shortage: None,
                        last_update: String::new(),
                        serp: vec![],
                    });
                }
            }
        }
        Ok(results)
    }

    async fn search_intent(&self, keywords: &[String]) -> Result<Vec<IntentClassification>> {
        // Ahrefs doesn't have an intent API, so we use pattern matching
        Ok(classify_batch_by_pattern(keywords))
    }

    async fn serp_features(&self, _keyword: &str, _country: &str) -> Result<SerpFeaturesResult> {
        // SERP feature analysis requires DataForSEO. The Ahrefs scraper path
        // only returns organic results and discards SERP features.
        Err(crate::error::Error::Other(
            "SERP feature analysis requires the DataForSEO provider. \
             Switch the project's seo_provider setting to 'dataforseo'."
                .to_string(),
        ))
    }

    fn name(&self) -> &'static str {
        "ahrefs"
    }
}
