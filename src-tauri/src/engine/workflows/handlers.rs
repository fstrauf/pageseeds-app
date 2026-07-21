/// Trait-based workflow handlers — one per task family.
///
/// Each handler knows:
///   - which task types it owns (`supports`)
///   - what steps the task needs (`plan`)
///
/// Step execution happens in `executor.rs`; handlers only describe the plan.
use super::{step_params, PromptSection, StepKind, WorkflowStep};
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
                // 6-step hybrid workflow:
                // deterministic → deterministic → agentic → agentic → deterministic → deterministic
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
                    // Step 4 (agentic): LLM validates themes for domain relevance and
                    // proposes 1-3 sharpened seed phrasings per on-topic theme.
                    // Uses rig Extractor<T> for guaranteed structured JSON output.
                    // Cannot be deterministic: requires understanding what is on-topic for this
                    // specific product/site. Hard-coding a relevance rule would produce silent errors
                    // on any input it was not tested against.
                    // Input contract: research_seed_extraction artifact {themes: [string]}
                    // Output contract: {validated_seeds: [{theme: string, seeds: [string]}]}
                    WorkflowStep::new("research_seed_validation", StepKind::Agentic),
                    // Step 5 (deterministic): DataForSEO related_keywords per validated seed.
                    // Deterministic: given validated seeds, fetches keyword ideas + KD + volume.
                    // Also consumes pending territory themes from research_shortlist as extra seeds.
                    WorkflowStep::new("research_ahrefs_pipeline", StepKind::KeywordResearchNative)
                        .with_latest_raw_policy(
                            crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput,
                        ),
                    // Step 6 (hybrid): Select best candidates from structured data
                    // (deterministic filter/sort, overshoot to 15), then one batched
                    // agentic relevance check drops off-domain candidates (non-fatal),
                    // then trim to 10 + winnability enrichment.
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
                | "create_landing_page"
                | "review_article_quality"
        )
    }

    fn plan(&self, task: &Task) -> Vec<WorkflowStep> {
        if task_type(task) == "review_article_quality" {
            // Structured quality gate: load the written MDX, then score it.
            return vec![
                WorkflowStep::new("content_quality_context", StepKind::ContentQualityContext),
                WorkflowStep::new("content_quality_review", StepKind::ContentQualityReview),
            ];
        }

        // Agentic: the agent reads the article spec and writes the MDX file.
        let has_hub_brief = task.artifacts.iter().any(|a| a.key == "hub_brief");
        let is_hub =
            has_hub_brief || matches!(task_type(task), "create_hub_page" | "refresh_hub_page");
        let is_new_article = matches!(
            task_type(task),
            "write_article"
                | "create_content"
                | "create_hub_page"
                | "refresh_hub_page"
                | "create_landing_page"
        );
        // Hub pages use the dedicated hub-write skill; landing pages use the
        // conversion-focused landing-page-write skill; all other content tasks
        // use content-write, which carries tone, differentiation, and E-E-A-T
        // directives. Previously regular articles loaded no skill at all and
        // fell through to a generic boilerplate prompt with no content strategy.
        let skill = if is_hub {
            "hub-write"
        } else if task_type(task) == "create_landing_page" {
            "landing-page-write"
        } else {
            "content-write"
        };
        // Declare the prompt sections this step needs (issue #4 stage C).
        // Order matters: content directives come before hub directives in the
        // assembled prompt. Previously `exec_agentic` derived all of this from
        // a boolean lattice over task type and skill name.
        let mut step = WorkflowStep::new("content_write_stage", StepKind::Agentic)
            .with_param(step_params::SKILL, skill)
            .with_prompt_section(PromptSection::ContentDirectives {
                new_article: is_new_article,
            });
        if is_hub {
            step = step.with_prompt_section(PromptSection::HubDirectives);
        }
        let mut steps = vec![step];
        // Step 2 (deterministic, new-article tasks only): verify the write stage
        // actually produced a registered article file. Turns the silent no-op of
        // text-only providers (task Done, zero output, issue #13) into a loud,
        // retryable failure. Optimize tasks modify an existing file, so no new
        // file is expected and this check does not apply to them.
        if is_new_article {
            steps.push(WorkflowStep::new(
                "content_write_verify",
                StepKind::ContentWriteVerify,
            ));
        }
        steps.push(
            // Final step (deterministic): verify every /blog/ link the agent wrote
            // resolves to a project article. Auto-repairs filename-form hrefs
            // (e.g. /blog/248_roast_profile_management) and fails the task with
            // a per-link report when a link target does not exist — broken
            // internal links must not reach the repo unrepaired.
            WorkflowStep::new("content_link_verify", StepKind::LinkIntegrityVerify),
        );
        steps
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
                | "calculator_rollout"
                | "gsc_indexing_recovery"
                | "fix_indexing_internal_links"
                | "gsc_indexing_outcome_review"
                | "content_outcome_review"
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
                // (word count, H1, title, internal links, canonical) plus the parsed task
                // description fields (recommended action, suggested title/H1). Obvious file
                // I/O — no judgment required — and saves the agent from hunting around the repo.
                WorkflowStep::new("indexing_fix_context", StepKind::IndexingFixContext)
                    .with_latest_raw_policy(
                        crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput,
                    ),
                // Step 2 (agentic): generate a structured IndexingFixPlan JSON. Judgment is
                // required because the fix depends on intent, content quality, and
                // site-specific conventions. The agent returns JSON only — it never edits
                // files (direct mode has no file I/O on most providers).
                // Output contract: IndexingFixPlan (see the indexing-fix skill).
                WorkflowStep::new("indexing_fix_generate", StepKind::IndexingFixGenerate)
                    .with_latest_raw_policy(
                        crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput,
                    )
                    .with_param(step_params::ARTIFACT_NAME, "indexing_fix_plan"),
                // Step 3 (deterministic): apply the plan to the MDX file with
                // snapshot/restore. Fails loudly when the plan produces no effective
                // change, so the task can never silently succeed without an edit.
                WorkflowStep::new("indexing_fix_apply", StepKind::IndexingFixApply),
                // Step 4 (deterministic): re-read the file and verify every planned
                // change landed. Fails loudly when the file is unchanged.
                WorkflowStep::new("indexing_fix_verify", StepKind::IndexingFixVerify),
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
            "content_outcome_review" => vec![
                // Step 1 (deterministic): compare pre/post GSC daily snapshot
                // windows for the article slug, classify
                // improved/regressed/neutral/insufficient_data, and persist the
                // result. Cannot need an LLM: it is a computable mapping from
                // structured snapshot rows to a classification (issue #23).
                // Output contract: JSON report with slug, baseline/recent
                // window metrics, and classification.
                WorkflowStep::new("content_outcome_compare", StepKind::ContentOutcomeCompare),
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
                // Step 1 (deterministic): Refresh GSC data so after-metrics are not
                // compared against stale syncs. Optional — the review still runs on
                // whatever data exists if the sync fails (e.g. missing credentials).
                WorkflowStep::new("ctr_gsc_refresh", StepKind::GscSyncArticles).optional(),
                // Step 2 (deterministic): verify deployment (live title shows the fix),
                // load baseline outcomes, fetch after-period GSC metrics, and compare
                // per-day clicks / CTR over explicit baseline/after windows.
                // Output contract: JSON report with improved/regressed/neutral/
                // insufficient_data/deployment_unverified counts.
                WorkflowStep::new("ctr_outcome_compare", StepKind::CtrOutcomeCompare),
                // Step 3 (deterministic): generate structured report artifact.
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
            // Step 7 (deterministic): Rewrite inbound links to redirected slugs
            // across all MDX files so nothing links to a redirected URL.
            WorkflowStep::new(
                "merge_rewrite_inbound_links",
                StepKind::MergeRewriteInboundLinks,
            ),
            // Step 8 (deterministic): Validate merged keeper and redirect map.
            WorkflowStep::new("merge_validate_output", StepKind::MergeValidateOutput),
            // Step 9 (deterministic): Sync merged articles back to SQLite and articles.json.
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

// ─── SEO Discovery ────────────────────────────────────────────────────────────

pub struct SeoDiscoveryHandler;

impl WorkflowHandler for SeoDiscoveryHandler {
    fn supports(&self, task: &Task) -> bool {
        task_type(task) == "seo_health_scan"
    }

    fn plan(&self, _task: &Task) -> Vec<WorkflowStep> {
        vec![
            // Step 1 (deterministic, optional): refresh GSC page + query metrics.
            WorkflowStep::new("seo_gsc_sync", StepKind::GscSyncArticles).optional(),
            // Step 2 (deterministic, optional): run the 21-check content quality audit.
            WorkflowStep::new("seo_content_audit", StepKind::ContentAudit).optional(),
            // Step 3 (deterministic): build CTR context (clicks_lost, query intent).
            WorkflowStep::new("seo_ctr_context", StepKind::CtrBuildContext)
                .with_latest_raw_policy(
                    crate::engine::workflows::LatestRawPolicy::ReplaceWithOutput,
                ),
            // Step 4 (deterministic, optional): build cannibalization clusters + hub gaps.
            WorkflowStep::new("seo_can_context", StepKind::CanBuildContext)
                .with_latest_raw_policy(crate::engine::workflows::LatestRawPolicy::Clear)
                .optional(),
            // Step 5 (deterministic, optional): build not-indexed target contexts.
            WorkflowStep::new("seo_ihc_context", StepKind::IhcBuildTargetContext).optional(),
            // Step 6 (deterministic, optional): summarize Clarity UX anomalies.
            WorkflowStep::new("seo_clarity_summarise", StepKind::ClaritySummarise).optional(),
            // Step 7 (deterministic): fuse all signals and rank opportunities.
            WorkflowStep::new("seo_rank_opportunities", StepKind::RankOpportunities)
                .with_param(step_params::ARTIFACT_NAME, "seo_opportunities"),
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
        Box::new(SeoDiscoveryHandler),
        Box::new(ImplementationHandler),
        Box::new(ManualFallbackHandler),
    ]
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

    /// The content write stage must declare exactly the prompt sections
    /// `exec_agentic` previously derived from its boolean lattice, in assembly
    /// order (content directives before hub directives).
    #[test]
    fn content_write_stage_declares_prompt_sections() {
        let handler = ContentHandler;

        // New article: content directives, new-article variant.
        let steps = handler.plan(&make_task("write_article"));
        assert_eq!(
            steps[0].prompt_sections,
            vec![PromptSection::ContentDirectives { new_article: true }]
        );

        // Optimize: content directives, preserve variant.
        let steps = handler.plan(&make_task("optimize_article"));
        assert_eq!(
            steps[0].prompt_sections,
            vec![PromptSection::ContentDirectives { new_article: false }]
        );

        // Hub task type: content directives first, hub directives second.
        let steps = handler.plan(&make_task("create_hub_page"));
        assert_eq!(
            steps[0].prompt_sections,
            vec![
                PromptSection::ContentDirectives { new_article: true },
                PromptSection::HubDirectives,
            ]
        );

        // write_article carrying a hub_brief artifact also gets hub directives.
        let mut task = make_task("write_article");
        task.artifacts.push(crate::models::task::TaskArtifact {
            key: "hub_brief".to_string(),
            path: None,
            artifact_type: None,
            source: None,
            content: Some("{}".to_string()),
        });
        let steps = handler.plan(&task);
        assert_eq!(
            steps[0].prompt_sections,
            vec![
                PromptSection::ContentDirectives { new_article: true },
                PromptSection::HubDirectives,
            ]
        );

        // Non-content agentic steps declare no sections.
        let steps = InvestigationHandler.plan(&make_task("investigate_posthog"));
        assert!(steps[0].prompt_sections.is_empty());
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
