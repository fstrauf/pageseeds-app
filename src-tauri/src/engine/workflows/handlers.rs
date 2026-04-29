/// Trait-based workflow handlers — one per task family.
///
/// Each handler knows:
///   - which task types it owns (`supports`)
///   - what steps the task needs (`plan`)
///
/// Step execution happens in `executor.rs`; handlers only describe the plan.
use super::{step_params, StepKind, StepResult, WorkflowStep};
use crate::engine::project_paths::ProjectPaths;
use crate::models::task::Task;

// ─── Trait ────────────────────────────────────────────────────────────────────

pub trait WorkflowHandler: Send + Sync {
    fn supports(&self, task: &Task) -> bool;
    fn plan(&self, task: &Task) -> Vec<WorkflowStep>;
}

// ─── Helper ───────────────────────────────────────────────────────────────────

fn task_type(t: &Task) -> &str {
    &t.task_type
}

// ─── Collection ───────────────────────────────────────────────────────────────

pub struct CollectionHandler;

impl WorkflowHandler for CollectionHandler {
    fn supports(&self, task: &Task) -> bool {
        matches!(task_type(task), "collect_gsc" | "collect_posthog")
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task_type(task) {
            "collect_gsc" => vec![WorkflowStep::new(
                "collect_gsc_inspect",
                StepKind::CollectGscInspect,
            )],
            // collect_posthog has no CLI implementation yet — fall back to agent.
            _ => vec![WorkflowStep::new("collect_agent_stage", StepKind::Agentic)],
        }
    }
}

// ─── Investigation ────────────────────────────────────────────────────────────

pub struct InvestigationHandler;

impl WorkflowHandler for InvestigationHandler {
    fn supports(&self, task: &Task) -> bool {
        matches!(task_type(task), "investigate_gsc" | "investigate_posthog")
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task_type(task) {
            "investigate_gsc" => vec![
                // Step 1 (deterministic): group reason_codes, count occurrences, pick example URLs.
                // Writes gsc_summary.json. Grouping and counting cannot require judgment.
                WorkflowStep::new("investigate_gsc_summarise", StepKind::GscSummarise),
                // Step 2 (agentic): interpret the grouped summary, identify patterns, recommend
                // corrective actions. Cannot be deterministic: interpreting *why* a cluster of
                // pages is not indexed and what to do about it requires intent-level judgment.
                WorkflowStep::new("investigate_gsc_agent", StepKind::GscInvestigateAgentic),
            ],
            // investigate_posthog has no CLI implementation yet — fall back to agent.
            _ => vec![WorkflowStep::new(
                "investigate_agent_stage",
                StepKind::Agentic,
            )],
        }
    }
}

// ─── Research ─────────────────────────────────────────────────────────────────

pub struct ResearchHandler;

impl WorkflowHandler for ResearchHandler {
    fn supports(&self, task: &Task) -> bool {
        matches!(
            task_type(task),
            "research_keywords" | "custom_keyword_research" | "research_landing_pages"
        )
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task_type(task) {
            "research_keywords" | "research_landing_pages" => {
                // 5-step hybrid workflow:
                // agentic → deterministic → agentic → deterministic → deterministic
                vec![
                    // Step 1 (agentic): LLM extracts 3-4 themes from project brief.
                    // Uses rig Extractor<T> for guaranteed structured JSON output.
                    // Cannot be deterministic: requires reading intent from free-form text.
                    WorkflowStep::new("research_seed_extraction", StepKind::Agentic),
                    // Step 2 (deterministic): fetch Google Autocomplete for all themes.
                    // Free API, always returns results. Outputs structured JSON: [{theme, suggestions}].
                    WorkflowStep::new("research_autocomplete", StepKind::ResearchAutocomplete),
                    // Step 3 (agentic): LLM filters autocomplete suggestions for domain relevance.
                    // Uses rig Extractor<T> for guaranteed structured JSON output.
                    // Cannot be deterministic: requires understanding what is on-topic for this
                    // specific product/site. Hard-coding a relevance rule would produce silent errors
                    // on any input it was not tested against.
                    // Input contract: [{theme, suggestions: [string]}]
                    // Output contract: {validated_seeds: [{theme: string, seeds: [string]}]}
                    WorkflowStep::new("research_seed_validation", StepKind::Agentic),
                    // Step 4 (deterministic): DataForSEO related_keywords per validated seed.
                    // Deterministic: given validated seeds, fetches keyword ideas + KD + volume.
                    WorkflowStep::new("research_ahrefs_pipeline", StepKind::KeywordResearchNative),
                    // Step 5 (deterministic): Select best candidates from structured data.
                    // Outputs clean JSON directly — no normalizer needed because upstream
                    // agentic steps now use Extractor<T>.
                    WorkflowStep::new("research_final_selection", StepKind::ResearchFinalSelection),
                ]
            }
            _ => {
                // Legacy path: raw agentic call via seo-keyword-research skill.
                // The output is raw agent text; downstream consumers must parse JSON if needed.
                vec![WorkflowStep::new("research_agent_stage", StepKind::Agentic)
                    .with_param(step_params::SKILL, "seo-keyword-research")]
            }
        }
    }
}

// ─── Content ──────────────────────────────────────────────────────────────────

pub struct ContentHandler;

impl WorkflowHandler for ContentHandler {
    fn supports(&self, task: &Task) -> bool {
        matches!(
            task_type(task),
            "write_article"
                | "optimize_article"
                | "create_content"
                | "optimize_content"
                | "content_review_apply"
        )
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        if task_type(task) == "content_review_apply" {
            // Dedicated step runner that reads the recommendations artifact and
            // builds a structured apply prompt — not a generic skill/agentic call.
            return vec![WorkflowStep::new(
                "content_review_apply_execute",
                StepKind::ContentReviewApplyExecute,
            )];
        }
        // Agentic: the agent reads the article spec and writes the MDX file.
        vec![WorkflowStep::new("content_write_stage", StepKind::Agentic)]
    }
}

// ─── Content Review ───────────────────────────────────────────────────────────

pub struct ContentReviewHandler;

impl WorkflowHandler for ContentReviewHandler {
    fn supports(&self, task: &Task) -> bool {
        matches!(task_type(task), "content_review" | "content_audit")
    }

