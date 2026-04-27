/// Typed contracts for the CTR (Click-Through Rate) audit and fix pipeline.
///
/// These structs replace loose serde_json::Value handoffs between the
/// normalizer, fix task creator, and verification step.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrAgentOutput {
    pub recommendations: Vec<CtrRecommendation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrRecommendation {
    pub article_id: i64,
    pub url_slug: String,
    /// Canonical file path for the article MDX file. Guaranteed by Rust context enrichment.
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected_ctr_improvement: Option<String>,
    /// Target keyword for this article (used by verifier for snippet keyword check).
    /// Guaranteed by Rust context enrichment.
    pub target_keyword: String,
    pub fixes: Vec<CtrFix>,
}

// ─── Verification Report (deterministic check results) ────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrVerificationReport {
    pub summary: String,
    pub verified_count: usize,
    pub failed_count: usize,
    pub skipped_count: usize,
    pub articles: Vec<CtrVerifiedArticle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrVerifiedArticle {
    pub article_id: i64,
    pub file: String,
    pub status: String, // "verified" | "failed" | "skipped"
    pub fixes: Vec<CtrVerifiedFix>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrVerifiedFix {
    pub fix_type: CtrFixType,
    pub status: String, // "verified" | "failed" | "skipped"
    /// Human-readable failure detail: "title is 61 chars (max 55)", "meta is 132 chars (expected 140-155)", etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Actual value that was measured
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    /// Expected rule / threshold
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrFix {
    #[serde(rename = "type")]
    pub fix_type: CtrFixType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    /// Recommended fix value. String for title/meta/snippet; array of questions for FAQ.
    pub recommended: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum CtrFixType {
    TitleRewrite,
    MetaDescription,
    FaqSchema,
    SnippetBait,
}

// ─── Fix Report (agent output after applying fixes) ───────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrFixReport {
    pub applied: Vec<CtrFixApplied>,
    pub skipped: Vec<CtrFixSkipped>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrFixApplied {
    pub article_id: i64,
    pub file: String,
    pub changes: Vec<CtrFixChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrFixChange {
    pub fix_type: CtrFixType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    pub new: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrFixSkipped {
    pub article_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ─── Agent Patch Output (structured replacement values) ───────────────────────

/// The agent returns this instead of raw MDX. Rust applies it deterministically.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrFixPatch {
    pub article_id: i64,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub changes: CtrFixPatchChanges,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrFixPatchChanges {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_paragraph: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub faq_questions: Option<Vec<CtrFixPatchFaqQuestion>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrFixPatchFaqQuestion {
    pub question: String,
    pub answer: String,
}

// ─── Per-Article Fix Verification Report ──────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrFixVerificationReport {
    pub article_id: i64,
    pub file: String,
    pub overall_status: String, // "verified" | "partial" | "failed"
    pub checks: Vec<CtrFixCheckResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrFixCheckResult {
    pub check_type: String, // "title" | "description" | "snippet" | "faq"
    pub status: String,     // "pass" | "fail" | "skip"
    pub expected: String,
    pub actual: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

// ─── Health Summary (project-wide CTR health dashboard) ───────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrHealthSummary {
    pub total_articles: usize,
    pub healthy_count: usize,
    pub unhealthy_count: usize,
    pub improved_count: usize,
    pub already_healthy_count: usize,
    pub regressed_count: usize,
    pub missing_files: usize,
    pub title_issues: usize,
    pub meta_issues: usize,
    pub snippet_issues: usize,
    pub faq_issues: usize,
    pub last_audit_at: Option<String>,
    pub articles: Vec<CtrHealthArticle>,
    pub pending_fix_tasks: usize,
    pub completed_audits: usize,
    pub open_issues_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrHealthArticle {
    pub id: i64,
    pub title: String,
    pub url_slug: String,
    pub file: String,
    pub healthy: bool,
    pub audit_status: String,
    pub issues: Vec<String>,
    pub last_audited_at: Option<String>,
    pub last_audit_issues: Vec<String>,
    pub resolved_issues: Vec<String>,
}

// ─── Rendered SERP Audit (page-level rendered HTML observation) ───────────────

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrRenderedPageAudit {
    pub article_id: i64,
    pub url: String,
    pub file: String,
    pub source_title: String,
    pub rendered_title: String,
    pub rendered_title_length: usize,
    pub title_issue_source: String,
    pub source_description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered_h1: Option<String>,
    pub schema_types: Vec<String>,
    pub has_rendered_faq_page: bool,
    pub snippet_markup: CtrSnippetMarkup,
    pub issues: Vec<String>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[derive(Default)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrSnippetMarkup {
    pub has_question_h2: bool,
    pub has_ordered_list: bool,
    pub has_table: bool,
}

// ─── Site Title Template Detection ────────────────────────────────────────────

/// Result of detecting a repeated site-wide title template pattern.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrTemplateDetectionResult {
    /// The detected template pattern, e.g. "{title} | Days to Expiry | Days to Expiry — Option Selling Analyzer"
    pub detected_pattern: String,
    /// The desired corrected pattern, e.g. "{title} | Days to Expiry"
    pub desired_pattern: String,
    /// Number of pages affected by this pattern
    pub affected_pages: usize,
    /// Candidate framework files that may contain the title template
    pub candidate_files: Vec<String>,
    /// Confidence level: "high", "medium", or "low"
    pub confidence: String,
    /// Whether this fix requires manual review before applying
    pub requires_manual_review: bool,
    /// Sample URLs to verify after fix
    pub verification_urls: Vec<String>,
    /// Per-page details for affected articles
    pub pages: Vec<CtrTemplatePageDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrTemplatePageDetail {
    pub article_id: i64,
    pub url: String,
    pub file: String,
    pub source_title: String,
    pub rendered_title: String,
}
