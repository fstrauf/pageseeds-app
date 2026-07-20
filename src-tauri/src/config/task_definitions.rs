use crate::models::task::{FollowUpPolicy, TaskReviewSurface, TaskRunPolicy};

/// Family of workflow handler that owns a task type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerFamily {
    Collection,
    Investigation,
    Research,
    Content,
    ContentReview,
    Reddit,
    Social,
    Performance,
    CtrAudit,
    CannibalizationAudit,
    ConsolidateCluster,
    Implementation,
    TerritoryResearch,
    #[allow(dead_code)]
    Manual,
}

/// Static metadata for every supported task type.
///
/// This is the single source of truth for:
/// - default phase
/// - run policy (can the system auto-enqueue this?)
/// - review surface (what UI appears after completion?)
/// - follow-up policy (how are child tasks created?)
/// - which handler family plans its workflow steps
#[derive(Debug, Clone, Copy)]
pub struct TaskDefinition {
    pub task_type: &'static str,
    pub phase: &'static str,
    pub run_policy: TaskRunPolicy,
    pub review_surface: TaskReviewSurface,
    pub follow_up_policy: FollowUpPolicy,
    #[allow(dead_code)]
    pub handler_family: HandlerFamily,
}

const DEFINITIONS: &[TaskDefinition] = &[
    // Content
    TaskDefinition {
        task_type: "write_article",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Content,
    },
    TaskDefinition {
        task_type: "optimize_article",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Content,
    },
    // Landing page writing: full ContentHandler path (landing-page-write skill,
    // write-verify, link-verify) with the standard post-write follow-ups.
    TaskDefinition {
        task_type: "create_landing_page",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Content,
    },
    TaskDefinition {
        task_type: "create_content",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Content,
    },
    TaskDefinition {
        task_type: "optimize_content",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Content,
    },
    // Quality gate: structured review of a freshly written article before clustering/linking.
    TaskDefinition {
        task_type: "review_article_quality",
        phase: "implementation",
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Content,
    },
    // Content Review
    TaskDefinition {
        task_type: "content_review",
        phase: "investigation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::FollowUpTasks,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::ContentReview,
    },
    TaskDefinition {
        task_type: "content_audit",
        phase: "investigation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::FollowUpTasks,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::ContentReview,
    },
    // Research
    TaskDefinition {
        task_type: "research_keywords",
        phase: "research",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::KeywordPicker,
        follow_up_policy: FollowUpPolicy::UserSelection,
        handler_family: HandlerFamily::Research,
    },
    TaskDefinition {
        task_type: "custom_keyword_research",
        phase: "research",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::KeywordPicker,
        follow_up_policy: FollowUpPolicy::UserSelection,
        handler_family: HandlerFamily::Research,
    },
    TaskDefinition {
        task_type: "research_landing_pages",
        phase: "research",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::KeywordPicker,
        follow_up_policy: FollowUpPolicy::UserSelection,
        handler_family: HandlerFamily::Research,
    },
    // Collection
    TaskDefinition {
        task_type: "collect_gsc",
        phase: "collection",
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Collection,
    },
    TaskDefinition {
        task_type: "collect_posthog",
        phase: "collection",
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Collection,
    },
    TaskDefinition {
        task_type: "collect_clarity",
        phase: "collection",
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Collection,
    },
    // Investigation
    TaskDefinition {
        task_type: "investigate_gsc",
        phase: "investigation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Investigation,
    },
    TaskDefinition {
        task_type: "investigate_posthog",
        phase: "investigation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Investigation,
    },
    TaskDefinition {
        task_type: "investigate_clarity",
        phase: "investigation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Investigation,
    },
    TaskDefinition {
        task_type: "clarity_analytics",
        phase: "investigation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Investigation,
    },
    // Reddit
    TaskDefinition {
        task_type: "reddit_opportunity_search",
        phase: "implementation",
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::RedditPicker,
        follow_up_policy: FollowUpPolicy::UserSelection,
        handler_family: HandlerFamily::Reddit,
    },
    TaskDefinition {
        task_type: "reddit_reply",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Reddit,
    },
    TaskDefinition {
        task_type: "reddit_post_reply",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Reddit,
    },
    // Implementation / fixes
    TaskDefinition {
        task_type: "fix_404s",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_redirects",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_indexing",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_technical",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_content",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_gsc_access",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    // Additional Implementation task types registered in handlers but previously
    // missing from definitions (falling through to silent defaults).
    TaskDefinition {
        task_type: "technical_fix",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "content_strategy",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "publish_content",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    // GSC Indexing Recovery Campaign
    // Parent campaign task: refreshes GSC/link data, computes drift, plans targets,
    // and spawns focused fix_indexing_internal_links children via post-actions.
    TaskDefinition {
        task_type: "gsc_indexing_recovery",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Implementation,
    },
    // Per-target internal link fix task. Auto-enqueued by the backend queue.
    // Each task carries a structured target artifact and verifies the target gained inbound links.
    TaskDefinition {
        task_type: "fix_indexing_internal_links",
        phase: "implementation",
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Implementation,
    },
    // Delayed GSC outcome review: re-inspects target URL after wait period to see if linking helped indexing.
    TaskDefinition {
        task_type: "gsc_indexing_outcome_review",
        phase: "verification",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    // Interlinking: spawned for not_indexed_other URLs (unknown to Google).
    // Runs the same steps as cluster_and_link — scan link graph, strategize,
    // apply Related Articles sections — to add inbound internal links from
    // indexed pages so Google can discover the target URL.
    TaskDefinition {
        task_type: "interlinking",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    // Cluster and link: scanned link graph + strategize + apply Related Articles.
    // Auto-enqueued by post-actions after article creation. Uses 3-step plan.
    TaskDefinition {
        task_type: "cluster_and_link",
        phase: "implementation",
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_ctr_article",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_ctr_site_template",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "ctr_outcome_review",
        phase: "investigation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::CtrAudit,
    },
    TaskDefinition {
        task_type: "fix_content_article",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "technical_seo",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "content_cleanup",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "sanitize_content",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    // Performance
    TaskDefinition {
        task_type: "analyze_gsc_performance",
        phase: "investigation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Performance,
    },
    // CTR Audit
    TaskDefinition {
        task_type: "ctr_audit",
        phase: "investigation",
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::CtrAudit,
    },
    // Cannibalization Audit
    TaskDefinition {
        task_type: "cannibalization_audit",
        phase: "investigation",
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::CannibalizationPicker,
        follow_up_policy: FollowUpPolicy::UserSelection,
        handler_family: HandlerFamily::CannibalizationAudit,
    },
    // Consolidation
    TaskDefinition {
        task_type: "consolidate_cluster",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::ConsolidateCluster,
    },
    // LEGACY: Hub page creation.
    // Preferred path is write_article with hub-write skill + structured hub_brief artifact.
    // Kept for backward compatibility until hub_spoke_context moves to a deterministic pre-step.
    TaskDefinition {
        task_type: "create_hub_page",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Content,
    },
    // LEGACY: Hub page refresh.
    TaskDefinition {
        task_type: "refresh_hub_page",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Content,
    },
    // update_research_shortlist: deterministic territory + coverage gap analysis
    // that feeds the persistent research_shortlist table for keyword research.
    TaskDefinition {
        task_type: "update_research_shortlist",
        phase: "investigation",
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Research,
    },
    // Territory research: 4-step pipeline (load recommendation → build context →
    // strategy → apply). Fully implemented with dedicated TerritoryResearchHandler.
    TaskDefinition {
        task_type: "territory_research",
        phase: "research",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::UserSelection,
        handler_family: HandlerFamily::TerritoryResearch,
    },
    // Calculator rollout (stub — full handler in Phase 6)
    TaskDefinition {
        task_type: "calculator_rollout",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Implementation,
    },
    // Indexing diagnostics
    TaskDefinition {
        task_type: "indexing_diagnostics",
        phase: "investigation",
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Implementation,
    },
    // Unified indexing health campaign
    // Orchestrates prerequisite checks, drift, cluster context, distinctiveness review,
    // and spawns the appropriate child fix tasks.
    TaskDefinition {
        task_type: "indexing_health_campaign",
        phase: "investigation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::BackendAuto,
        handler_family: HandlerFamily::Implementation,
    },
    // Unified SEO health scan
    // Fuses content audit, CTR, indexing, cannibalization, and Clarity UX signals
    // into a single ranked opportunity backlog. Phase 1 surfaces the JSON artifact
    // via ArtifactReview; Phase 2 will add a dedicated OpportunityReview UI.
    TaskDefinition {
        task_type: "seo_health_scan",
        phase: "investigation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::ArtifactReview,
        follow_up_policy: FollowUpPolicy::UserSelection,
        handler_family: HandlerFamily::Implementation,
    },
    // Feature Spec Generation
    TaskDefinition {
        task_type: "generate_feature_spec",
        phase: "investigation",
        run_policy: TaskRunPolicy::AutoEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Implementation,
    },
    // Social
    TaskDefinition {
        task_type: "social_generate_campaign",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Social,
    },
    TaskDefinition {
        task_type: "social_regenerate_campaign",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Social,
    },
    TaskDefinition {
        task_type: "social_design_template",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Social,
    },
    TaskDefinition {
        task_type: "social_save_template",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Social,
    },
    // Additional social task types registered in SocialHandler but previously
    // missing from definitions.
    TaskDefinition {
        task_type: "social_generate_from_article",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Social,
    },
    TaskDefinition {
        task_type: "social_regenerate_post",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Social,
    },
    TaskDefinition {
        task_type: "social_create_template",
        phase: "implementation",
        run_policy: TaskRunPolicy::UserEnqueue,
        review_surface: TaskReviewSurface::None,
        follow_up_policy: FollowUpPolicy::None,
        handler_family: HandlerFamily::Social,
    },
];

#[allow(dead_code)]
pub fn all() -> &'static [TaskDefinition] {
    DEFINITIONS
}

pub fn find(task_type: &str) -> Option<&'static TaskDefinition> {
    DEFINITIONS.iter().find(|d| d.task_type == task_type)
}

pub fn default_phase(task_type: &str) -> &'static str {
    find(task_type).map(|d| d.phase).unwrap_or("implementation")
}

pub fn default_run_policy(task_type: &str) -> TaskRunPolicy {
    find(task_type)
        .map(|d| d.run_policy)
        .unwrap_or(TaskRunPolicy::UserEnqueue)
}

pub fn default_review_surface(task_type: &str) -> TaskReviewSurface {
    find(task_type)
        .map(|d| d.review_surface)
        .unwrap_or(TaskReviewSurface::None)
}

pub fn default_follow_up_policy(task_type: &str) -> FollowUpPolicy {
    find(task_type)
        .map(|d| d.follow_up_policy)
        .unwrap_or(FollowUpPolicy::None)
}

#[allow(dead_code)]
pub fn handler_family(task_type: &str) -> Option<HandlerFamily> {
    find(task_type).map(|d| d.handler_family)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_definition_has_a_unique_task_type() {
        let mut seen = std::collections::HashSet::new();
        for def in all() {
            assert!(
                seen.insert(def.task_type),
                "Duplicate task type: {}",
                def.task_type
            );
        }
    }

    #[test]
    fn research_tasks_have_keyword_picker_surface() {
        assert_eq!(
            default_review_surface("research_keywords"),
            TaskReviewSurface::KeywordPicker
        );
        assert_eq!(
            default_review_surface("custom_keyword_research"),
            TaskReviewSurface::KeywordPicker
        );
        assert_eq!(
            default_review_surface("research_landing_pages"),
            TaskReviewSurface::KeywordPicker
        );
    }

    #[test]
    fn reddit_search_has_reddit_picker_surface() {
        assert_eq!(
            default_review_surface("reddit_opportunity_search"),
            TaskReviewSurface::RedditPicker
        );
    }

    #[test]
    fn cannibalization_audit_has_cannibalization_picker_surface() {
        assert_eq!(
            default_review_surface("cannibalization_audit"),
            TaskReviewSurface::CannibalizationPicker
        );
        assert_eq!(
            default_follow_up_policy("cannibalization_audit"),
            FollowUpPolicy::UserSelection
        );
    }

    #[test]
    fn content_review_has_follow_up_tasks_surface() {
        assert_eq!(
            default_review_surface("content_review"),
            TaskReviewSurface::FollowUpTasks
        );
        assert_eq!(
            default_review_surface("content_audit"),
            TaskReviewSurface::FollowUpTasks
        );
    }

    #[test]
    fn auto_enqueue_tasks_are_collection_and_audit() {
        assert_eq!(
            default_run_policy("collect_gsc"),
            TaskRunPolicy::AutoEnqueue
        );
        assert_eq!(
            default_run_policy("collect_posthog"),
            TaskRunPolicy::AutoEnqueue
        );
        assert_eq!(default_run_policy("ctr_audit"), TaskRunPolicy::AutoEnqueue);
        assert_eq!(
            default_run_policy("cannibalization_audit"),
            TaskRunPolicy::AutoEnqueue
        );
        assert_eq!(
            default_run_policy("indexing_diagnostics"),
            TaskRunPolicy::AutoEnqueue
        );
        assert_eq!(
            default_run_policy("reddit_opportunity_search"),
            TaskRunPolicy::AutoEnqueue
        );
    }

    #[test]
    fn write_tasks_have_backend_auto_follow_up() {
        assert_eq!(
            default_follow_up_policy("write_article"),
            FollowUpPolicy::BackendAuto
        );
        assert_eq!(
            default_follow_up_policy("create_hub_page"),
            FollowUpPolicy::BackendAuto
        );
    }

    #[test]
    fn default_phase_matches_task_type() {
        assert_eq!(default_phase("research_keywords"), "research");
        assert_eq!(default_phase("collect_gsc"), "collection");
        assert_eq!(default_phase("write_article"), "implementation");
        assert_eq!(default_phase("unknown_task"), "implementation");
    }

    #[test]
    fn default_run_policy_matches_task_type() {
        assert_eq!(
            default_run_policy("collect_gsc"),
            TaskRunPolicy::AutoEnqueue
        );
        assert_eq!(
            default_run_policy("reddit_search"),
            TaskRunPolicy::UserEnqueue
        ); // reddit_search not in registry, falls back
        assert_eq!(
            default_run_policy("reddit_opportunity_search"),
            TaskRunPolicy::AutoEnqueue
        );
        assert_eq!(
            default_run_policy("research_keywords"),
            TaskRunPolicy::UserEnqueue
        );
        assert_eq!(
            default_run_policy("write_article"),
            TaskRunPolicy::UserEnqueue
        );
    }
}