    fn plan(&self, _task: &Task) -> Vec<WorkflowStep> {
        vec![
            // Step 1: fetch GSC page metrics and write into articles.json.
            // Optional — a missing service account skips gracefully rather than aborting.
            WorkflowStep::new("content_review_gsc_sync", StepKind::GscSyncArticles).optional(),
            // Step 2: deterministic multi-check audit → writes content_audit.json.
            // Optional — still valuable even without GSC data.
            WorkflowStep::new("content_review_audit", StepKind::ContentAudit).optional(),
            // Step 3: native sync — validates articles.json ↔ content files, dates.
            WorkflowStep::new("content_review_sync", StepKind::ContentSync).optional(),
            // Step 4: select priority articles, build structured context, get agent recommendations.
            // One focused agent call (not N calls). Writes recommendations.json.
            WorkflowStep::new("content_review_recommend", StepKind::ContentReviewRecommend),
        ]
    }
}

// ─── Implementation ───────────────────────────────────────────────────────────

pub struct ImplementationHandler;

impl WorkflowHandler for ImplementationHandler {
    fn supports(&self, task: &Task) -> bool {
        let t = task_type(task);
        // Only claim types that are explicitly listed or named like fix_*.
        // Do NOT use a phase catch-all — that was the root cause of content_review being
        // silently captured and generating a bogus `pageseeds content review run` command.
        // Unknown task types fall through to ManualFallbackHandler instead.
        matches!(
            t,
            "cluster_and_link"
                | "content_cleanup"
                | "sanitize_content"
                | "publish_content"
                | "indexing_diagnostics"
                | "content_strategy"
                | "technical_fix"
                | "technical_seo"
                | "landing_page_spec"
                | "create_landing_page"
                | "calculator_rollout"
        ) || t.starts_with("fix_")
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task_type(task) {
            "content_cleanup" => vec![
                // Step 1 (deterministic): validate frontmatter format across all MDX files.
                // Writes format_issues.json to the automation dir.
                WorkflowStep::new("content_cleanup_validate", StepKind::FormatValidation),
                // Step 2 (deterministic): apply auto-fixes for all auto-fixable issues.
                WorkflowStep::new("content_cleanup_fix", StepKind::FormatFix),
            ],
            "sanitize_content" => vec![
                // Single deterministic step: rename .md → .mdx, repair paths, validate frontmatter (read-only).
                // Broad frontmatter auto-fix is intentionally NOT applied here; use format_fix for that.
                WorkflowStep::new("sanitize_content_run", StepKind::SanitizeContent),
            ],
            "publish_content" => {
                vec![
                    WorkflowStep::new("publish_content_run", StepKind::Deterministic).with_param(
                        step_params::CMD,
                        "pageseeds content validate --workspace-dir {automation_dir}",
                    ),
                ]
            }
            "fix_content_article" => vec![
                // Per-article content fix: reads the recommendations artifact embedded in the task
                // and applies SEO improvements (title, meta, intro, internal links, FAQ, EEAT, CTA)
                // to a single MDX file. One focused agent call per article.
                WorkflowStep::new("fix_content_article_apply", StepKind::Agentic),
            ],
            "fix_ctr_article" => vec![
                // Step 1 (agentic): Analyze the single article's CTR context and produce a
                // CtrRecommendation. Reads ctr_context from the task's artifacts.
                // Output contract: single CtrRecommendation JSON (stored as ctr_recommendations artifact).
                WorkflowStep::new("ctr_analyze_single", StepKind::CtrAnalyze)
                    .with_param(step_params::SKILL, "ctr-optimization")
                    .with_param(step_params::ARTIFACT_NAME, "ctr_recommendations"),
                // Step 2 (agentic): read file + recommendations, produce structured CtrFixPatch JSON.
                // Cannot be deterministic: the agent must read the article and write prose that
                // satisfies SERP intent, brand voice, and keyword context.
                // Input contract: single CtrRecommendation artifact + file contents.
                // Output contract: CtrFixPatch JSON.
                WorkflowStep::new("fix_ctr_article_generate", StepKind::Agentic)
                    .with_param(step_params::SKILL, "ctr-fix-apply"),
                // Step 3 (deterministic): apply the patch to the MDX file.
                // Snapshots original, replaces frontmatter/body, validates structure, restores on corruption.
                WorkflowStep::new("fix_ctr_article_apply", StepKind::CtrFixApply),
                // Step 4 (deterministic): re-run health checks to verify fixes meet thresholds.
                // Produces CtrFixVerificationReport. Status = done if all pass, review if partial.
                WorkflowStep::new("fix_ctr_article_verify", StepKind::CtrVerifyFix),
            ],
            "fix_ctr_site_template" => vec![
                // Step 1 (deterministic): detect repeated title template patterns from rendered audits.
                // Groups pages by common suffix, identifies framework files, computes desired pattern.
                WorkflowStep::new("ctr_template_detect", StepKind::CtrTemplateDetect),
                // Step 2 (agentic/manual-review): produce framework-aware fix plan.
                // Cannot be deterministic: the correct fix depends on framework (Next.js/Astro/Gatsby)
                // and site-specific conventions. The agent reads candidate files and suggests edits.
                // Output contract: markdown plan with file paths, current code, proposed changes.
                WorkflowStep::new("ctr_template_plan", StepKind::Agentic)
                    .with_param(step_params::SKILL, "ctr-template-fix"),
                // Step 3 (manual): framework code changes require manual review/application.
                // The task cannot auto-apply changes to the target repo's layout/metadata code.
                // The workflow ends here; verification happens via a subsequent ctr_audit run after
                // the user applies the fix in the target repo.
                WorkflowStep::new("ctr_template_apply", StepKind::Manual),
            ],
            "indexing_diagnostics" => vec![
                // Stateful GSC indexing diagnostics: native Rust, tracks per-URL history in SQLite,
                // only re-checks stale or known-bad URLs, and spawns fix tasks for new/regressed
                // or unresolved issues. Deterministic because it is pure API calls + DB comparison.
                WorkflowStep::new("indexing_diagnostics_run", StepKind::IndexingDiagnosticsRun),
            ],
            "fix_indexing" | "fix_technical" => vec![
                // Step 1 (deterministic): load the target MDX file and extract structured context
                // (word count, H1, title, internal links, canonical). This is obvious file I/O —
                // no judgment required — and saves the agent from hunting around the repo.
                WorkflowStep::new("indexing_fix_context", StepKind::IndexingFixContext),
                // Step 2 (agentic): apply the fix. The agent gets the GSC issue + structured
                // context and edits the MDX file directly. Judgment is required because the fix
                // depends on intent, content quality, and site-specific conventions.
                WorkflowStep::new("indexing_fix_apply", StepKind::IndexingFixApply),
            ],
            "cluster_and_link" => vec![
                // Step 1 (deterministic, native Rust): scan all MDX files, build the full link
                // map, identify orphans and coverage gaps.  Pure file I/O + regex — no judgment.
                // Writes link_scan.json to the automation dir for the next step to consume.
                WorkflowStep::new("cluster_and_link_scan", StepKind::ClusterLinkScan),
                // Step 2 (agentic): interpret the scan output, determine pillar/cluster
                // structure, and recommend specific missing links to add.
                // Cannot be deterministic: deciding which articles are pillars vs supports,
                // and which gaps matter most, requires understanding article intent and
                // business priorities — not just graph connectivity.
                // Writes links_to_add.json (output contract: {links_to_add:[{source_article_id,
                // source_file, target_article_id, target_title, target_slug, reason}]}).
                WorkflowStep::new("cluster_and_link_strategy", StepKind::ClusterLinkStrategy),
                // Step 3 (deterministic): read links_to_add.json and append "Related Articles"
                // sections to the MDX files that are missing them.
                // Skips files that already have a Related Articles section or already link
                // to the target slug.
                WorkflowStep::new("cluster_and_link_apply", StepKind::ClusterLinkApply),
            ],
            "create_landing_page" | "landing_page_spec" => vec![
                // Deterministic: build a structured spec file from keyword metadata
                // already on the task. No LLM needed — the spec is a structured template
                // populated with keyword, page type, intent, volume, and KD.
                WorkflowStep::new("landing_page_spec_write", StepKind::LandingPageSpecWrite),
            ],
            // fix_* and other implementation types: agentic for now.
            //
            // TODO: each fix_* type that gets implemented should follow the hybrid pattern:
            //   Step 1 (deterministic): classify URLs/issues by pattern, generate
            //     mechanical rules (e.g. redirect rules for recognisable URL formats)
            //   Step 2 (agentic): handle ambiguous cases that don't fit a pattern
            //
            // An agentic step alone for fix_404s will ask the LLM to generate redirect
            // rules for patterns that a regex would handle reliably.
            _ => vec![WorkflowStep::new(
                "implementation_agent_stage",
                StepKind::Agentic,
            )],
        }
    }
}

