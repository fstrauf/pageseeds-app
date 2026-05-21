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
    /// Deterministic: ensure keyword_coverage.json exists and is fresh.
    /// If stale or missing, runs coverage analysis inline before dependent steps proceed.
    EnsureCoverageFresh,
    RedditPostReply,
    SocialExtractArticle,
    /// Fetch Google Autocomplete suggestions per theme (deterministic).
    ResearchAutocomplete,
    /// LLM filters autocomplete suggestions for domain relevance (agentic).
    ResearchSeedValidation,
    /// Rig Agent with native tool calling for keyword research (experimental).
    KeywordResearchToolAgent,
    /// Deterministic territory analysis: groups articles by keyword, finds open/saturated themes.
    ResearchTerritoryAnalysis,
    /// Build structured context for CTR audit (deterministic data collection).
    CtrBuildContext,
    /// Agentic CTR analysis — titles, meta, snippets, FAQ (agentic).
    CtrAnalyze,
    /// Build structured context for cannibalization audit (TF-IDF + data formatting).
    CanBuildContext,
    /// Deterministic detection of exact duplicate target keywords + GSC ranking.
    CanExactKeywordDupes,
    /// Deterministic selection of merge/hub/territory candidates from audit artifacts.
    CanSelectCandidates,
    /// Agentic analysis of individual candidate batches (byte-budgeted).
    CanAnalyzeCandidates,
    /// Deterministic merge of batch outputs into final strategy JSON.
    CanReduceStrategy,
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
    // ─── Fix Content Article ────────────────────────────────────────────────────
    /// Deterministic: load recommendations + file content for a single article.
    FixContentArticleContext,
    /// Agentic: generate structured ContentFixPatch using skill + Rig extraction.
    FixContentArticleGenerate,
    /// Deterministic: apply agent-generated content fix patch to MDX file.
    FixContentArticleApply,
    /// Deterministic: verify applied content fixes meet health thresholds.
    FixContentArticleVerify,
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
    /// Sync merged articles back to SQLite and articles.json.
    MergeSyncArticles,
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
    // ─── GSC Indexing Recovery ──────────────────────────────────────────────────
    /// Deterministic: refresh stale GSC/link data before recovery planning.
    GscRecoveryPrepare,
    /// Deterministic: compute drift report from current sitemap/GSC/link data.
    GscRecoveryDrift,
    /// Deterministic: filter, score, and build target plan with source candidates.
    GscRecoveryPlan,
    // ─── Fix Indexing Internal Links ────────────────────────────────────────────
    /// Deterministic: build per-target context (target + source shortlist).
    IndexingLinkContext,
    /// Agentic: choose source links from shortlist for the target.
    IndexingLinkPlan,
    /// Deterministic: apply Related Articles links to source MDX files.
    IndexingLinkApply,
    /// Deterministic: verify target gained inbound links after apply.
    IndexingLinkVerify,
    // ─── GSC Indexing Outcome Review ────────────────────────────────────────────
    /// Deterministic: re-inspect target URL in GSC after wait period.
    GscIndexingOutcomeInspect,
    /// Deterministic: compare before/after indexing status and write report.
    GscIndexingOutcomeReport,
    // ─── Indexing Health Campaign ─────────────────────────────────────────────
    /// Deterministic: check prerequisite artifact freshness.
    IhcCheckPrerequisites,
    /// Deterministic: build per-target cluster context for not-indexed URLs.
    IhcBuildTargetContext,
    /// Agentic: judge title/H1 distinctiveness against cluster siblings.
    IhcDistinctivenessReview,
    /// Deterministic: reduce all inputs into a campaign plan.
    IhcReducePlan,
    /// Agentic: synthesize audit findings into a prioritized developer feature spec.
    GenerateFeatureSpec,
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
            Self::EnsureCoverageFresh => "ensure_coverage_fresh",
            Self::RedditPostReply => "reddit_post_reply",
            Self::SocialExtractArticle => "social_extract_article",
            Self::ResearchAutocomplete => "research_autocomplete",
            Self::ResearchSeedValidation => "research_seed_validation",
            Self::KeywordResearchToolAgent => "keyword_research_tool_agent",
            Self::ResearchTerritoryAnalysis => "research_territory_analysis",
            Self::CtrBuildContext => "ctr_build_context",
            Self::CtrAnalyze => "ctr_analyze",
            Self::CanBuildContext => "can_build_context",
            Self::CanExactKeywordDupes => "can_exact_keyword_dupes",
            Self::CanSelectCandidates => "can_select_candidates",
            Self::CanAnalyzeCandidates => "can_analyze_candidates",
            Self::CanReduceStrategy => "can_reduce_strategy",
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
            Self::FixContentArticleContext => "fix_content_article_context",
            Self::FixContentArticleGenerate => "fix_content_article_generate",
            Self::FixContentArticleApply => "fix_content_article_apply",
            Self::FixContentArticleVerify => "fix_content_article_verify",
            Self::MergeLoadPlan => "merge_load_plan",
            Self::MergePreflight => "merge_preflight",
            Self::MergeExtractSections => "merge_extract_sections",
            Self::MergeDraftPatch => "merge_draft_patch",
            Self::MergeApplyPatch => "merge_apply_patch",
            Self::MergeGenerateRedirects => "merge_generate_redirects",
            Self::MergeValidateOutput => "merge_validate_output",
            Self::MergeSyncArticles => "merge_sync_articles",
            Self::TerritoryLoadRecommendation => "territory_load_recommendation",
            Self::TerritoryBuildContext => "territory_build_context",
            Self::TerritoryStrategy => "territory_strategy",
            Self::TerritoryApply => "territory_apply",
            Self::SanitizeContent => "sanitize_content",
            Self::GscRecoveryPrepare => "gsc_recovery_prepare",
            Self::GscRecoveryDrift => "gsc_recovery_drift",
            Self::GscRecoveryPlan => "gsc_recovery_plan",
            Self::IndexingLinkContext => "indexing_link_context",
            Self::IndexingLinkPlan => "indexing_link_plan",
            Self::IndexingLinkApply => "indexing_link_apply",
            Self::IndexingLinkVerify => "indexing_link_verify",
            Self::GscIndexingOutcomeInspect => "gsc_indexing_outcome_inspect",
            Self::GscIndexingOutcomeReport => "gsc_indexing_outcome_report",
            Self::IhcCheckPrerequisites => "ihc_check_prerequisites",
            Self::IhcBuildTargetContext => "ihc_build_target_context",
            Self::IhcDistinctivenessReview => "ihc_distinctiveness_review",
            Self::IhcReducePlan => "ihc_reduce_plan",
            Self::GenerateFeatureSpec => "generate_feature_spec",
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
            "ensure_coverage_fresh" => Ok(Self::EnsureCoverageFresh),
            "reddit_post_reply" => Ok(Self::RedditPostReply),
            "social_extract_article" => Ok(Self::SocialExtractArticle),
            "research_autocomplete" => Ok(Self::ResearchAutocomplete),
            "research_seed_validation" => Ok(Self::ResearchSeedValidation),
            "keyword_research_tool_agent" => Ok(Self::KeywordResearchToolAgent),
            "research_territory_analysis" => Ok(Self::ResearchTerritoryAnalysis),
            "ctr_build_context" => Ok(Self::CtrBuildContext),
            "ctr_analyze" => Ok(Self::CtrAnalyze),
            "can_build_context" => Ok(Self::CanBuildContext),
            "can_exact_keyword_dupes" => Ok(Self::CanExactKeywordDupes),
            "can_select_candidates" => Ok(Self::CanSelectCandidates),
            "can_analyze_candidates" => Ok(Self::CanAnalyzeCandidates),
            "can_reduce_strategy" => Ok(Self::CanReduceStrategy),
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
            "fix_content_article_context" => Ok(Self::FixContentArticleContext),
            "fix_content_article_generate" => Ok(Self::FixContentArticleGenerate),
            "fix_content_article_apply" => Ok(Self::FixContentArticleApply),
            "fix_content_article_verify" => Ok(Self::FixContentArticleVerify),
            "merge_load_plan" => Ok(Self::MergeLoadPlan),
            "merge_preflight" => Ok(Self::MergePreflight),
            "merge_extract_sections" => Ok(Self::MergeExtractSections),
            "merge_draft_patch" => Ok(Self::MergeDraftPatch),
            "merge_apply_patch" => Ok(Self::MergeApplyPatch),
            "merge_generate_redirects" => Ok(Self::MergeGenerateRedirects),
            "merge_validate_output" => Ok(Self::MergeValidateOutput),
            "merge_sync_articles" => Ok(Self::MergeSyncArticles),
            "territory_load_recommendation" => Ok(Self::TerritoryLoadRecommendation),
            "territory_build_context" => Ok(Self::TerritoryBuildContext),
            "territory_strategy" => Ok(Self::TerritoryStrategy),
            "territory_apply" => Ok(Self::TerritoryApply),
            "sanitize_content" => Ok(Self::SanitizeContent),
            "gsc_recovery_prepare" => Ok(Self::GscRecoveryPrepare),
            "gsc_recovery_drift" => Ok(Self::GscRecoveryDrift),
            "gsc_recovery_plan" => Ok(Self::GscRecoveryPlan),
            "indexing_link_context" => Ok(Self::IndexingLinkContext),
            "indexing_link_plan" => Ok(Self::IndexingLinkPlan),
            "indexing_link_apply" => Ok(Self::IndexingLinkApply),
            "indexing_link_verify" => Ok(Self::IndexingLinkVerify),
            "gsc_indexing_outcome_inspect" => Ok(Self::GscIndexingOutcomeInspect),
            "gsc_indexing_outcome_report" => Ok(Self::GscIndexingOutcomeReport),
            "ihc_check_prerequisites" => Ok(Self::IhcCheckPrerequisites),
            "ihc_build_target_context" => Ok(Self::IhcBuildTargetContext),
            "ihc_distinctiveness_review" => Ok(Self::IhcDistinctivenessReview),
            "ihc_reduce_plan" => Ok(Self::IhcReducePlan),
            "generate_feature_spec" => Ok(Self::GenerateFeatureSpec),
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
            StepKind::EnsureCoverageFresh,
            StepKind::RedditPostReply,
            StepKind::SocialExtractArticle,
            StepKind::ResearchAutocomplete,
            StepKind::ResearchSeedValidation,
            StepKind::KeywordResearchToolAgent,
            StepKind::ResearchTerritoryAnalysis,
            StepKind::CtrBuildContext,
            StepKind::CtrAnalyze,
            StepKind::CtrFixGenerate,
            StepKind::CanBuildContext,
            StepKind::CanExactKeywordDupes,
            StepKind::CanSelectCandidates,
            StepKind::CanAnalyzeCandidates,
            StepKind::CanReduceStrategy,
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
            StepKind::FixContentArticleContext,
            StepKind::FixContentArticleGenerate,
            StepKind::FixContentArticleApply,
            StepKind::FixContentArticleVerify,
            StepKind::MergeLoadPlan,
            StepKind::MergePreflight,
            StepKind::MergeExtractSections,
            StepKind::MergeDraftPatch,
            StepKind::MergeApplyPatch,
            StepKind::MergeGenerateRedirects,
            StepKind::MergeValidateOutput,
            StepKind::TerritoryLoadRecommendation,
            StepKind::TerritoryBuildContext,
            StepKind::TerritoryStrategy,
            StepKind::TerritoryApply,
            StepKind::SanitizeContent,
            StepKind::GscRecoveryPrepare,
            StepKind::GscRecoveryDrift,
            StepKind::GscRecoveryPlan,
            StepKind::IndexingLinkContext,
            StepKind::IndexingLinkPlan,
            StepKind::IndexingLinkApply,
            StepKind::IndexingLinkVerify,
            StepKind::GscIndexingOutcomeInspect,
            StepKind::GscIndexingOutcomeReport,
            StepKind::IhcCheckPrerequisites,
            StepKind::IhcBuildTargetContext,
            StepKind::IhcDistinctivenessReview,
            StepKind::IhcReducePlan,
            StepKind::GenerateFeatureSpec,
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
