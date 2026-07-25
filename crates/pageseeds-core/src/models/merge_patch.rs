/// Types for content merge patches produced by the agentic `merge_draft_patch`
/// step and applied deterministically by `merge_apply_patch`.
use serde::{Deserialize, Serialize};

/// A structured patch describing how to merge unique content from redirect
/// targets into a keeper article.
#[derive(Debug, Clone, Serialize, Deserialize, Default, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct ContentMergePatch {
    /// Absolute or repo-relative path to the keeper MDX file.
    pub keeper_file: String,
    /// Sections to insert into the keeper.
    #[serde(default)]
    pub additions: Vec<SectionAddition>,
    /// Any prose transitions or edits to existing paragraphs.
    #[serde(default)]
    pub transitions: Vec<TransitionEdit>,
    /// Notes for the reviewer (not applied automatically).
    #[serde(default)]
    pub notes: Vec<String>,
}

/// A section to add to the keeper.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct SectionAddition {
    /// Heading text (without # prefix).
    pub heading: String,
    /// Body markdown content under the heading.
    pub content: String,
    /// Where to insert: "after:<heading>", "before:<heading>", or "end".
    pub position: String,
    /// Which source redirect page this content came from.
    pub source_file: String,
}

/// A transition edit to an existing paragraph.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub struct TransitionEdit {
    /// Existing text to locate (must be unique enough to find).
    pub find: String,
    /// Replacement text.
    pub replace: String,
}

/// A single redirect rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RedirectRule {
    pub source: String,
    pub destination: String,
    pub status: u16,
}

/// Preflight report for a merge operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MergePreflightReport {
    pub keeper_file_exists: bool,
    pub keeper_is_indexable: bool,
    pub redirect_files_exist: Vec<String>,
    pub redirect_files_missing: Vec<String>,
    pub redirect_cycles_detected: Vec<String>,
    pub can_proceed: bool,
    pub notes: Vec<String>,
}

/// Extracted unique sections from a redirect page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SectionInventory {
    pub file: String,
    pub headings: Vec<ExtractedHeading>,
    pub tables: Vec<ExtractedTable>,
    pub examples: Vec<ExtractedExample>,
    pub faqs: Vec<ExtractedFaq>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExtractedHeading {
    pub level: u8,
    pub text: String,
    pub body: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExtractedTable {
    pub caption: Option<String>,
    pub markdown: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExtractedExample {
    pub caption: Option<String>,
    pub code: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ExtractedFaq {
    pub question: String,
    pub answer: String,
}

/// Validation report after merge apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MergeValidationReport {
    pub keeper_valid: bool,
    pub keeper_word_count: usize,
    pub redirect_map_path: Option<String>,
    pub issues: Vec<String>,
}
