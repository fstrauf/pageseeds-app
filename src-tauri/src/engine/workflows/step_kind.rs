use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// All known workflow step kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StepKind {
    Deterministic,
    Agentic,
    Manual,
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
    FormatValidation,
    FormatFix,
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
    /// Rig Agent with native tool calling for keyword research (experimental).
    KeywordResearchToolAgent,
    /// Build structured context for CTR audit (deterministic data collection).
    CtrBuildContext,
    /// Agentic CTR analysis — titles, meta, snippets, FAQ (agentic).
    CtrAnalyze,
    /// Build structured context for cannibalization audit (TF-IDF + data formatting).
    CanBuildContext,
    /// Agentic cannibalization strategy — merges, hubs, territories (agentic).
    CanAnalyze,
    /// Typed structured extraction of a CtrFixPatch from an LLM.
    CtrFixGenerate,
    /// Deterministic application of agent-generated CTR fix patch.
    CtrFixApply,
    /// Deterministic verification that applied CTR fixes meet health thresholds.
    CtrVerifyFix,
    /// Deterministic rendered SERP audit — fetch live HTML, compare with source.
    CtrRenderedSerpAudit,
    /// Deterministic detection of repeated site-wide title template patterns.
    CtrTemplateDetect,
    /// Agentic/manual step: plan framework-aware title template fix.
    CtrTemplatePlan,
    /// Deterministic verification that rendered sample pages pass title checks.
    CtrTemplateVerifyRender,
    /// Deterministic detection of articles with source FAQ but missing rendered JSON-LD.
    CtrSchemaDetect,
    /// Deterministic verification that rendered pages contain FAQPage JSON-LD.
    CtrSchemaVerifyRender,
    /// Deterministic comparison of before/after CTR metrics for outcome tracking.
    CtrOutcomeCompare,
    /// Deterministic generation of CTR outcome report artifact.
    CtrOutcomeReport,
    /// Load approved merge plan from strategy artifact.
    MergeLoadPlan,
    /// Preflight checks before merging (files exist, no cycles, keeper indexable).
    MergePreflight,
    /// Extract unique sections from redirect pages.
    MergeExtractSections,
    /// Agentic step: draft structured ContentMergePatch JSON.
    MergeDraftPatch,
    /// Apply structured merge patch to keeper file, snapshot original.
    MergeApplyPatch,
    /// Generate redirect rules (generic CSV first, platform adapters later).
    MergeGenerateRedirects,
    /// Validate merged keeper and redirect map.
    MergeValidateOutput,
    /// Load approved hub recommendation from strategy artifact.
    HubLoadRecommendation,
    /// Gather spoke metadata, excerpts, and GSC metrics into HubBrief.
    HubBuildBrief,
    /// Agentic: generate structured hub outline and linking strategy from HubBrief.
    HubOutline,
    /// Agentic: generate full MDX hub page from HubBrief using hub-write skill.
    HubWrite,
    /// Write MDX file to content dir, register in SQLite and articles.json.
    HubApplyDraft,
    /// Add hub↔spoke Related Articles links.
    HubApplyLinks,
    /// Validate hub page: frontmatter, H1, word count ≥1500, spoke links.
    HubValidate,
    /// Load approved territory recommendation from strategy artifact.
    TerritoryLoadRecommendation,
    /// Gather existing articles, excerpts, and GSC metrics for territory context.
    TerritoryBuildContext,
    /// Agentic: generate TerritoryStrategy JSON from context.
    TerritoryStrategy,
    /// Write territory strategy JSON to automation dir.
    TerritoryApply,
    /// Sanitize content: rename .md → .mdx, repair paths, validate frontmatter (read-only report).
    SanitizeContent,
    /// Fallback for unknown strings during deserialization.
    Unknown,
}