// ─── Reddit ───────────────────────────────────────────────────────────────────

pub struct RedditHandler;

impl WorkflowHandler for RedditHandler {
    fn supports(&self, task: &Task) -> bool {
        task_type(task).starts_with("reddit_")
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task_type(task) {
            "reddit_opportunity_search" => vec![
                // Step 1 (agentic): Parse reddit_config.md and extract structured search parameters.
                WorkflowStep::new("reddit_config_parse_stage", StepKind::RedditConfigParse),
                // Step 2 (deterministic): API search using the structured parameters.
                WorkflowStep::new("reddit_search_stage", StepKind::RedditSearch),
                // Step 3 (agentic): relevance scoring, pain point extraction, reply drafting.
                WorkflowStep::new("reddit_enrich_stage", StepKind::RedditEnrich),
                // Step 4 (deterministic): Fetch enriched opportunities from DB.
                WorkflowStep::new("reddit_results_stage", StepKind::RedditFetchResults),
            ],
            "reddit_reply" => vec![
                // Step 1 (deterministic): Post the reply to Reddit via API.
                // Extracts post_id and reply_text from task description.
                WorkflowStep::new("reddit_post_reply", StepKind::RedditPostReply),
            ],
            _ => {
                // Other reddit tasks: raw agentic call.
                vec![WorkflowStep::new("reddit_agent_stage", StepKind::Agentic)]
            }
        }
    }
}

// ─── Keyword Coverage ─────────────────────────────────────────────────────────

pub struct CoverageHandler;

impl WorkflowHandler for CoverageHandler {
    fn supports(&self, task: &Task) -> bool {
        task_type(task) == "analyze_keyword_coverage"
    }

    fn plan(&self, _task: &Task) -> Vec<WorkflowStep> {
        vec![
            // Step 1 (deterministic): Load articles from articles.json
            WorkflowStep::new("coverage_load_articles", StepKind::CoverageLoadArticles),
            // Step 2 (agentic): Cluster articles by semantic similarity
            // Cannot be deterministic: understanding topic relationships and naming
            // clusters requires semantic judgment about content themes.
            WorkflowStep::new(
                "coverage_cluster_analysis",
                StepKind::CoverageClusterAnalysis,
            ),
            // Step 3 (deterministic): Save results to keyword_coverage.json
            WorkflowStep::new("coverage_save", StepKind::CoverageSave),
        ]
    }
}

// ─── Performance ─────────────────────────────────────────────────────────────

pub struct PerformanceHandler;

impl WorkflowHandler for PerformanceHandler {
    fn supports(&self, task: &Task) -> bool {
        task_type(task) == "analyze_gsc_performance"
    }

    fn plan(&self, _task: &Task) -> Vec<WorkflowStep> {
        // TODO: not yet implemented. The correct design is:
        //   Step 1 (deterministic): fetch GSC analytics/movers data for the site
        //   Step 2 (agentic): interpret trends, surface ranking opportunities
        //
        // An empty agentic step with no data context is fake intelligence — the
        // agent receives no artifact and produces generic filler. Use manual until
        // the deterministic data-fetch step exists.
        vec![WorkflowStep::new("performance_manual", StepKind::Manual)]
    }
}

// ─── CTR Audit ────────────────────────────────────────────────────────────────

pub struct CtrAuditHandler;

