//! Shared prompt size budget for LLM calls.
//!
//! Single source of truth for how large an assembled prompt may be. The
//! default Kimi backend is the CLI (`kimi -p`), which has no byte cap; the
//! budget is a safety rail, not a provider limit. The retired Kimi bridge's
//! 20 KB limit no longer applies anywhere in the live pipeline.

/// Prompt size budget in bytes.
pub struct PromptBudget {
    /// Above this, log a warning but proceed.
    pub target: usize,
    /// Above this, fail the step before calling the provider.
    pub hard: usize,
}

/// Default budget for all agentic prompts: 80 KB target / 90 KB hard.
pub fn default_prompt_budget() -> PromptBudget {
    PromptBudget {
        target: 80 * 1024,
        hard: 90 * 1024,
    }
}