impl StepKind {
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Deterministic => "deterministic",
            Self::Agentic => "agentic",
            Self::Manual => "manual",
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
            Self::FormatValidation => "format_validation",
            Self::FormatFix => "format_fix",
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
            Self::KeywordResearchToolAgent => "keyword_research_tool_agent",
            Self::CtrBuildContext => "ctr_build_context",
            Self::CtrAnalyze => "ctr_analyze",
            Self::CanBuildContext => "can_build_context",
            Self::CanAnalyze => "can_analyze",
            Self::CtrFixGenerate => "ctr_fix_generate",
            Self::CtrFixApply => "ctr_fix_apply",
            Self::CtrVerifyFix => "ctr_verify_fix",
            Self::CtrRenderedSerpAudit => "ctr_rendered_serp_audit",
            Self::CtrTemplateDetect => "ctr_template_detect",
            Self::CtrTemplatePlan => "ctr_template_plan",
            Self::CtrTemplateVerifyRender => "ctr_template_verify_render",
            Self::CtrSchemaDetect => "ctr_schema_detect",
            Self::CtrSchemaVerifyRender => "ctr_schema_verify_render",
            Self::CtrOutcomeCompare => "ctr_outcome_compare",
            Self::CtrOutcomeReport => "ctr_outcome_report",
            Self::MergeLoadPlan => "merge_load_plan",
            Self::MergePreflight => "merge_preflight",
            Self::MergeExtractSections => "merge_extract_sections",
            Self::MergeDraftPatch => "merge_draft_patch",
            Self::MergeApplyPatch => "merge_apply_patch",
            Self::MergeGenerateRedirects => "merge_generate_redirects",
            Self::MergeValidateOutput => "merge_validate_output",
            Self::HubLoadRecommendation => "hub_load_recommendation",
            Self::HubBuildBrief => "hub_build_brief",
            Self::HubOutline => "hub_outline",
            Self::HubWrite => "hub_write",
            Self::HubApplyDraft => "hub_apply_draft",
            Self::HubApplyLinks => "hub_apply_links",
            Self::HubValidate => "hub_validate",
            Self::TerritoryLoadRecommendation => "territory_load_recommendation",
            Self::TerritoryBuildContext => "territory_build_context",
            Self::TerritoryStrategy => "territory_strategy",
            Self::TerritoryApply => "territory_apply",
            Self::SanitizeContent => "sanitize_content",
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
            "format_validation" => Ok(Self::FormatValidation),
            "format_fix" => Ok(Self::FormatFix),
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
            "keyword_research_tool_agent" => Ok(Self::KeywordResearchToolAgent),
            "ctr_build_context" => Ok(Self::CtrBuildContext),
            "ctr_analyze" => Ok(Self::CtrAnalyze),
            "can_build_context" => Ok(Self::CanBuildContext),
            "can_analyze" => Ok(Self::CanAnalyze),
            "ctr_fix_generate" => Ok(Self::CtrFixGenerate),
            "ctr_fix_apply" => Ok(Self::CtrFixApply),
            "ctr_verify_fix" => Ok(Self::CtrVerifyFix),
            "ctr_rendered_serp_audit" => Ok(Self::CtrRenderedSerpAudit),
            "ctr_template_detect" => Ok(Self::CtrTemplateDetect),
            "ctr_template_plan" => Ok(Self::CtrTemplatePlan),
            "ctr_template_verify_render" => Ok(Self::CtrTemplateVerifyRender),
            "ctr_schema_detect" => Ok(Self::CtrSchemaDetect),
            "ctr_schema_verify_render" => Ok(Self::CtrSchemaVerifyRender),
            "ctr_outcome_compare" => Ok(Self::CtrOutcomeCompare),
            "ctr_outcome_report" => Ok(Self::CtrOutcomeReport),
            "merge_load_plan" => Ok(Self::MergeLoadPlan),
            "merge_preflight" => Ok(Self::MergePreflight),
            "merge_extract_sections" => Ok(Self::MergeExtractSections),
            "merge_draft_patch" => Ok(Self::MergeDraftPatch),
            "merge_apply_patch" => Ok(Self::MergeApplyPatch),
            "merge_generate_redirects" => Ok(Self::MergeGenerateRedirects),
            "merge_validate_output" => Ok(Self::MergeValidateOutput),
            "hub_load_recommendation" => Ok(Self::HubLoadRecommendation),
            "hub_build_brief" => Ok(Self::HubBuildBrief),
            "hub_outline" => Ok(Self::HubOutline),
            "hub_write" => Ok(Self::HubWrite),
            "hub_apply_draft" => Ok(Self::HubApplyDraft),
            "hub_apply_links" => Ok(Self::HubApplyLinks),
            "hub_validate" => Ok(Self::HubValidate),
            "territory_load_recommendation" => Ok(Self::TerritoryLoadRecommendation),
            "territory_build_context" => Ok(Self::TerritoryBuildContext),
            "territory_strategy" => Ok(Self::TerritoryStrategy),
            "territory_apply" => Ok(Self::TerritoryApply),
            "sanitize_content" => Ok(Self::SanitizeContent),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_variants_round_trip_through_as_str_and_from_str() {
        let variants = [
            StepKind::Deterministic,
            StepKind::Agentic,
            StepKind::Manual,
            StepKind::ClusterLinkScan,
            StepKind::ClusterLinkStrategy,
            StepKind::ClusterLinkApply,
            StepKind::ContentReviewRecommend,
            StepKind::ContentReviewApplyExecute,
            StepKind::KeywordResearchNative,
            StepKind::ResearchFinalSelection,
            StepKind::LandingPageSpecWrite,
            StepKind::RedditConfigParse,
            StepKind::RedditSearch,
            StepKind::RedditEnrich,
            StepKind::RedditFetchResults,
            StepKind::ContentSync,
            StepKind::FormatValidation,
            StepKind::FormatFix,
            StepKind::GscSyncArticles,
            StepKind::GscSummarise,
            StepKind::IndexingFixContext,
            StepKind::IndexingFixApply,
            StepKind::ContentAudit,
            StepKind::CollectGscInspect,
            StepKind::IndexingDiagnosticsRun,
            StepKind::GscInvestigateAgentic,
            StepKind::SocialCollectSources,
            StepKind::SocialLoadTemplates,
            StepKind::SocialGeneratePosts,
            StepKind::SocialBuildVisuals,
            StepKind::SocialSaveCampaign,
            StepKind::SocialRegenerateSingle,
            StepKind::SocialRebuildVisual,
            StepKind::SocialUpdatePost,
            StepKind::SocialDesignTemplate,
            StepKind::SocialSaveTemplate,
            StepKind::CoverageLoadArticles,
            StepKind::CoverageClusterAnalysis,
            StepKind::CoverageSave,
            StepKind::RedditPostReply,
            StepKind::SocialExtractArticle,
            StepKind::ResearchAutocomplete,
            StepKind::ResearchSeedValidation,
            StepKind::KeywordResearchToolAgent,
            StepKind::CtrBuildContext,
            StepKind::CtrAnalyze,
            StepKind::CtrFixGenerate,
            StepKind::CanBuildContext,
            StepKind::CanAnalyze,
            StepKind::CtrFixGenerate,
            StepKind::CtrFixApply,
            StepKind::CtrVerifyFix,
            StepKind::CtrRenderedSerpAudit,
            StepKind::CtrTemplateDetect,
            StepKind::CtrTemplatePlan,
            StepKind::CtrTemplateVerifyRender,
            StepKind::CtrSchemaDetect,
            StepKind::CtrSchemaVerifyRender,
            StepKind::CtrOutcomeCompare,
            StepKind::CtrOutcomeReport,
            StepKind::MergeLoadPlan,
            StepKind::MergePreflight,
            StepKind::MergeExtractSections,
            StepKind::MergeDraftPatch,
            StepKind::MergeApplyPatch,
            StepKind::MergeGenerateRedirects,
            StepKind::MergeValidateOutput,
            StepKind::HubLoadRecommendation,
            StepKind::HubBuildBrief,
            StepKind::HubOutline,
            StepKind::HubWrite,
            StepKind::HubApplyDraft,
            StepKind::HubApplyLinks,
            StepKind::HubValidate,
            StepKind::TerritoryLoadRecommendation,
            StepKind::TerritoryBuildContext,
            StepKind::TerritoryStrategy,
            StepKind::TerritoryApply,
            StepKind::SanitizeContent,
        ];

        for variant in &variants {
            let s = variant.as_str();
            let parsed = StepKind::from_str(s).unwrap_or_else(|_| {
                panic!(
                    "StepKind variant {:?} failed to round-trip through '{}'",
                    variant, s
                )
            });
            assert_eq!(
                *variant, parsed,
                "Round-trip failed for '{}': expected {:?}, got {:?}",
                s, variant, parsed
            );
        }
    }

    #[test]
    fn unknown_variant_as_str_and_display() {
        assert_eq!(StepKind::Unknown.as_str(), "unknown");
        assert_eq!(format!("{}", StepKind::Unknown), "unknown");
        // Note: from_str("unknown") intentionally returns Err so deserialization
        // maps unrecognized strings to StepKind::Unknown rather than treating
        // "unknown" as a magic sentinel value.
        assert!(StepKind::from_str("unknown").is_err());
    }

    #[test]
    fn display_matches_as_str() {
        assert_eq!(format!("{}", StepKind::Deterministic), "deterministic");
        assert_eq!(
            format!("{}", StepKind::CtrBuildContext),
            "ctr_build_context"
        );
    }

    #[test]
    fn serialize_deserialize_round_trip() {
        let kind = StepKind::RedditSearch;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"reddit_search\"");
        let decoded: StepKind = serde_json::from_str(&json).unwrap();
        assert_eq!(kind, decoded);
    }

    #[test]
    fn deserialize_unknown_defaults_to_unknown_variant() {
        let decoded: StepKind = serde_json::from_str("\"nonexistent_kind\"").unwrap();
        assert_eq!(decoded, StepKind::Unknown);
    }
}