impl WorkflowHandler for CtrAuditHandler {
    fn supports(&self, task: &Task) -> bool {
        matches!(task_type(task), "ctr_audit" | "ctr_outcome_review")
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task_type(task) {
            "ctr_outcome_review" => vec![
                // Step 1 (deterministic): load baseline outcomes and fetch after-period GSC metrics.
                // Compare before/after clicks, CTR, position per article and query.
                // Output contract: JSON report with improved/regressed/neutral counts.
                WorkflowStep::new("ctr_outcome_compare", StepKind::CtrOutcomeCompare),
                // Step 2 (deterministic): generate structured report artifact.
                WorkflowStep::new("ctr_outcome_report", StepKind::CtrOutcomeReport),
            ],
            _ => vec![
                // Step 1 (deterministic): Sync latest GSC data.
                WorkflowStep::new("ctr_gsc_sync", StepKind::GscSyncArticles).optional(),
                // Step 2 (deterministic): Fetch live HTML for target pages and compare rendered
                // title/meta/schema/snippet markup against source files.
                // Optional — still valuable even if some pages fail to fetch.
                WorkflowStep::new("ctr_rendered_serp_audit", StepKind::CtrRenderedSerpAudit)
                    .optional(),
                // Step 3 (deterministic): Collect raw article data + compute CTR scores.
                // Includes rendered audit results from DB if available.
                // NO quality judgments — just raw titles, meta descs, first paragraphs, GSC metrics,
                // and deterministic math: clicks_lost = impressions * max(0, target_ctr - actual_ctr).
                WorkflowStep::new("ctr_build_context", StepKind::CtrBuildContext),
                // Per-article fix tasks are spawned in post_actions::after_task_success
                // after this task completes, reading the ctr_build_context artifact.
            ],
        }
    }
}

// ─── Cannibalization Audit ────────────────────────────────────────────────────

pub struct CannibalizationAuditHandler;

impl WorkflowHandler for CannibalizationAuditHandler {
    fn supports(&self, task: &Task) -> bool {
        task_type(task) == "cannibalization_audit"
    }

    fn plan(&self, _task: &Task) -> Vec<WorkflowStep> {
        vec![
            // Step 1 (deterministic): Sync latest GSC data.
            WorkflowStep::new("can_gsc_sync", StepKind::GscSyncArticles).optional(),
            // Step 2 (deterministic): Load articles from articles.json or live-site inventory.
            WorkflowStep::new("can_coverage_load", StepKind::CoverageLoadArticles),
            // Step 3 (deterministic): Compute TF-IDF similarity matrix + format structured context.
            // Pure math: TF-IDF vectorization on [title, h1, target_keyword, first_200_words],
            // cosine similarity between pairs, group by shared keyword. NO judgment about what
            // to do with the clusters.
            WorkflowStep::new("can_build_context", StepKind::CanBuildContext),
            // Step 4 (agentic): Generate merge strategy + expansion plan.
            // Cannot be deterministic: deciding which article to keep in a merge requires
            // judgment about authority (impressions, internal links), content quality, and
            // brand alignment. Cannot be reduced to a single metric.
            // Input contract: structured JSON with similarity clusters + article metadata.
            // Output contract: JSON with merge_recommendations, hub_recommendations, territory_recommendations.
            WorkflowStep::new("can_analyze", StepKind::CanAnalyze)
                .with_param(step_params::SKILL, "cannibalization-strategy")
                .with_param(step_params::ARTIFACT_NAME, "cannibalization_strategy"),
            // No normalizer needed — can_analyze extracts JSON internally.
        ]
    }
}

// ─── Social Media Marketing ───────────────────────────────────────────────────

pub struct SocialHandler;

impl WorkflowHandler for SocialHandler {
    fn supports(&self, task: &Task) -> bool {
        task_type(task).starts_with("social_")
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task_type(task) {
            "social_generate_campaign" => vec![
                WorkflowStep::new("social_collect_sources", StepKind::SocialCollectSources),
                WorkflowStep::new("social_load_templates", StepKind::SocialLoadTemplates),
                WorkflowStep::new("social_generate_posts", StepKind::SocialGeneratePosts),
                WorkflowStep::new("social_build_visuals", StepKind::SocialBuildVisuals),
                WorkflowStep::new("social_save_campaign", StepKind::SocialSaveCampaign),
            ],
            "social_generate_from_article" => vec![
                WorkflowStep::new("social_extract_article", StepKind::SocialExtractArticle),
                WorkflowStep::new("social_generate_posts", StepKind::SocialGeneratePosts),
                WorkflowStep::new("social_build_visuals", StepKind::SocialBuildVisuals),
                WorkflowStep::new("social_save_campaign", StepKind::SocialSaveCampaign),
            ],
            "social_regenerate_post" => vec![
                WorkflowStep::new("social_regenerate_single", StepKind::SocialRegenerateSingle),
                WorkflowStep::new("social_rebuild_visual", StepKind::SocialRebuildVisual),
                WorkflowStep::new("social_update_post", StepKind::SocialUpdatePost),
            ],
            "social_create_template" => vec![
                WorkflowStep::new("social_design_template", StepKind::SocialDesignTemplate),
                WorkflowStep::new("social_save_template", StepKind::SocialSaveTemplate),
            ],
            _ => vec![WorkflowStep::new(
                &format!("{}_manual", task_type(task)),
                StepKind::Manual,
            )],
        }
    }
}

// ─── Consolidate Cluster ─────────────────────────────────────────────────────

pub struct ConsolidateClusterHandler;

impl WorkflowHandler for ConsolidateClusterHandler {
    fn supports(&self, task: &Task) -> bool {
        task_type(task) == "consolidate_cluster"
    }

