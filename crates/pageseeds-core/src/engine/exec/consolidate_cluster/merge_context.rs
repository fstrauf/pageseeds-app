//! Typed structures for the merge context produced by `merge_extract_sections`
//! and consumed by `merge_draft_patch`.
//!
//! The JSON field names are part of the `merge-content` skill's prompt contract
//! — rename fields only together with the skill documentation.
use serde::{Deserialize, Serialize};

/// One heading entry of the keeper outline (`{ level, text }`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct OutlineHeading {
    pub level: u8,
    pub text: String,
}

/// A heading section extracted from a redirect page.
/// Sections the keeper already covers carry no body.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MergeSection {
    pub level: u8,
    pub text: String,
    pub body: String,
    pub covered_by_keeper: bool,
}

/// A markdown table extracted from a redirect page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MergeTable {
    pub markdown: String,
}

/// A code example extracted from a redirect page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MergeExample {
    pub language: Option<String>,
    pub code: String,
}

/// An FAQ entry extracted from a redirect page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MergeFaq {
    pub question: String,
    pub answer: String,
}

/// One redirect page's extracted content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct RedirectPage {
    pub file: String,
    pub url: String,
    pub title: String,
    pub word_count: usize,
    pub sections: Vec<MergeSection>,
    pub tables: Vec<MergeTable>,
    pub examples: Vec<MergeExample>,
    pub faqs: Vec<MergeFaq>,
    /// Set when the page alone exceeded the merge batch budget and was
    /// deterministically truncated. Records what was cut and why, so the
    /// reduction is visible in the merge context the agent receives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncation_note: Option<String>,
}

/// One batch of redirect pages, drafted and applied as a single round.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MergeBatch {
    pub batch_index: usize,
    pub redirect_pages: Vec<RedirectPage>,
}

/// The full merge context serialized by `merge_extract_sections`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MergeContext {
    pub keeper_file: String,
    pub keeper_outline: Vec<OutlineHeading>,
    pub keeper_excerpt: String,
    pub total_redirects: usize,
    pub batch_count: usize,
    pub batches: Vec<MergeBatch>,
}

/// The per-round context handed to the agent by `merge_draft_patch`: the
/// keeper context plus one batch's redirect pages. Borrows from the parsed
/// `MergeContext` — serialization only.
#[derive(Debug, Serialize)]
pub(crate) struct MergeRoundContext<'a> {
    pub keeper_file: &'a str,
    pub keeper_outline: &'a [OutlineHeading],
    pub keeper_excerpt: &'a str,
    pub total_redirects: usize,
    pub batch_count: usize,
    pub batch_index: usize,
    pub redirect_pages: &'a [RedirectPage],
}
