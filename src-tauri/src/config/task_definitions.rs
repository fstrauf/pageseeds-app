use crate::models::task::ExecutionMode;

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
    Coverage,
    CtrAudit,
    CannibalizationAudit,
    Implementation,
    Manual,
}

/// Static metadata for every supported task type.
///
/// This is the single source of truth for:
/// - default phase
/// - default execution mode
/// - whether the task lands in Review status on success
/// - which handler family plans its workflow steps
#[derive(Debug, Clone, Copy)]
pub struct TaskDefinition {
    pub task_type: &'static str,
    pub phase: &'static str,
    pub execution_mode: ExecutionMode,
    pub review_on_success: bool,
    pub handler_family: HandlerFamily,
}

const DEFINITIONS: &[TaskDefinition] = &[
    // Content
    TaskDefinition {
        task_type: "write_article",
        phase: "implementation",
        execution_mode: ExecutionMode::Spec,
        review_on_success: false,
        handler_family: HandlerFamily::Content,
    },
    TaskDefinition {
        task_type: "optimize_article",
        phase: "implementation",
        execution_mode: ExecutionMode::Spec,
        review_on_success: false,
        handler_family: HandlerFamily::Content,
    },
    TaskDefinition {
        task_type: "create_landing_page",
        phase: "implementation",
        execution_mode: ExecutionMode::Spec,
        review_on_success: false,
        handler_family: HandlerFamily::Content,
    },
    TaskDefinition {
        task_type: "create_content",
        phase: "implementation",
        execution_mode: ExecutionMode::Spec,
        review_on_success: false,
        handler_family: HandlerFamily::Content,
    },
    TaskDefinition {
        task_type: "optimize_content",
        phase: "implementation",
        execution_mode: ExecutionMode::Spec,
        review_on_success: false,
        handler_family: HandlerFamily::Content,
    },
    TaskDefinition {
        task_type: "content_review_apply",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Content,
    },
    // Content Review
    TaskDefinition {
        task_type: "content_review",
        phase: "investigation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::ContentReview,
    },
    TaskDefinition {
        task_type: "content_audit",
        phase: "investigation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::ContentReview,
    },
    // Research
    TaskDefinition {
        task_type: "research_keywords",
        phase: "research",
        execution_mode: ExecutionMode::Manual,
        review_on_success: true,
        handler_family: HandlerFamily::Research,
    },
    TaskDefinition {
        task_type: "custom_keyword_research",
        phase: "research",
        execution_mode: ExecutionMode::Manual,
        review_on_success: true,
        handler_family: HandlerFamily::Research,
    },
    TaskDefinition {
        task_type: "research_landing_pages",
        phase: "research",
        execution_mode: ExecutionMode::Manual,
        review_on_success: true,
        handler_family: HandlerFamily::Research,
    },
    // Collection
    TaskDefinition {
        task_type: "collect_gsc",
        phase: "collection",
        execution_mode: ExecutionMode::Automatic,
        review_on_success: false,
        handler_family: HandlerFamily::Collection,
    },
    TaskDefinition {
        task_type: "collect_posthog",
        phase: "collection",
        execution_mode: ExecutionMode::Automatic,
        review_on_success: false,
        handler_family: HandlerFamily::Collection,
    },
    // Investigation
    TaskDefinition {
        task_type: "investigate_gsc",
        phase: "investigation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Investigation,
    },
    TaskDefinition {
        task_type: "investigate_posthog",
        phase: "investigation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Investigation,
    },
    // Reddit
    TaskDefinition {
        task_type: "reddit_opportunity_search",
        phase: "implementation",
        execution_mode: ExecutionMode::Batchable,
        review_on_success: true,
        handler_family: HandlerFamily::Reddit,
    },
    TaskDefinition {
        task_type: "reddit_reply",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Reddit,
    },
    TaskDefinition {
        task_type: "reddit_post_reply",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Reddit,
    },
    // Implementation / fixes
    TaskDefinition {
        task_type: "fix_404s",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_redirects",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_indexing",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_technical",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_content",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_gsc_access",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_ctr_article",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "fix_content_article",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "technical_seo",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "content_cleanup",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Implementation,
    },
    TaskDefinition {
        task_type: "sanitize_content",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Implementation,
    },
    // Performance
    TaskDefinition {
        task_type: "analyze_gsc_performance",
        phase: "investigation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Performance,
    },
    // Coverage
    TaskDefinition {
        task_type: "analyze_keyword_coverage",
        phase: "investigation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Coverage,
    },
    // CTR Audit
    TaskDefinition {
        task_type: "ctr_audit",
        phase: "investigation",
        execution_mode: ExecutionMode::Automatic,
        review_on_success: false,
        handler_family: HandlerFamily::CtrAudit,
    },
    // Cannibalization Audit
    TaskDefinition {
        task_type: "cannibalization_audit",
        phase: "investigation",
        execution_mode: ExecutionMode::Automatic,
        review_on_success: false,
        handler_family: HandlerFamily::CannibalizationAudit,
    },
    // Indexing diagnostics
    TaskDefinition {
        task_type: "indexing_diagnostics",
        phase: "investigation",
        execution_mode: ExecutionMode::Automatic,
        review_on_success: false,
        handler_family: HandlerFamily::Implementation,
    },
    // Social
    TaskDefinition {
        task_type: "social_generate_campaign",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Social,
    },
    TaskDefinition {
        task_type: "social_regenerate_campaign",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Social,
    },
    TaskDefinition {
        task_type: "social_design_template",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Social,
    },
    TaskDefinition {
        task_type: "social_save_template",
        phase: "implementation",
        execution_mode: ExecutionMode::Manual,
        review_on_success: false,
        handler_family: HandlerFamily::Social,
    },
];

