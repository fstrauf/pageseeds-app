use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// All known workflow step kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepKind {
    Deterministic,
    Agentic,
    Manual,
    Normalizer,
    ClusterLinkScan,
    ClusterLinkStrategy,
    ClusterLinkApply,
    ContentReviewRecommend,
    ContentReviewApplyExecute,
    KeywordResearchNative,
    ResearchFinalSelection,
    LandingPageSpecWrite,
    RedditConfigParse,
    RedditSearch,
    RedditEnrich,
    RedditFetchResults,
    ContentSync,
    GscSyncArticles,
    GscSummarise,
    IndexingFixContext,
    IndexingFixApply,
    ContentAudit,
    CollectGscInspect,
    IndexingDiagnosticsRun,
    GscInvestigateAgentic,
    SocialCollectSources,
    SocialLoadTemplates,
    SocialGeneratePosts,
    SocialBuildVisuals,
    SocialSaveCampaign,
    SocialRegenerateSingle,
    SocialRebuildVisual,
    SocialUpdatePost,
    SocialDesignTemplate,
    SocialSaveTemplate,
    CoverageLoadArticles,
    CoverageClusterAnalysis,
    CoverageSave,
    RedditPostReply,
    SocialExtractArticle,
    /// Fetch Google Autocomplete suggestions per theme (deterministic).
    ResearchAutocomplete,
    /// LLM filters autocomplete suggestions for domain relevance (agentic).
    ResearchSeedValidation,
    /// Fallback for unknown strings during deserialization.
    Unknown,
}

impl StepKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::Agentic => "agentic",
            Self::Manual => "manual",
            Self::Normalizer => "normalizer",
            Self::ClusterLinkScan => "cluster_link_scan",
            Self::ClusterLinkStrategy => "cluster_link_strategy",
            Self::ClusterLinkApply => "cluster_link_apply",
            Self::ContentReviewRecommend => "content_review_recommend",
            Self::ContentReviewApplyExecute => "content_review_apply_execute",
            Self::KeywordResearchNative => "keyword_research_native",
            Self::ResearchFinalSelection => "research_final_selection",
            Self::LandingPageSpecWrite => "landing_page_spec_write",
            Self::RedditConfigParse => "reddit_config_parse",
            Self::RedditSearch => "reddit_search",
            Self::RedditEnrich => "reddit_enrich",
            Self::RedditFetchResults => "reddit_fetch_results",
            Self::ContentSync => "content_sync",
            Self::GscSyncArticles => "gsc_sync_articles",
            Self::GscSummarise => "gsc_summarise",
            Self::IndexingFixContext => "indexing_fix_context",
            Self::IndexingFixApply => "indexing_fix_apply",
            Self::ContentAudit => "content_audit",
            Self::CollectGscInspect => "collect_gsc_inspect",
            Self::IndexingDiagnosticsRun => "indexing_diagnostics_run",
            Self::GscInvestigateAgentic => "gsc_investigate_agentic",
            Self::SocialCollectSources => "social_collect_sources",
            Self::SocialLoadTemplates => "social_load_templates",
            Self::SocialGeneratePosts => "social_generate_posts",
            Self::SocialBuildVisuals => "social_build_visuals",
            Self::SocialSaveCampaign => "social_save_campaign",
            Self::SocialRegenerateSingle => "social_regenerate_single",
            Self::SocialRebuildVisual => "social_rebuild_visual",
            Self::SocialUpdatePost => "social_update_post",
            Self::SocialDesignTemplate => "social_design_template",
            Self::SocialSaveTemplate => "social_save_template",
            Self::CoverageLoadArticles => "coverage_load_articles",
            Self::CoverageClusterAnalysis => "coverage_cluster_analysis",
            Self::CoverageSave => "coverage_save",
            Self::RedditPostReply => "reddit_post_reply",
            Self::SocialExtractArticle => "social_extract_article",
            Self::ResearchAutocomplete => "research_autocomplete",
            Self::ResearchSeedValidation => "research_seed_validation",
            Self::Unknown => "unknown",
        }
    }
}

impl AsRef<str> for StepKind {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for StepKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for StepKind {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "deterministic" => Ok(Self::Deterministic),
            "agentic" => Ok(Self::Agentic),
            "manual" => Ok(Self::Manual),
            "normalizer" => Ok(Self::Normalizer),
            "cluster_link_scan" => Ok(Self::ClusterLinkScan),
            "cluster_link_strategy" => Ok(Self::ClusterLinkStrategy),
            "cluster_link_apply" => Ok(Self::ClusterLinkApply),
            "content_review_recommend" => Ok(Self::ContentReviewRecommend),
            "content_review_apply_execute" => Ok(Self::ContentReviewApplyExecute),
            "keyword_research_native" => Ok(Self::KeywordResearchNative),
            "research_final_selection" => Ok(Self::ResearchFinalSelection),
            "landing_page_spec_write" => Ok(Self::LandingPageSpecWrite),
            "reddit_config_parse" => Ok(Self::RedditConfigParse),
            "reddit_search" => Ok(Self::RedditSearch),
            "reddit_enrich" => Ok(Self::RedditEnrich),
            "reddit_fetch_results" => Ok(Self::RedditFetchResults),
            "content_sync" => Ok(Self::ContentSync),
            "gsc_sync_articles" => Ok(Self::GscSyncArticles),
            "gsc_summarise" => Ok(Self::GscSummarise),
            "indexing_fix_context" => Ok(Self::IndexingFixContext),
            "indexing_fix_apply" => Ok(Self::IndexingFixApply),
            "content_audit" => Ok(Self::ContentAudit),
            "collect_gsc_inspect" => Ok(Self::CollectGscInspect),
            "indexing_diagnostics_run" => Ok(Self::IndexingDiagnosticsRun),
            "gsc_investigate_agentic" => Ok(Self::GscInvestigateAgentic),
            "social_collect_sources" => Ok(Self::SocialCollectSources),
            "social_load_templates" => Ok(Self::SocialLoadTemplates),
            "social_generate_posts" => Ok(Self::SocialGeneratePosts),
            "social_build_visuals" => Ok(Self::SocialBuildVisuals),
            "social_save_campaign" => Ok(Self::SocialSaveCampaign),
            "social_regenerate_single" => Ok(Self::SocialRegenerateSingle),
            "social_rebuild_visual" => Ok(Self::SocialRebuildVisual),
            "social_update_post" => Ok(Self::SocialUpdatePost),
            "social_design_template" => Ok(Self::SocialDesignTemplate),
            "social_save_template" => Ok(Self::SocialSaveTemplate),
            "coverage_load_articles" => Ok(Self::CoverageLoadArticles),
            "coverage_cluster_analysis" => Ok(Self::CoverageClusterAnalysis),
            "coverage_save" => Ok(Self::CoverageSave),
            "reddit_post_reply" => Ok(Self::RedditPostReply),
            "social_extract_article" => Ok(Self::SocialExtractArticle),
            "research_autocomplete" => Ok(Self::ResearchAutocomplete),
            "research_seed_validation" => Ok(Self::ResearchSeedValidation),
            _ => Err(()),
        }
    }
}

impl Serialize for StepKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StepKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(StepKind::from_str(&s).unwrap_or(StepKind::Unknown))
    }
}