    fn plan(&self, _task: &Task) -> Vec<WorkflowStep> {
        vec![
            // Step 1 (deterministic): Load approved merge plan from strategy artifact.
            WorkflowStep::new("merge_load_plan", StepKind::MergeLoadPlan),
            // Step 2 (deterministic): Preflight checks — files exist, no redirect cycles, keeper indexable.
            WorkflowStep::new("merge_preflight", StepKind::MergePreflight),
            // Step 3 (deterministic): Extract unique sections (headings, tables, examples, FAQs) from redirect pages.
            WorkflowStep::new("merge_extract_sections", StepKind::MergeExtractSections),
            // Step 4 (agentic): Draft ContentMergePatch JSON deciding which unique content belongs in keeper.
            // Cannot be deterministic: understanding whether a section adds unique value requires judgment.
            // Input contract: structured JSON with keeper content + extracted unique sections.
            // Output contract: ContentMergePatch JSON.
            WorkflowStep::new("merge_draft_patch", StepKind::MergeDraftPatch)
                .with_param(step_params::SKILL, "merge-content"),
            // Step 5 (deterministic): Apply structured patch, snapshot original, validate MDX/frontmatter.
            WorkflowStep::new("merge_apply_patch", StepKind::MergeApplyPatch),
            // Step 6 (deterministic): Generate redirect rules as generic CSV.
            WorkflowStep::new("merge_generate_redirects", StepKind::MergeGenerateRedirects),
            // Step 7 (deterministic): Validate merged keeper and redirect map.
            WorkflowStep::new("merge_validate_output", StepKind::MergeValidateOutput),
        ]
    }
}

// ─── Hub Page ────────────────────────────────────────────────────────────────

pub struct HubPageHandler;

impl WorkflowHandler for HubPageHandler {
    fn supports(&self, task: &Task) -> bool {
        let t = task_type(task);
        t == "create_hub_page" || t == "refresh_hub_page"
    }

    fn plan(&self, _task: &Task) -> Vec<WorkflowStep> {
        vec![
            WorkflowStep::new("hub_load_recommendation", StepKind::HubLoadRecommendation),
            WorkflowStep::new("hub_build_brief", StepKind::HubBuildBrief),
            WorkflowStep::new("hub_outline", StepKind::HubOutline)
                .with_param(step_params::SKILL, "hub-outline"),
            WorkflowStep::new("hub_write", StepKind::HubWrite)
                .with_param(step_params::SKILL, "hub-write"),
            WorkflowStep::new("hub_apply_draft", StepKind::HubApplyDraft),
            WorkflowStep::new("hub_apply_links", StepKind::HubApplyLinks),
            WorkflowStep::new("hub_validate", StepKind::HubValidate),
        ]
    }
}

// ─── Manual Fallback ─────────────────────────────────────────────────────────

// ─── Territory Research ───────────────────────────────────────────────────────

pub struct TerritoryResearchHandler;

impl WorkflowHandler for TerritoryResearchHandler {
    fn supports(&self, task: &Task) -> bool {
        task_type(task) == "territory_research"
    }

    fn plan(&self, _task: &Task) -> Vec<WorkflowStep> {
        vec![
            // Step 1 (deterministic): Load approved territory recommendation from strategy artifact.
            // Extracts theme from task title and finds the matching recommendation.
            WorkflowStep::new(
                "territory_load_recommendation",
                StepKind::TerritoryLoadRecommendation,
            ),
            // Step 2 (deterministic): Query SQLite for existing articles matching theme, read excerpts.
            // Pure data collection — no judgment. Outputs structured TerritoryContext JSON.
            WorkflowStep::new("territory_build_context", StepKind::TerritoryBuildContext),
            // Step 3 (agentic): Generate TerritoryStrategy JSON from context.
            // Cannot be deterministic: deciding which gaps to fill, what competitors cover,
            // and how to avoid cannibalization requires semantic judgment.
            // Input contract: structured TerritoryContext JSON.
            // Output contract: TerritoryStrategy JSON.
            WorkflowStep::new("territory_strategy", StepKind::TerritoryStrategy)
                .with_param(step_params::SKILL, "territory-strategy"),
            // Step 4 (deterministic): Write strategy JSON to automation dir.
            WorkflowStep::new("territory_apply", StepKind::TerritoryApply),
        ]
    }
}

pub struct ManualFallbackHandler;

impl WorkflowHandler for ManualFallbackHandler {
    fn supports(&self, _task: &Task) -> bool {
        true
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        vec![WorkflowStep::new(
            &format!("{}_manual", task_type(task)),
            StepKind::Manual,
        )]
    }
}

// ─── Registry ─────────────────────────────────────────────────────────────────

/// Default ordered handler list (most specific first, fallback last).
pub fn default_handlers() -> Vec<Box<dyn WorkflowHandler>> {
    vec![
        Box::new(CollectionHandler),
        Box::new(InvestigationHandler),
        Box::new(ResearchHandler),
        Box::new(ContentHandler),
        Box::new(ContentReviewHandler),
        Box::new(RedditHandler),
        Box::new(SocialHandler),
        Box::new(PerformanceHandler),
        Box::new(CoverageHandler),
        Box::new(CtrAuditHandler),
        Box::new(CannibalizationAuditHandler),
        Box::new(ConsolidateClusterHandler),
        Box::new(HubPageHandler),
        Box::new(TerritoryResearchHandler),
        Box::new(ImplementationHandler),
        Box::new(ManualFallbackHandler),
    ]
}

// ─── Step execution helpers (used by executor) ────────────────────────────────

