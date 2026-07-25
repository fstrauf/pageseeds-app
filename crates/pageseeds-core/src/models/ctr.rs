/// Typed contracts for the CTR (Click-Through Rate) audit and fix pipeline.
///
/// These structs replace loose serde_json::Value handoffs between the
/// normalizer, fix task creator, and verification step.
use schemars::JsonSchema;
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
    #[serde(default)]
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

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, PartialEq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, JsonSchema)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrFixPatch {
    pub article_id: i64,
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub changes: CtrFixPatchChanges,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, ts_rs::TS, JsonSchema)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet_patch: Option<CtrSnippetPatch>,
}

// NOTE: row-objects are required for provider-safe schemars (no nested
// array-of-arrays for tool params — OpenAI-shaped providers reject them).
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, JsonSchema)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrComparisonTableRow {
    pub cells: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, JsonSchema)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrSnippetPatch {
    pub target_query: String,
    pub format: CtrSnippetFormat,
    pub heading: String,
    pub answer_paragraph: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ordered_list: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comparison_table: Option<Vec<CtrComparisonTableRow>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, JsonSchema)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum CtrSnippetFormat {
    DirectAnswerParagraph,
    ComparisonParagraph,
    BestListOrdered,
    ComparisonTable,
    DefinitionWithSteps,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, JsonSchema)]
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
    pub rendered_faq_question_count: usize,
    pub snippet_markup: CtrSnippetMarkup,
    pub issues: Vec<String>,
    pub checked_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS, Default)]
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

// ─── CTR Outcome Tracking ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub struct CtrOutcome {
    pub project_id: String,
    pub article_id: i64,
    pub fix_task_id: String,
    pub baseline_start: String,
    pub baseline_end: String,
    pub after_start: Option<String>,
    pub after_end: Option<String>,
    pub baseline_clicks: f64,
    pub baseline_impressions: f64,
    pub baseline_ctr: f64,
    pub baseline_position: f64,
    pub after_clicks: Option<f64>,
    pub after_impressions: Option<f64>,
    pub after_ctr: Option<f64>,
    pub after_position: Option<f64>,
    pub position_delta: Option<f64>,
    pub outcome_status: String,
    pub deployed_at: Option<String>,
    pub reviewed_at: Option<String>,
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify the embedded `ctr-optimization` skill file's example JSON deserializes
    /// into the canonical Rust model.  This catches schema drift between the skill
    /// file (agent-facing contract) and the Rust parser (downstream consumer).
    #[test]
    fn ctr_optimization_skill_example_deserializes() {
        let skill_md = include_str!("../../skills/ctr-optimization/SKILL.md");

        // Extract the first ```json block after "## Output Contract"
        let contract_start = skill_md.find("## Output Contract").expect("missing Output Contract section");
        let after_contract = &skill_md[contract_start..];
        let json_start = after_contract.find("```json").expect("missing ```json block");
        let after_block = &after_contract[json_start + 7..];
        let json_end = after_block.find("```").expect("unclosed ```json block");
        let json_str = after_block[..json_end].trim();

        let output: CtrAgentOutput = serde_json::from_str(json_str)
            .unwrap_or_else(|e| panic!("Skill file example JSON does not match CtrAgentOutput: {}", e));

        assert_eq!(output.recommendations.len(), 1, "expected one recommendation in example");
        let rec = &output.recommendations[0];
        assert_eq!(rec.article_id, 42);
        assert_eq!(rec.url_slug, "best-stocks-csp");
        assert_eq!(rec.fixes.len(), 4, "expected four fixes in example");

        // Verify each fix type parses correctly
        let expected_types = vec![
            CtrFixType::TitleRewrite,
            CtrFixType::MetaDescription,
            CtrFixType::FaqSchema,
            CtrFixType::SnippetBait,
        ];
        for (fix, expected) in rec.fixes.iter().zip(expected_types.iter()) {
            assert_eq!(&fix.fix_type, expected, "fix type mismatch");
        }
    }

    /// Freezes the provider-safe tool schema for `CtrFixPatch`:
    /// `comparison_table` must be an array of row objects (`{ cells: string[] }`),
    /// never a nested array-of-arrays (rejected by OpenAI-shaped providers as
    /// `invalid_function_parameters`).
    #[test]
    fn ctr_fix_patch_schema_comparison_table_is_not_nested_array_of_arrays() {
        let schema = schemars::schema_for!(CtrFixPatch);
        let schema_json = serde_json::to_value(&schema).expect("serialize schema");

        // Locate every `comparison_table` property schema and assert its items
        // are objects (or $ref to objects), not arrays.
        let mut found = false;
        walk_comparison_table_schemas(&schema_json, &mut |prop_schema| {
            found = true;
            assert_items_not_array(prop_schema);
        });
        assert!(
            found,
            "expected to find comparison_table in CtrFixPatch schemars output"
        );
    }

    fn walk_comparison_table_schemas(value: &serde_json::Value, visit: &mut dyn FnMut(&serde_json::Value)) {
        match value {
            serde_json::Value::Object(map) => {
                if let Some(props) = map.get("properties").and_then(|p| p.as_object()) {
                    if let Some(ct) = props.get("comparison_table") {
                        visit(ct);
                    }
                }
                for v in map.values() {
                    walk_comparison_table_schemas(v, visit);
                }
            }
            serde_json::Value::Array(arr) => {
                for v in arr {
                    walk_comparison_table_schemas(v, visit);
                }
            }
            _ => {}
        }
    }

    fn assert_items_not_array(prop_schema: &serde_json::Value) {
        // Unwrap oneOf/anyOf/allOf wrappers commonly used for Option<T>.
        let candidates: Vec<&serde_json::Value> = if let Some(arr) = prop_schema
            .get("oneOf")
            .or_else(|| prop_schema.get("anyOf"))
            .or_else(|| prop_schema.get("allOf"))
            .and_then(|v| v.as_array())
        {
            arr.iter().collect()
        } else {
            vec![prop_schema]
        };

        for candidate in candidates {
            // Skip pure null variants from Option.
            if candidate.get("type").and_then(|t| t.as_str()) == Some("null") {
                continue;
            }
            if let Some(items) = candidate.get("items") {
                // Nested array-of-arrays: items.type == "array" (or items is array type)
                let is_nested_array = items
                    .get("type")
                    .map(|t| match t {
                        serde_json::Value::String(s) => s == "array",
                        serde_json::Value::Array(types) => {
                            types.iter().any(|x| x.as_str() == Some("array"))
                        }
                        _ => false,
                    })
                    .unwrap_or(false);
                assert!(
                    !is_nested_array,
                    "comparison_table must not use nested array-of-arrays in tool schema; got items={}. \
                     Use Vec<CtrComparisonTableRow> (objects with cells) instead.",
                    items
                );
            }
        }
    }
}
