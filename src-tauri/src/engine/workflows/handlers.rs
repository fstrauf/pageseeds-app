/// Trait-based workflow handlers — one per task family.
///
/// Each handler knows:
///   - which task types it owns (`supports`)
///   - what steps the task needs (`plan`)
///
/// Step execution happens in `executor.rs`; handlers only describe the plan.
use super::{step_params, StepKind, StepResult, WorkflowStep};
use crate::engine::project_paths::ProjectPaths;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

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
        matches!(
            task_type(task),
            "collect_gsc" | "collect_posthog" | "collect_clarity"
        )
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task_type(task) {
            "collect_gsc" => vec![WorkflowStep::new(
                "collect_gsc_inspect",
                StepKind::CollectGscInspect,
            )],
            "collect_clarity" => vec![WorkflowStep::new(
                "collect_clarity_export",
                StepKind::CollectClarity,
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
        matches!(
            task_type(task),
            "investigate_gsc" | "investigate_posthog" | "investigate_clarity" | "clarity_analytics"
        )
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
            "investigate_clarity" => vec![
                // Step 1 (deterministic): aggregate per-page behavioral metrics and compute
                // anomaly scores. Writes clarity_summary.json.
                WorkflowStep::new("clarity_summarise", StepKind::ClaritySummarise),
                // Step 2 (agentic): interpret the scores, group issues, and produce ranked
                // findings with dashboard links. Requires judgment about UX/SEO significance.
                WorkflowStep::new(
                    "clarity_investigate_agent",
                    StepKind::ClarityInvestigateAgentic,
                ),
            ],
            "clarity_analytics" => vec![
                // Step 1 (deterministic): fetch the latest Clarity Export API data.
                // Stores flattened rows in SQLite and writes clarity_collection.json.
                WorkflowStep::new("collect_clarity_export", StepKind::CollectClarity),
                // Step 2 (deterministic): aggregate per-page behavioral metrics and compute
                // anomaly scores. Writes clarity_summary.json.
                WorkflowStep::new("clarity_summarise", StepKind::ClaritySummarise),
                // Step 3 (agentic): interpret the scores and produce ranked findings.
                WorkflowStep::new(
                    "clarity_investigate_agent",
                    StepKind::ClarityInvestigateAgentic,
                ),
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
            "research_keywords" | "custom_keyword_research" | "research_landing_pages" | "update_research_shortlist"
        )
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        match task_type(task) {
            "update_research_shortlist" => vec![
                // Deterministic: run territory analysis and sync to SQLite shortlist.
                WorkflowStep::new(
                    "research_territory_analysis",
                    StepKind::ResearchTerritoryAnalysis,
                ),
            ],
            "research_keywords" | "research_landing_pages" => {
                // 7-step hybrid workflow:
                // deterministic → deterministic → agentic → deterministic → agentic → deterministic → deterministic
                vec![
                    // Step 1 (deterministic): Ensure coverage data is fresh (< 7 days).
                    // If stale or missing, runs coverage analysis inline. Fully invisible to user.
                    WorkflowStep::new("ensure_coverage_fresh", StepKind::EnsureCoverageFresh),
                    // Step 2 (deterministic): Territory analysis — reads articles + GSC data,
                    // groups by target_keyword, identifies open territories and saturated themes.
                    // Writes findings to the persistent research_shortlist SQLite table.
                    WorkflowStep::new(
                        "research_territory_analysis",
                        StepKind::ResearchTerritoryAnalysis,
                    ),
                    // Step 3 (agentic): LLM extracts 3-4 themes from project brief.
                    // Uses rig Extractor<T> for guaranteed structured JSON output.
                    // Cannot be deterministic: requires reading intent from free-form text.
                    // Now also reads the research_shortlist to prioritize open territories.
                    WorkflowStep::new("research_seed_extraction", StepKind::Agentic),
                    // Step 4 (deterministic): fetch Google Autocomplete for all themes.
                    // Free API, always returns results. Outputs structured JSON: [{theme, suggestions}].
                    WorkflowStep::new("research_autocomplete", StepKind::ResearchAutocomplete),
                    // Step 5 (agentic): LLM filters autocomplete suggestions for domain relevance.
                    // Uses rig Extractor<T> for guaranteed structured JSON output.
                    // Cannot be deterministic: requires understanding what is on-topic for this
                    // specific product/site. Hard-coding a relevance rule would produce silent errors
                    // on any input it was not tested against.
                    // Input contract: [{theme, suggestions: [string]}]
                    // Output contract: {validated_seeds: [{theme: string, seeds: [string]}]}
                    WorkflowStep::new("research_seed_validation", StepKind::Agentic),
                    // Step 6 (deterministic): DataForSEO related_keywords per validated seed.
                    // Deterministic: given validated seeds, fetches keyword ideas + KD + volume.
                    // Also consumes pending territory themes from research_shortlist as extra seeds.
                    WorkflowStep::new("research_ahrefs_pipeline", StepKind::KeywordResearchNative)
                        .with_latest_raw_policy(
                            crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput,
                        ),
                    // Step 7 (deterministic): Select best candidates from structured data.
                    // Outputs clean JSON directly — no normalizer needed because upstream
                    // agentic steps now use Extractor<T>.
                    WorkflowStep::new("research_final_selection", StepKind::ResearchFinalSelection),
                ]
            }
            "custom_keyword_research" => {
                // Streamlined pipeline for user-provided keywords.
                // Skips agentic seed extraction / autocomplete / validation —
                // the user already knows what they want to research.
                // Reads themes directly from task.description (one per line).
                vec![
                    // Step 1 (deterministic): Ensure coverage data is fresh.
                    WorkflowStep::new("ensure_coverage_fresh", StepKind::EnsureCoverageFresh),
                    WorkflowStep::new("research_ahrefs_pipeline", StepKind::KeywordResearchNative)
                        .with_latest_raw_policy(
                            crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput,
                        ),
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
                | "create_hub_page"
                | "refresh_hub_page"
        )
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        // Agentic: the agent reads the article spec and writes the MDX file.
        let has_hub_brief = task.artifacts.iter().any(|a| a.key == "hub_brief");
        let is_hub =
            has_hub_brief || matches!(task_type(task), "create_hub_page" | "refresh_hub_page");
        let step = WorkflowStep::new("content_write_stage", StepKind::Agentic);
        if is_hub {
            vec![step.with_param(step_params::SKILL, "hub-write")]
        } else {
            vec![step]
        }
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
                | "interlinking"
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
                | "gsc_indexing_recovery"
                | "fix_indexing_internal_links"
                | "gsc_indexing_outcome_review"
                | "indexing_health_campaign"
                | "generate_feature_spec"
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
                vec![WorkflowStep::new(
                    "publish_content_validate",
                    StepKind::FormatValidation,
                )]
            }
            "fix_content_article" => vec![
                // Step 1 (deterministic): load the article's recommendations from
                // recommendations.json and read the current file state. Builds structured
                // context consumed by the generate step.
                WorkflowStep::new(
                    "fix_content_article_context",
                    StepKind::FixContentArticleContext,
                )
                .with_latest_raw_policy(
                    crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput,
                )
                .with_param(step_params::ARTIFACT_NAME, "content_fix_context"),
                // Step 2 (agentic): generate structured ContentFixPatch via Rig extraction.
                // Cannot be deterministic: the agent must interpret recommendations and write
                // prose that satisfies SEO rules, brand voice, and keyword context.
                // Input contract: structured context with file content + recommendations.
                // Output contract: ContentFixPatch JSON.
                WorkflowStep::new("fix_content_article_generate", StepKind::FixContentArticleGenerate)
                    .with_param(step_params::SKILL, "content-fix-apply")
                    .with_param(step_params::ARTIFACT_NAME, "content_fix_patch"),
                // Step 3 (deterministic): apply the patch to the MDX file.
                // Snapshots original, replaces frontmatter/body, validates structure, restores on corruption.
                WorkflowStep::new("fix_content_article_apply", StepKind::FixContentArticleApply),
                // Step 4 (deterministic): re-run health checks to verify fixes meet thresholds.
                // Produces ContentFixVerificationReport. Status = done if all pass, review if partial.
                WorkflowStep::new("fix_content_article_verify", StepKind::FixContentArticleVerify),
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
                // Uses rig structured extraction (CtrFixGenerate) instead of raw agentic text.
                // Input contract: single CtrRecommendation artifact + file contents.
                // Output contract: CtrFixPatch JSON.
                WorkflowStep::new("fix_ctr_article_generate", StepKind::CtrFixGenerate)
                    .with_param(step_params::SKILL, "ctr-fix-apply")
                    .with_param(step_params::ARTIFACT_NAME, "ctr_fix_patch"),
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
            "indexing_health_campaign" => vec![
                // Step 1 (deterministic): check prerequisite artifacts for freshness.
                // Writes indexing_prerequisites.json. Auto-runnable prerequisites are enqueued
                // by post-action; manual ones surface in the task output.
                WorkflowStep::new("ihc_check_prerequisites", StepKind::IhcCheckPrerequisites),
                // Step 2 (deterministic): compute drift from current sitemap/GSC/link data.
                // Reuses existing drift computation.
                WorkflowStep::new("ihc_drift_analysis", StepKind::GscRecoveryDrift),
                // Step 3 (deterministic): build per-target cluster context for not-indexed URLs.
                // Loads cannibalization clusters and content audit, matches each URL to siblings.
                WorkflowStep::new("ihc_build_target_context", StepKind::IhcBuildTargetContext),
                // Step 4 (deterministic): run or refresh content audit.
                WorkflowStep::new("ihc_content_audit", StepKind::ContentAudit),
                // Step 5 (agentic): judge title/H1 distinctiveness against cluster siblings.
                // One agent call per target. Uses indexing-distinctiveness skill.
                WorkflowStep::new("ihc_distinctiveness_review", StepKind::IhcDistinctivenessReview)
                    .with_param(step_params::SKILL, "indexing-distinctiveness"),
                // Step 6 (deterministic): merge all inputs into campaign plan.
                // Writes indexing_campaign_plan.json consumed by post-actions.
                WorkflowStep::new("ihc_reduce_plan", StepKind::IhcReducePlan),
            ],
            "fix_indexing" | "fix_technical" => vec![
                // Step 1 (deterministic): load the target MDX file and extract structured context
                // (word count, H1, title, internal links, canonical). This is obvious file I/O —
                // no judgment required — and saves the agent from hunting around the repo.
                WorkflowStep::new("indexing_fix_context", StepKind::IndexingFixContext)
                    .with_latest_raw_policy(
                        crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput,
                    ),
                // Step 2 (agentic): apply the fix. The agent gets the GSC issue + structured
                // context and edits the MDX file directly. Judgment is required because the fix
                // depends on intent, content quality, and site-specific conventions.
                WorkflowStep::new("indexing_fix_apply", StepKind::IndexingFixApply),
            ],
            "cluster_and_link" | "interlinking" => vec![
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
            "gsc_indexing_recovery" => vec![
                // Step 1 (deterministic): refresh stale GSC and link data before planning.
                // Uses shared collection helpers. Writes refreshed gsc_collection.json and link_scan.json.
                WorkflowStep::new("gsc_recovery_prepare", StepKind::GscRecoveryPrepare),
                // Step 2 (deterministic): compute drift from current sitemap/GSC/link data.
                // Reuses existing drift computation. No hidden repair work.
                WorkflowStep::new("gsc_recovery_drift", StepKind::GscRecoveryDrift),
                // Step 3 (deterministic): filter, score eligible targets, build source candidates,
                // write gsc_recovery_plan artifact consumed by post-actions.
                WorkflowStep::new("gsc_recovery_plan", StepKind::GscRecoveryPlan),
            ],
            "fix_indexing_internal_links" => vec![
                // Step 1 (deterministic): build compact per-target context from the target artifact,
                // current link scan, article metadata, and source files.
                WorkflowStep::new("indexing_link_context", StepKind::IndexingLinkContext),
                // Step 2 (agentic): choose relevant source and anchor from the shortlist.
                // Requires topical judgment. Uses existing prompt-based pattern in V1.
                WorkflowStep::new("indexing_link_plan", StepKind::IndexingLinkPlan),
                // Step 3 (deterministic): apply Related Articles links to source MDX files.
                // Reuses existing append_related_section logic.
                WorkflowStep::new("indexing_link_apply", StepKind::IndexingLinkApply),
                // Step 4 (deterministic): rescan link graph, verify target gained inbound links.
                // Fails or moves to review if no inbound link was added.
                WorkflowStep::new("indexing_link_verify", StepKind::IndexingLinkVerify),
            ],
            "gsc_indexing_outcome_review" => vec![
                // Step 1 (deterministic): re-inspect target URL in GSC after wait period.
                WorkflowStep::new(
                    "gsc_indexing_outcome_inspect",
                    StepKind::GscIndexingOutcomeInspect,
                ),
                // Step 2 (deterministic): compare before/after status, write outcome report.
                WorkflowStep::new(
                    "gsc_indexing_outcome_report",
                    StepKind::GscIndexingOutcomeReport,
                ),
            ],
            "create_landing_page" | "landing_page_spec" => vec![
                // Deterministic: build a structured spec file from keyword metadata
                // already on the task. No LLM needed — the spec is a structured template
                // populated with keyword, page type, intent, volume, and KD.
                WorkflowStep::new("landing_page_spec_write", StepKind::LandingPageSpecWrite),
            ],
            "generate_feature_spec" => vec![
                // Agentic: read all audit artifacts, synthesize findings via LLM,
                // and write a prioritized developer feature spec to the automation dir.
                // Distinguishes code fixes (P0), content fixes (P1), and structural changes (P2).
                WorkflowStep::new("generate_feature_spec", StepKind::GenerateFeatureSpec)
                    .with_param(step_params::SKILL, "feature-spec-generation"),
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
                WorkflowStep::new("ctr_build_context", StepKind::CtrBuildContext)
                    .with_latest_raw_policy(
                        crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput,
                    ),
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
            // Step 1 (deterministic): Sync latest GSC data. Optional — clustering
            // works without GSC; articles with zero GSC data are still included.
            WorkflowStep::new("can_gsc_sync", StepKind::GscSyncArticles).optional(),
            // Step 2 (deterministic): Load articles from articles.json or live-site inventory.
            WorkflowStep::new("can_coverage_load", StepKind::CoverageLoadArticles),
            // Step 3 (deterministic): Compute TF-IDF similarity matrix + write reference artifacts.
            // Returns a compact summary; full context is written to disk for downstream steps.
            WorkflowStep::new("can_build_context", StepKind::CanBuildContext)
                .with_latest_raw_policy(crate::engine::workflows::LatestRawPolicy::Clear),
            // Step 4 (deterministic): Detect exact duplicate target keywords + rank by GSC.
            // Writes exact_keyword_duplicates.json. These are guaranteed overlap cases.
            WorkflowStep::new("can_exact_keyword_dupes", StepKind::CanExactKeywordDupes),
            // Step 5 (deterministic): Select merge candidates from clusters.
            // Also injects exact-keyword-duplicate groups as high-priority candidates.
            // Splits giant components by target keyword, caps pages at 8 per candidate.
            WorkflowStep::new("can_select_candidates", StepKind::CanSelectCandidates),
            // Step 6 (agentic): Analyze individual merge candidates with byte-budgeted prompts.
            // One agent call per candidate to stay under the Kimi bridge limit.
            WorkflowStep::new("can_analyze_candidates", StepKind::CanAnalyzeCandidates)
                .with_param(step_params::SKILL, "cannibalization-strategy"),
            // Step 7 (deterministic): Merge batch outputs into final strategy JSON.
            // Validates recommendations and includes deterministic hub/territory data.
            WorkflowStep::new("can_reduce_strategy", StepKind::CanReduceStrategy)
                .with_param(step_params::ARTIFACT_NAME, "cannibalization_strategy"),
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
            WorkflowStep::new("merge_extract_sections", StepKind::MergeExtractSections)
                .with_latest_raw_policy(
                    crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput,
                ),
            // Step 4 (agentic): Draft ContentMergePatch JSON deciding which unique content belongs in keeper.
            // Cannot be deterministic: understanding whether a section adds unique value requires judgment.
            // Input contract: structured JSON with keeper content + extracted unique sections.
            // Output contract: ContentMergePatch JSON.
            WorkflowStep::new("merge_draft_patch", StepKind::MergeDraftPatch)
                .with_latest_raw_policy(
                    crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput,
                )
                .with_param(step_params::SKILL, "merge-content"),
            // Step 5 (deterministic): Apply structured patch, snapshot original, validate MDX/frontmatter.
            WorkflowStep::new("merge_apply_patch", StepKind::MergeApplyPatch),
            // Step 6 (deterministic): Generate redirect rules as generic CSV.
            WorkflowStep::new("merge_generate_redirects", StepKind::MergeGenerateRedirects),
            // Step 7 (deterministic): Validate merged keeper and redirect map.
            WorkflowStep::new("merge_validate_output", StepKind::MergeValidateOutput),
            // Step 8 (deterministic): Sync merged articles back to SQLite and articles.json.
            WorkflowStep::new("merge_sync_articles", StepKind::MergeSyncArticles),
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
            WorkflowStep::new("territory_build_context", StepKind::TerritoryBuildContext)
                .with_latest_raw_policy(
                    crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput,
                ),
            // Step 3 (agentic): Generate TerritoryStrategy JSON from context.
            // Cannot be deterministic: deciding which gaps to fill, what competitors cover,
            // and how to avoid cannibalization requires semantic judgment.
            // Input contract: structured TerritoryContext JSON.
            // Output contract: TerritoryStrategy JSON.
            WorkflowStep::new("territory_strategy", StepKind::TerritoryStrategy)
                .with_latest_raw_policy(
                    crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput,
                )
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
        Box::new(CtrAuditHandler),
        Box::new(CannibalizationAuditHandler),
        Box::new(ConsolidateClusterHandler),
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
fn hub_spoke_context(task: &Task, project_path: &str) -> String {
    let hub_topic = task
        .title
        .as_deref()
        .and_then(|t| {
            t.strip_prefix("Create hub:")
                .or_else(|| t.strip_prefix("Refresh hub:"))
        })
        .unwrap_or("")
        .trim();

    if hub_topic.is_empty() {
        return String::new();
    }

    // Try focused hub_brief artifact first (new write_article path)
    let hub_brief_json = task
        .artifacts
        .iter()
        .find(|a| a.key == "hub_brief")
        .and_then(|a| a.content.clone());

    let (suggested_url, suggested_title, spoke_pages) = if let Some(brief_json) = hub_brief_json {
        match serde_json::from_str::<serde_json::Value>(&brief_json) {
            Ok(brief) => {
                let url = brief
                    .get("suggested_url")
                    .and_then(|u| u.as_str())
                    .unwrap_or("")
                    .to_string();
                let title = brief
                    .get("suggested_title")
                    .and_then(|t| t.as_str())
                    .unwrap_or(hub_topic)
                    .to_string();
                let pages = brief
                    .get("spoke_pages")
                    .and_then(|p| p.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect::<Vec<_>>())
                    .unwrap_or_default();
                (url, title, pages)
            }
            Err(_) => return String::new(),
        }
    } else {
        // Legacy path: parse from full cannibalization_strategy artifact
        let strategy_json = task
            .artifacts
            .iter()
            .find(|a| a.key == "cannibalization_strategy")
            .and_then(|a| a.content.clone())
            .unwrap_or_default();

        if strategy_json.is_empty() {
            return String::new();
        }

        let strategy: serde_json::Value = match serde_json::from_str(&strategy_json) {
            Ok(v) => v,
            Err(_) => return String::new(),
        };

        let recommendations = strategy
            .get("hub_recommendations")
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();

        let rec = recommendations.iter().find(|r| {
            r.get("suggested_title")
                .and_then(|t| t.as_str())
                .map(|t| t.trim().eq_ignore_ascii_case(hub_topic))
                .unwrap_or(false)
                || r.get("topic")
                    .and_then(|t| t.as_str())
                    .map(|t| t.trim().eq_ignore_ascii_case(hub_topic))
                    .unwrap_or(false)
        });

        let rec = match rec {
            Some(r) => r,
            None => return String::new(),
        };

        let url = rec
            .get("suggested_url")
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string();
        let title = rec
            .get("suggested_title")
            .and_then(|t| t.as_str())
            .unwrap_or(hub_topic)
            .to_string();
        let pages = rec
            .get("spoke_pages")
            .and_then(|p| p.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_i64()).collect::<Vec<_>>())
            .unwrap_or_default();
        (url, title, pages)
    };

    let db_path = crate::db::default_db_path();
    let mut spokes = match rusqlite::Connection::open(&db_path) {
        Ok(conn) => {
            if spoke_pages.is_empty() {
                Vec::new()
            } else {
                crate::engine::exec::content::hub_page::gather_spoke_briefs(
                    &conn,
                    &task.project_id,
                    project_path,
                    &spoke_pages,
                )
            }
        }
        Err(_) => Vec::new(),
    };

    // Sort by impressions (highest first) and cap at 8 to stay within prompt budget.
    spokes.sort_by(|a, b| {
        b.impressions
            .partial_cmp(&a.impressions)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    const MAX_SPOKES: usize = 8;
    spokes.truncate(MAX_SPOKES);

    let mut ctx = format!(
        "\n\n## Hub Page Task\n\n\
         You are writing a pillar / hub page MDX document.\n\
         - Topic: {}\n\
         - Title: {}\n\
         - URL slug: {}\n",
        hub_topic, suggested_title, suggested_url
    );

    if !spokes.is_empty() {
        ctx.push_str("\n### Spoke Articles to Connect\n\n");
        for spoke in &spokes {
            ctx.push_str(&format!(
                "- **{}** (`{}`)\n  Summary: {}\n\n",
                spoke.title,
                crate::content::slug::format_blog_link(&spoke.url_slug),
                spoke.excerpt
            ));
        }
    }

    const MAX_HUB_CONTEXT_BYTES: usize = 40_000;
    if ctx.len() > MAX_HUB_CONTEXT_BYTES {
        log::warn!(
            "[hub_spoke_context] context too large ({} bytes) for hub '{}'; truncating",
            ctx.len(),
            hub_topic
        );
        let mut truncated = ctx.chars().take(MAX_HUB_CONTEXT_BYTES).collect::<String>();
        truncated.push_str("\n\n[…hub context truncated…]\n");
        return truncated;
    }

    ctx
}

// ─────────────────────────────────────────────────────────────────────────────
// Kimi backend routing helper
// ─────────────────────────────────────────────────────────────────────────────

/// Return the `X-Kimi-Backend` header value for a given task/step.
///
/// Content-writing tasks use ACP because generation reliably takes 160–170s,
/// which exceeds the bridge's direct-mode hard timeout (120s). ACP has a
/// 300s timeout and can handle long-running completions.
///
/// Non-writing tasks use direct mode: it is stateless, fast, and reliable.
fn kimi_backend_preference_for_step(task: &Task, _step: &WorkflowStep) -> Option<&'static str> {
    match task.task_type.as_str() {
        "write_article" | "optimize_article" | "create_content" | "optimize_content"
        | "create_hub_page" | "refresh_hub_page" => Some("acp"),
        _ => Some("direct"),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Prompt budget preflight
// ─────────────────────────────────────────────────────────────────────────────

struct PromptBudget {
    target: usize,
    hard: usize,
}

fn default_budget_for_backend(_backend_preference: Option<&str>) -> PromptBudget {
    // Defaults mirror the Kimi bridge limits until health data is wired.
    // Bridge hard limit is 100 KB based on live evidence (reddit_enrich ~25 KB,
    // CTR audit ~46 KB). Target leaves headroom for JSON/preamble overhead.
    PromptBudget {
        target: 80 * 1024,
        hard: 90 * 1024,
    }
}

fn estimate_prompt_bytes(prompt: &str) -> usize {
    prompt.len()
}

pub async fn exec_agentic(
    step: &WorkflowStep,
    task: &Task,
    project_path: &str,
    site_url: &str,
    agent_provider: &str,
    latest_raw_output: Option<&str>,
    next_publish_date: Option<String>,
) -> StepResult {
    use crate::engine::project_paths::ProjectPaths;
    use crate::engine::{agent, prompts, skills};
    use std::path::Path;

    let repo_root = Path::new(project_path);
    let paths = ProjectPaths::from_path(project_path);

    let is_content_task = matches!(
        task.task_type.as_str(),
        "write_article"
            | "optimize_article"
            | "create_content"
            | "optimize_content"
            | "create_hub_page"
            | "refresh_hub_page"
            | "fix_content_article"
    );
    let is_new_article_task = matches!(
        task.task_type.as_str(),
        "write_article" | "create_content" | "create_hub_page" | "refresh_hub_page"
    );
    let is_hub_task = step.params.get(step_params::SKILL).map(|s| s.as_str()) == Some("hub-write");

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
        .filter(|a| {
            // Hub tasks get focused hub context via hub_spoke_context(); inlining the
            // full cannibalization_strategy here duplicates data and blows the prompt
            // budget (it can be 20-90KB). Skip it for hub tasks.
            !(is_hub_task && a.key == "cannibalization_strategy")
        })
        .filter_map(|a| {
            a.content.as_ref().map(|c| {
                const MAX_ARTIFACT_CHARS: usize = 10_000;
                let preview = if c.len() > MAX_ARTIFACT_CHARS {
                    format!(
                        "{}… [truncated]",
                        crate::engine::text::char_prefix(c, MAX_ARTIFACT_CHARS)
                    )
                } else {
                    c.clone()
                };
                format!("\n\n## Artifact: {}\n\n```\n{}\n```", a.key, preview)
            })
        })
        .collect();
    if !task_artifacts.is_empty() {
        prompt.push_str("\n\n## Task Artifacts\n");
        prompt.push_str(&task_artifacts.join("\n"));
    }

    if is_content_task {
        // Pre-compute the next safe publish date and inject it into the prompt.
        // Without this, the agent defaults to today's date which conflicts with
        // articles already in the database and breaks the date distribution.
        // We read from SQLite (canonical source of truth) rather than articles.json
        // so queued tasks see the most current state after orphan ingestion.
        if let Some(ref date) = next_publish_date {
            if is_new_article_task {
                prompt.push_str(&format!(
                    "\n\n## Publish Date (Required)\n\
                             - The frontmatter `date:` field MUST be exactly: `{date}`\n\
                             - Do not use today's date or any other value — use the date above."
                ));
            } else {
                // Modification tasks: preserve existing date, avoid collisions
                prompt.push_str(&format!(
                    "\n\n## Publish Date (Preserve)\n\
                             - Preserve the existing `date:` field in the frontmatter.\n\
                             - Do NOT change it to today's date, a future date, or a date already used by another article.\n\
                             - If the article has no date, use exactly: `{date}`"
                ));
            }
        }

        prompt.push_str(
            "\n\n## Content File Format (Required)\n\
                     - New articles must be written as `.mdx` files (never `.md`).\n\
                     - If you propose a filename, it must end in `.mdx`.\n\
                     - Preserve valid frontmatter and markdown/MDX syntax.",
        );

        prompt.push_str(
            "\n\n## Internal Link Format (Required)\n\
                     - All internal links MUST use standard markdown syntax: `[anchor text](/blog/slug)`\n\
                     - The URL path must be wrapped in parentheses `()` immediately after the closing bracket `]`.\n\
                     - WRONG: `[anchor text]/blog/slug` or `[anchor text] /blog/slug`\n\
                     - CORRECT: `[anchor text](/blog/slug)`\n\
                     - If you include a 'Related Articles' section, use the same `[title](/blog/slug)` format for every bullet.",
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

    if is_hub_task {
        prompt.push_str(&hub_spoke_context(task, project_path));
        prompt.push_str(
            "\n\n## Hub Page Requirements\n\
             - Write a comprehensive pillar / hub page MDX document.\n\
             - YAML frontmatter MUST include `type: hub` and `hub_topic`.\n\
             - Include an H1 matching the hub title.\n\
             - Link to every spoke article using `/blog/{slug}` format.\n\
             - Total word count MUST be 1500+ words.\n\
             - The frontmatter `title:` and the body H1 must be complete, grammatically correct phrases. They must NOT end mid-sentence or with dangling words such as `a`, `an`, `the`, `and`, `or`, `to`, `for`, `of`, `in`, `on`, `with`, `by`, `from`, `as`, `is`, `are`, `what`, `how`, `when`, `where`, `why`, `which`, `complete`, `guide`, `income`, `without`, `track`, `close`, `compared`, or trailing punctuation (`:`, `,`, `-`). Rewrite rather than truncate.\n\
             - The first body paragraph must begin with a complete sentence; do not drop leading characters from the opening sentence.\n\
             - Return ONLY the complete MDX content. No explanations outside the MDX.\n",
        );
    }

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

    let backend_preference = kimi_backend_preference_for_step(task, step);

    // Prompt budget preflight — fail before the provider call if the prompt
    // exceeds the hard budget for the chosen backend.
    let prompt_bytes = estimate_prompt_bytes(&prompt);
    let budget = default_budget_for_backend(backend_preference);
    if prompt_bytes > budget.hard {
        return StepResult {
            success: false,
            message: format!(
                "Prompt size ({} bytes) exceeds hard budget ({} bytes) for step '{}'. \
                 Trim artifacts, reduce context, or batch the workflow before retrying.",
                prompt_bytes, budget.hard, step_name
            ),
            output: None,
        };
    }
    if prompt_bytes > budget.target {
        log::warn!(
            "[executor] Prompt size {} exceeds target budget {} for step '{}'",
            prompt_bytes,
            budget.target,
            step_name
        );
    }

    match tokio::task::spawn_blocking(move || {
        agent::run_agent_with_backend(&agent_provider, &prompt, &repo_root, backend_preference)
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

/// Read all articles for a project from SQLite and return the next
/// unoccupied past date using the canonical `date_policy::suggest_next_safe_date`.
///
/// This is more current than reading articles.json because SQLite is
/// updated by ingest_orphans before articles.json is exported.
pub(crate) fn compute_next_publish_date(
    conn: &rusqlite::Connection,
    project_id: &str,
) -> Option<String> {
    let project_exists: bool = conn
        .query_row(
            "SELECT 1 FROM projects WHERE id = ?1 LIMIT 1",
            [project_id],
            |_| Ok(true),
        )
        .unwrap_or(false);
    if !project_exists {
        return None;
    }

    let articles = crate::engine::task_store::list_articles(conn, project_id).ok()?;
    Some(crate::content::date_policy::suggest_next_safe_date(&articles))
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
mod tests {
    use super::*;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
        TaskStatus,
    };

    fn make_task(task_type: &str) -> Task {
        Task {
            id: format!("test-{task_type}"),
            task_type: task_type.to_string(),
            phase: "research".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
            agent_policy: AgentPolicy::Optional,
            title: Some(format!("{task_type} test")),
            description: None,
            project_id: "proj1".to_string(),
            depends_on: vec![],
            artifacts: vec![],
            run: TaskRun {
                attempts: 0,
                last_error: None,
                ..Default::default()
            },
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            not_before: None,
        }
    }

    #[test]
    fn test_kimi_backend_preference_for_step_content() {
        let step = WorkflowStep::new("test", crate::engine::workflows::StepKind::Agentic);
        for tt in [
            "write_article",
            "optimize_article",
            "create_content",
            "optimize_content",
            "create_hub_page",
            "refresh_hub_page",
        ] {
            let task = make_task(tt);
            assert_eq!(
                kimi_backend_preference_for_step(&task, &step),
                Some("acp"),
                "{} should prefer acp",
                tt
            );
        }
    }

    #[test]
    fn test_kimi_backend_preference_for_step_other() {
        let step = WorkflowStep::new("test", crate::engine::workflows::StepKind::Agentic);
        for tt in [
            "content_audit",
            "collect_gsc",
            "cluster_and_link",
            "reddit_reply",
        ] {
            let task = make_task(tt);
            assert_eq!(
                kimi_backend_preference_for_step(&task, &step),
                Some("direct"),
                "{} should prefer direct",
                tt
            );
        }
    }

    #[test]
    fn test_prompt_budget_defaults() {
        let b = default_budget_for_backend(Some("acp"));
        assert_eq!(b.target, 80 * 1024);
        assert_eq!(b.hard, 90 * 1024);
    }

    #[test]
    fn test_estimate_prompt_bytes() {
        assert_eq!(estimate_prompt_bytes("hello"), 5);
        assert_eq!(estimate_prompt_bytes(""), 0);
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    use crate::config::task_definitions;
    use crate::models::task::{
        AgentPolicy, FollowUpPolicy, Priority, Task, TaskReviewSurface, TaskRun, TaskRunPolicy,
        TaskStatus,
    };

    fn make_task(task_type: &str) -> Task {
        Task {
            id: format!("test-{task_type}"),
            task_type: task_type.to_string(),
            phase: "research".to_string(),
            status: TaskStatus::Todo,
            priority: Priority::Medium,
            run_policy: TaskRunPolicy::UserEnqueue,
            review_surface: TaskReviewSurface::None,
            follow_up_policy: FollowUpPolicy::None,
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
            not_before: None,
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