/// Execute a deterministic step by invoking an installed CLI tool via shell.
/// Returns the captured stdout/stderr and success flag.
///
/// The `cmd` param MUST be set explicitly on the WorkflowStep via `.with_param("cmd", "...")`.
/// Auto-generation of CLI commands from step names was removed because it silently produced
/// broken commands (e.g. step `content_review_run` → `pageseeds content review run`).
///
/// Supported tokens in `cmd`:
///   {project_path}   → repo root
///   {automation_dir} → repo/.github/automation
pub async fn exec_deterministic(
    step: &WorkflowStep,
    _task: &Task,
    project_path: &str,
    _seo_provider: &str,
) -> StepResult {
    let paths = ProjectPaths::from_path(project_path);
    let automation_dir = paths.automation_dir.to_string_lossy();

    let raw_cmd = match step.params.get(step_params::CMD) {
        Some(c) => c.clone(),
        None => {
            return StepResult {
                success: false,
                message: format!(
                    "Step '{}' is 'deterministic' but has no 'cmd' param. \
                     Set it via .with_param(\"cmd\", \"pageseeds ...\") in the handler's plan().",
                    step.name
                ),
                output: None,
            };
        }
    };

    let cmd = raw_cmd
        .replace("{project_path}", project_path)
        .replace("{automation_dir}", &automation_dir);

    log::info!("[executor] deterministic step '{}' cmd: {}", step.name, cmd);

    // Run blocking subprocess in spawn_blocking
    let step_name = step.name.clone();
    let project_path = project_path.to_string();
    match tokio::task::spawn_blocking(move || {
        std::process::Command::new("sh")
            .arg("-c")
            .arg(&cmd)
            .current_dir(&project_path)
            .output()
    })
    .await
    {
        Ok(Ok(out)) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            let combined = if stderr.is_empty() {
                stdout.clone()
            } else {
                format!("{}\n[stderr]\n{}", stdout, stderr)
            };
            if out.status.success() {
                StepResult {
                    success: true,
                    message: format!("Step '{}' OK", step_name),
                    output: Some(combined),
                }
            } else {
                StepResult {
                    success: false,
                    message: format!(
                        "Step '{}' failed (exit {}): {}",
                        step_name,
                        out.status,
                        stderr.trim()
                    ),
                    output: Some(combined),
                }
            }
        }
        Ok(Err(e)) => StepResult {
            success: false,
            message: format!("Step '{}' could not launch: {}", step_name, e),
            output: None,
        },
        Err(e) => StepResult {
            success: false,
            message: format!("Step '{}' task failed: {}", step.name, e),
            output: None,
        },
    }
}