pub fn all() -> &'static [TaskDefinition] {
    DEFINITIONS
}

pub fn find(task_type: &str) -> Option<&'static TaskDefinition> {
    DEFINITIONS.iter().find(|d| d.task_type == task_type)
}

pub fn default_phase(task_type: &str) -> &'static str {
    find(task_type).map(|d| d.phase).unwrap_or("implementation")
}

pub fn default_execution_mode(task_type: &str) -> ExecutionMode {
    find(task_type).map(|d| d.execution_mode).unwrap_or(ExecutionMode::Manual)
}

pub fn review_on_success(task_type: &str) -> bool {
    find(task_type).map(|d| d.review_on_success).unwrap_or(false)
}

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
    fn review_tasks_are_as_expected() {
        assert!(review_on_success("research_keywords"));
        assert!(review_on_success("custom_keyword_research"));
        assert!(review_on_success("research_landing_pages"));
        assert!(review_on_success("reddit_opportunity_search"));
        assert!(!review_on_success("write_article"));
        assert!(!review_on_success("collect_gsc"));
        assert!(!review_on_success("ctr_audit"));
    }

    #[test]
    fn default_phase_matches_task_type() {
        assert_eq!(default_phase("research_keywords"), "research");
        assert_eq!(default_phase("collect_gsc"), "collection");
        assert_eq!(default_phase("write_article"), "implementation");
        assert_eq!(default_phase("unknown_task"), "implementation");
    }

    #[test]
    fn default_execution_mode_matches_task_type() {
        assert_eq!(default_execution_mode("collect_gsc"), ExecutionMode::Automatic);
        assert_eq!(default_execution_mode("reddit_search"), ExecutionMode::Manual); // reddit_search not in registry, falls back
        assert_eq!(default_execution_mode("reddit_opportunity_search"), ExecutionMode::Batchable);
        assert_eq!(default_execution_mode("research_keywords"), ExecutionMode::Manual);
        assert_eq!(default_execution_mode("write_article"), ExecutionMode::Spec);
    }

    #[test]
    fn manual_tasks_have_manual_handler_or_implementation() {
        for def in all() {
            if def.handler_family == HandlerFamily::Manual {
                assert_eq!(def.execution_mode, ExecutionMode::Manual, "Manual handler family task {} should have Manual execution mode", def.task_type);
            }
        }
    }
}