/// Execute an agentic step — invokes the configured agent CLI with a built prompt.
///
/// Build order:
///   1. Load skill from step params ("skill" key) → SKILL.md text
///   2. Build a prompt via `prompts::build_prompt`
///   3. Call `agent::run_agent(provider, prompt, project_path)`
///   4. Return the raw output as the step result
pub async fn exec_agentic(
    step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    site_url: &str,
    agent_provider: &str,
    latest_raw_output: Option<&str>,
) -> StepResult {
    use crate::engine::project_paths::ProjectPaths;
    use crate::engine::{agent, prompts, skills};
    use std::path::Path;

    let repo_root = Path::new(project_path);
    let paths = ProjectPaths::from_path(project_path);

    let is_content_task = matches!(
        task.task_type.as_str(),
        "write_article" | "optimize_article" | "create_content" | "optimize_content"
    );

    let content_context = if is_content_task {
        let resolved = crate::content::locator::resolve(repo_root, None);
        resolved.selected.as_ref().map(|dir| {
            (
                dir.clone(),
                snapshot_markdown_mtime(dir),
                detect_numbered_mdx_style(dir),
            )
        })
    } else {
        None
    };

    // 1. Load skill if specified.  A declared skill is required — missing it is
    //    a hard error so the step does not silently degrade to a vague generic
    //    prompt that produces unparseable prose.
    let skill = if let Some(name) = step.params.get("skill") {
        match skills::load_skill(repo_root, name) {
            Some(s) => Some(s),
            None => {
                return StepResult {
                    success: false,
                    message: format!(
                        "Required skill '{}' not found in project repo or app defaults",
                        name
                    ),
                    output: None,
                };
            }
        }
    } else {
        None
    };

    // 2. Optionally load step artifact content into prompt context.
    let artifact_context = if let Some(artifact_name) = step.params.get(step_params::ARTIFACT) {
        let artifact_path = paths.automation_dir.join(artifact_name);
        match std::fs::read_to_string(&artifact_path) {
            Ok(content) => format!(
                "\n\n## Artifact: {}\n\n```json\n{}\n```",
                artifact_name, content
            ),
            Err(_) => {
                return StepResult {
                    success: false,
                    message: format!("{} not found — run collect_gsc first", artifact_name),
                    output: None,
                };
            }
        }
    } else {
        String::new()
    };

    // 3. Build prompt
    let mut prompt = if let Some(ref s) = skill {
        let mut p = prompts::build_prompt(task, s, project_path, Some(site_url)).prompt;
        p.push_str(&artifact_context);
        p
    } else {
        // Fallback prompt when no skill is configured.
        // Include description so the agent knows exactly which file to edit and
        // what checks to fix — avoiding any need for shell-based file discovery.
        let desc_section = task
            .description
            .as_deref()
            .filter(|d| !d.is_empty())
            .map(|d| format!("\n\n## Task Details\n\n{}", d))
            .unwrap_or_default();
        format!(
            "## Task\n\n- ID: {}\n- Type: {}\n- Title: {}\n- Step: {}\n- Site: {}\n- Repo: {}{}\n\nExecute this task step and return the results.",
            task.id,
            task.task_type,
            task.title.as_deref().unwrap_or("(untitled)"),
            step.name,
            site_url,
            project_path,
            desc_section,
        ) + &artifact_context
    };

    // Include embedded task artifacts so follow-up fix tasks receive parent context
    // (e.g. ctr_recommendations, cannibalization_strategy attached by create_*_fix_tasks).
    let task_artifacts: Vec<String> = task
        .artifacts
        .iter()
        .filter_map(|a| {
            a.content
                .as_ref()
                .map(|c| format!("\n\n## Artifact: {}\n\n```\n{}\n```", a.key, c))
        })
        .collect();
    if !task_artifacts.is_empty() {
        prompt.push_str("\n\n## Task Artifacts\n");
        prompt.push_str(&task_artifacts.join("\n"));
    }

    if is_content_task {
        // Pre-compute the next safe publish date and inject it into the prompt.
        // Without this, the agent defaults to today's date which conflicts with
        // articles already in articles.json and breaks the date distribution.
        // Cannot be deterministic-only: the date depends on the current state of
        // articles.json and must be computed from the existing occupied slots.
        if let Some(date) = compute_next_publish_date(project_path) {
            prompt.push_str(&format!(
                "\n\n## Publish Date (Required)\n\
                         - The frontmatter `date:` field MUST be exactly: `{date}`\n\
                         - Do not use today's date or any other value — use the date above."
            ));
        }

        prompt.push_str(
            "\n\n## Content File Format (Required)\n\
                     - New articles must be written as `.mdx` files (never `.md`).\n\
                     - If you propose a filename, it must end in `.mdx`.\n\
                     - Preserve valid frontmatter and markdown/MDX syntax.",
        );

        if let Some((_dir, _before, Some(style))) = &content_context {
            prompt.push_str(&format!(
                "\n\n## Content Filename Convention (Required)\n\
                         - Follow this naming format: `{{id}}_topic_slug.mdx`\n\
                         - Use lowercase with underscores in the slug.\n\
                         - Continue numbering from approximately {}.",
                style.next_id
            ));
        }
    }

    log::info!(
        "[executor] agentic step '{}' with provider '{}' (skill: {:?})",
        step.name,
        agent_provider,
        step.params.get(step_params::SKILL)
    );

    log::info!(
        "[executor] agentic step '{}' with provider '{}' (skill: {:?})",
        step.name,
        agent_provider,
        step.params.get(step_params::SKILL)
    );

    // Check if this is a research workflow step that uses ToolCallingAgent
    // Note: research_final_selection is now deterministic, not agentic
    let is_research_step = matches!(
        step.name.as_str(),
        "research_seed_extraction" | "research_keyword_discovery" | "research_seed_validation"
    );

    if is_research_step {
        // Research steps use the same CLI agent path as all other agentic steps
        return crate::engine::exec::research::exec_research_workflow_step(
            step,
            task,
            project_path,
            agent_provider,
            latest_raw_output,
        )
        .await;
    }

    // 4. Call agent (blocking subprocess, run in spawn_blocking)
    let agent_provider = agent_provider.to_string();
    let prompt = prompt.clone();
    let repo_root = repo_root.to_path_buf();
    let step_name = step.name.clone();

    match tokio::task::spawn_blocking(move || {
        agent::run_agent(&agent_provider, &prompt, &repo_root)
    })
    .await
    {
        Ok(Ok(output)) => {
            let mut message = format!(
                "Agentic step '{}' complete ({} chars)",
                step_name,
                output.len()
            );

            if let Some((content_dir, before, style)) = content_context {
                let renamed = rename_new_or_modified_md_to_mdx(&content_dir, &before);
                if !renamed.is_empty() {
                    message.push_str(&format!(" · enforced MDX on {} file(s)", renamed.len()));
                    for (old, new) in &renamed {
                        log::info!(
                            "[content_mdx] renamed {} -> {}",
                            old.display(),
                            new.display()
                        );
                    }
                }

                if let Some(style) = style {
                    let renamed_style =
                        rename_new_files_to_numbered_mdx(&content_dir, &before, style.next_id);
                    if !renamed_style.is_empty() {
                        message.push_str(&format!(
                            " · normalized naming on {} file(s)",
                            renamed_style.len()
                        ));
                        for (old, new) in &renamed_style {
                            log::info!(
                                "[content_name] renamed {} -> {}",
                                old.display(),
                                new.display()
                            );
                        }
                    }
                }
            }

            StepResult {
                success: true,
                message,
                output: Some(output),
            }
        }
        Ok(Err(err)) => {
            log::warn!("[executor] agentic step '{}' failed: {}", step_name, err);
            StepResult {
                success: false,
                message: format!("Agentic step '{}' failed: {}", step_name, err),
                output: None,
            }
        }
        Err(e) => {
            log::warn!("[executor] agentic step '{}' task failed: {}", step_name, e);
            StepResult {
                success: false,
                message: format!("Agentic step '{}' task failed: {}", step_name, e),
                output: None,
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct NumberedMdxStyle {
    next_id: i64,
}

fn detect_numbered_mdx_style(dir: &std::path::Path) -> Option<NumberedMdxStyle> {
    let mut count = 0i64;
    let mut max_id = 0i64;

    for path in crate::content::locator::collect_markdown_files(dir) {
        let is_mdx = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("mdx"))
            .unwrap_or(false);
        if !is_mdx {
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if let Some(id) = parse_numeric_prefix(name) {
            count += 1;
            if id > max_id {
                max_id = id;
            }
        }
    }

    // Only enforce when this style is clearly established in the repo.
    if count >= 5 {
        Some(NumberedMdxStyle {
            next_id: max_id + 1,
        })
    } else {
        None
    }
}

fn parse_numeric_prefix(filename: &str) -> Option<i64> {
    let prefix = filename.split_once('_')?.0;
    if prefix.chars().all(|c| c.is_ascii_digit()) {
        prefix.parse::<i64>().ok()
    } else {
        None
    }
}

fn normalize_slug_underscored(stem: &str) -> String {
    let mut out = String::new();
    let mut prev_sep = false;

    for ch in stem.chars() {
        let c = ch.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_sep = false;
        } else if !prev_sep {
            out.push('_');
            prev_sep = true;
        }
    }

    let trimmed = out.trim_matches('_').to_string();
    if trimmed.is_empty() {
        "article".to_string()
    } else {
        trimmed
    }
}

fn rename_new_files_to_numbered_mdx(
    dir: &std::path::Path,
    before: &std::collections::HashMap<std::path::PathBuf, std::time::SystemTime>,
    start_id: i64,
) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    let mut renamed = Vec::new();
    let mut next_id = start_id;

    for path in crate::content::locator::collect_markdown_files(dir) {
        // Rename only newly created files from this run, not existing repo files.
        if before.contains_key(&path) {
            continue;
        }

        let is_mdx = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("mdx"))
            .unwrap_or(false);
        if !is_mdx {
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if parse_numeric_prefix(name).is_some() {
            continue;
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("article");
        let slug = normalize_slug_underscored(stem);

        let target = loop {
            let candidate = dir.join(format!("{}_{}.mdx", next_id, slug));
            if !candidate.exists() {
                break candidate;
            }
            next_id += 1;
        };

        if std::fs::rename(&path, &target).is_ok() {
            renamed.push((path, target));
            next_id += 1;
        }
    }

    renamed
}

fn snapshot_markdown_mtime(
    dir: &std::path::Path,
) -> std::collections::HashMap<std::path::PathBuf, std::time::SystemTime> {
    let mut out = std::collections::HashMap::new();
    for path in crate::content::locator::collect_markdown_files(dir) {
        if let Ok(meta) = std::fs::metadata(&path) {
            if let Ok(mtime) = meta.modified() {
                out.insert(path, mtime);
            }
        }
    }
    out
}

/// Load keyword coverage from the automation directory and format it for the agent prompt.
///
/// Returns a formatted string describing current topic clusters, or an empty string
/// if no coverage analysis exists.
#[allow(dead_code)]
fn load_coverage_context(automation_dir: &std::path::Path) -> String {
    let coverage_path = automation_dir.join("keyword_coverage.json");

    let content = match std::fs::read_to_string(&coverage_path) {
        Ok(c) => c,
        Err(_) => return String::new(), // No coverage file yet
    };

    let coverage: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };

    let clusters = coverage.get("clusters").and_then(|c| c.as_array());
    let article_count = coverage
        .get("article_count")
        .and_then(|a| a.as_i64())
        .unwrap_or(0);

    if clusters.is_none() || clusters.unwrap().is_empty() {
        return String::new();
    }

    let clusters = clusters.unwrap();
    let cluster_summaries: Vec<String> = clusters
        .iter()
        .filter_map(|c| {
            let name = c.get("cluster_name").and_then(|n| n.as_str())?;
            let keywords = c.get("primary_keywords").and_then(|k| k.as_array())?;
            let count = c.get("article_count").and_then(|n| n.as_i64()).unwrap_or(0);

            let keyword_list: Vec<&str> = keywords.iter().filter_map(|k| k.as_str()).collect();

            Some(format!(
                "- {} ({} articles): focus on {}",
                name,
                count,
                keyword_list.join(", ")
            ))
        })
        .collect();

    if cluster_summaries.is_empty() {
        return String::new();
    }

    format!(
        "## Current Keyword Coverage (from previous analysis)\n\
         This project has {} articles organized into {} topic clusters:\n\
         {}\n\
         \n\
         ## Gap-Filling Guidance\n\
         When selecting themes, consider:\n\
         1. Which clusters are under-represented and need more content?\n\
         2. What related topics are NOT yet covered by existing clusters?\n\
         3. What sub-topics within large clusters could use dedicated articles?\n\
         Prioritize themes that fill gaps or expand thinly-covered areas.",
        article_count,
        clusters.len(),
        cluster_summaries.join("\n")
    )
}

/// Read articles.json from `{project_path}/.github/automation/articles.json` and return
/// the next unoccupied past date for a new article.
///
/// Implements the same logic as `content::date_policy::suggest_next_safe_date` but reads
/// dates directly from the on-disk JSON file instead of requiring a DB connection, so it
/// can be called from inside `exec_agentic` which has no access to SQLite.
pub(crate) fn compute_next_publish_date(project_path: &str) -> Option<String> {
    use chrono::{Duration, NaiveDate, Utc};
    use std::collections::HashSet;

    let articles_path = std::path::Path::new(project_path)
        .join(".github")
        .join("automation")
        .join("articles.json");

    let json = std::fs::read_to_string(&articles_path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&json).ok()?;
    let articles = value.get("articles")?.as_array()?;

    let occupied: HashSet<NaiveDate> = articles
        .iter()
        .filter_map(|a| a["published_date"].as_str())
        .filter(|d| !d.is_empty())
        .filter_map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
        .collect();

    let today = Utc::now().date_naive();
    let mut cursor = today - Duration::days(1);
    while occupied.contains(&cursor) {
        cursor -= Duration::days(1);
    }
    Some(cursor.format("%Y-%m-%d").to_string())
}

fn rename_new_or_modified_md_to_mdx(
    dir: &std::path::Path,
    before: &std::collections::HashMap<std::path::PathBuf, std::time::SystemTime>,
) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    let mut renamed = Vec::new();

    for path in crate::content::locator::collect_markdown_files(dir) {
        let is_md = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("md"))
            .unwrap_or(false);
        if !is_md {
            continue;
        }

        let modified = std::fs::metadata(&path)
            .ok()
            .and_then(|m| m.modified().ok());

        let changed_since_before = match (before.get(&path), modified) {
            (None, Some(_)) => true,
            (Some(prev), Some(now)) => now > *prev,
            _ => false,
        };

        if !changed_since_before {
            continue;
        }

        let target = path.with_extension("mdx");
        if target.exists() {
            log::warn!(
                "[content_mdx] skipping rename {} -> {} because target exists",
                path.display(),
                target.display()
            );
            continue;
        }

        if std::fs::rename(&path, &target).is_ok() {
            renamed.push((path, target));
        }
    }

    renamed
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    use crate::config::task_definitions;
    use crate::models::task::{AgentPolicy, ExecutionMode, Priority, Task, TaskRun, TaskStatus};

    fn make_task(task_type: &str) -> Task {
        Task {
            id: format!("test-{task_type}"),
            task_type: task_type.to_string(),
            phase: "research".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            execution_mode: ExecutionMode::Manual,
            agent_policy: AgentPolicy::Optional,
            title: Some(format!("{task_type} test")),
            description: None,
            project_id: "proj1".to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun {
                attempts: 0,
                last_error: None,
                provider: None,
                ..Default::default()
            },
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Every task type in task_definitions::all() must match a real handler.
    /// This catches silent registration failures where a new task type is added
    /// to the registry but not to the handler registry.
    ///
    /// Intentional placeholder handlers (e.g. PerformanceHandler returning Manual)
    /// are allowed — they are registered and documented. Only the
    /// ManualFallbackHandler (always last in default_handlers()) is rejected.
    #[test]
    #[cfg(debug_assertions)]
    fn all_task_types_have_non_fallback_handler() {
        let handlers = default_handlers();

        let definitions = task_definitions::all();
        for def in definitions {
            let task = make_task(def.task_type);
            let matched_idx = handlers.iter().position(|h| h.supports(&task));
            assert!(
                matched_idx.is_some(),
                "Task type '{}' has no handler at all",
                def.task_type
            );
            // The last handler is always ManualFallbackHandler
            let is_fallback = matched_idx.unwrap() == handlers.len() - 1;
            assert!(
                !is_fallback,
                "Task type '{}' falls through to ManualFallbackHandler. Add a real handler.",
                def.task_type
            );
        }

        log::info!(
            "[registry_test] All {} task types have non-fallback handlers",
            definitions.len()
        );
    }
}
