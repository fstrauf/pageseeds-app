//! Live eval: content_review investigation path (routing judgment from tool
//! evidence → [`InvestigationFindings`]).
//!
//! Does **not** run the multi-turn RO tool agent (not hermetic without a real
//! project DB). Fixtures supply a structured tool-evidence bundle; the suite
//! reuses production extract framing + typed extraction, then:
//! - deterministic: valid task types, evidence citations, proposal caps, enums
//! - judge (optional): findings grounded; healthy sites not over-prescribed

use serde::Deserialize;
use std::collections::HashSet;

use super::{finish_suite, generation_backend, judge_score, list_cases, load_fixture, CaseReport};
use crate::config::task_definitions;
use crate::engine::exec::content::{
    build_content_review_investigation_extract_prompt,
    content_review_investigation_extract_preamble, format_tool_evidence_as_analysis,
};
use crate::models::content_review::InvestigationFindings;

/// Read-only investigation tool names (must match TOOL_INVENTORY RO set).
const RO_INVESTIGATION_TOOLS: &[&str] = &[
    "gsc_performance",
    "gsc_queries",
    "gsc_movers",
    "article_list",
    "article_frontmatter",
    "article_body_hash",
    "article_title_scan",
    "content_audit_report",
    "cannibalization_clusters",
    "indexing_status",
    "ctr_health",
    "framework_files",
    "article_link_graph",
    "get_task_status",
];

const VALID_SEVERITIES: &[&str] = &["critical", "warning", "info"];
const VALID_FIX_TYPES: &[&str] = &[
    "auto_fixable",
    "developer_actionable",
    "hybrid",
    "informational",
];

const JUDGE_CRITERIA: &[&str] = &[
    "Findings are grounded in the provided tool evidence (no invented metrics or tools).",
    "Proposed tasks are justified by concrete findings; healthy sites are not over-prescribed.",
    "Evidence citations name the investigation tools that actually appear in the analysis.",
];

#[derive(Debug, Deserialize)]
struct InvestigateCase {
    name: String,
    tool_evidence: serde_json::Value,
    #[serde(default)]
    expect: Expect,
}

#[derive(Debug, Deserialize, Default, Clone)]
struct Expect {
    #[serde(default)]
    zero_proposals: bool,
    #[serde(default = "default_max_proposals")]
    max_proposals: usize,
    #[serde(default)]
    required_task_types_any: Vec<String>,
    #[serde(default)]
    required_evidence_tools_any: Vec<String>,
}

fn default_max_proposals() -> usize {
    5
}

/// Stable identity for duplicate-proposal detection: task_type + title + params.
fn proposal_identity(task_type: &str, title: &str, params: &serde_json::Value) -> String {
    let params_key = serde_json::to_string(params).unwrap_or_else(|_| "{}".to_string());
    format!("{task_type}\0{title}\0{params_key}")
}

fn evidence_mentions_known_tool(evidence: &str) -> bool {
    let lower = evidence.to_lowercase();
    RO_INVESTIGATION_TOOLS
        .iter()
        .any(|t| lower.contains(&t.to_lowercase()))
}

fn check_findings(findings: &InvestigationFindings, expect: &Expect) -> Vec<String> {
    let mut violations = Vec::new();

    if findings.summary.trim().is_empty() {
        violations.push("summary is empty".to_string());
    }

    if expect.zero_proposals && !findings.proposed_tasks.is_empty() {
        violations.push(format!(
            "expected zero proposed_tasks (healthy site), got {}",
            findings.proposed_tasks.len()
        ));
    }

    if findings.proposed_tasks.len() > expect.max_proposals {
        violations.push(format!(
            "proposed_tasks count {} exceeds max_proposals {}",
            findings.proposed_tasks.len(),
            expect.max_proposals
        ));
    }

    let mut seen_identities = HashSet::new();
    for (i, p) in findings.proposed_tasks.iter().enumerate() {
        let label = format!("proposed_task #{} ({})", i + 1, p.task_type);
        if task_definitions::find(&p.task_type).is_none() {
            violations.push(format!(
                "{label} has unknown task_type (not in task_definitions)"
            ));
        }
        let id = proposal_identity(&p.task_type, &p.title, &p.params);
        if !seen_identities.insert(id) {
            violations.push(format!(
                "{label} is a duplicate proposal (same task_type + title + params)"
            ));
        }
    }

    for (i, f) in findings.findings.iter().enumerate() {
        let label = format!("finding #{} ({})", i + 1, f.title);
        if f.evidence.trim().is_empty() || !evidence_mentions_known_tool(&f.evidence) {
            violations.push(format!(
                "{label} evidence must mention at least one known investigation tool name"
            ));
        }
        if !f.severity.is_empty() && !VALID_SEVERITIES.contains(&f.severity.as_str()) {
            violations.push(format!(
                "{label} has invalid severity '{}' (expected one of {})",
                f.severity,
                VALID_SEVERITIES.join("|")
            ));
        }
        if !f.fix_type.is_empty() && !VALID_FIX_TYPES.contains(&f.fix_type.as_str()) {
            violations.push(format!(
                "{label} has invalid fix_type '{}' (expected one of {})",
                f.fix_type,
                VALID_FIX_TYPES.join("|")
            ));
        }
    }

    if !expect.required_task_types_any.is_empty() {
        let have: HashSet<&str> = findings
            .proposed_tasks
            .iter()
            .map(|p| p.task_type.as_str())
            .collect();
        let any = expect
            .required_task_types_any
            .iter()
            .any(|t| have.contains(t.as_str()));
        if !any {
            violations.push(format!(
                "expected at least one proposed task_type in {:?}, got {:?}",
                expect.required_task_types_any,
                findings
                    .proposed_tasks
                    .iter()
                    .map(|p| p.task_type.as_str())
                    .collect::<Vec<_>>()
            ));
        }
    }

    if !expect.required_evidence_tools_any.is_empty() {
        let all_evidence: String = findings
            .findings
            .iter()
            .map(|f| f.evidence.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        let any = expect
            .required_evidence_tools_any
            .iter()
            .any(|t| all_evidence.contains(&t.to_lowercase()));
        if !any {
            violations.push(format!(
                "expected at least one finding.evidence to cite one of {:?}; none matched",
                expect.required_evidence_tools_any
            ));
        }
    }

    violations
}

#[tokio::test]
#[ignore = "live LLM eval; run with `cargo test evals -- --ignored --nocapture`"]
async fn eval_content_review_investigate() {
    let _guard = super::EVAL_LOCK.lock().await;
    let backend = generation_backend().await;
    let mut reports = Vec::new();

    for case_path in list_cases("content_review_investigate") {
        let case: InvestigateCase = load_fixture(&case_path);
        let analysis = format_tool_evidence_as_analysis(&case.tool_evidence);
        let prompt = build_content_review_investigation_extract_prompt(&analysis);
        let preamble = content_review_investigation_extract_preamble();

        let result = crate::rig::extraction::extract_with_backend::<InvestigationFindings>(
            &backend,
            &prompt,
            Some(preamble),
            Some("direct"),
            None,
        )
        .await;

        let mut violations = Vec::new();
        let mut judge = None;

        match result {
            Err(e) => violations.push(format!("extraction failed: {e}")),
            Ok(findings) => {
                violations.extend(check_findings(&findings, &case.expect));
                let judge_input = format!(
                    "Tool evidence:\n{}\n\nExtracted InvestigationFindings:\n{}",
                    serde_json::to_string_pretty(&case.tool_evidence).unwrap_or_default(),
                    serde_json::to_string_pretty(&findings).unwrap_or_default()
                );
                judge = judge_score(JUDGE_CRITERIA, judge_input).await;
            }
        }

        reports.push(CaseReport {
            name: case.name,
            violations,
            judge,
        });
    }

    finish_suite("content_review_investigate", reports);
}

// ─── Contract-check unit tests (no LLM — guard the eval harness itself) ──────

#[cfg(test)]
mod contract_tests {
    use super::*;
    use crate::models::content_review::{Finding, ProposedTask};
    use serde_json::json;

    fn finding(title: &str, evidence: &str, severity: &str, fix_type: &str) -> Finding {
        Finding {
            title: title.to_string(),
            description: "desc".to_string(),
            evidence: evidence.to_string(),
            severity: severity.to_string(),
            fix_type: fix_type.to_string(),
        }
    }

    fn proposal(task_type: &str, title: &str) -> ProposedTask {
        ProposedTask {
            task_type: task_type.to_string(),
            title: title.to_string(),
            reason: "because".to_string(),
            params: json!({}),
        }
    }

    fn findings(
        summary: &str,
        items: Vec<Finding>,
        proposed: Vec<ProposedTask>,
    ) -> InvestigationFindings {
        InvestigationFindings {
            summary: summary.to_string(),
            findings: items,
            proposed_tasks: proposed,
        }
    }

    #[test]
    fn fixtures_exist() {
        let cases = list_cases("content_review_investigate");
        assert!(
            cases.len() >= 3,
            "expected >=3 investigate fixtures, got {}",
            cases.len()
        );
        for path in &cases {
            let _: InvestigateCase = load_fixture(path);
        }
    }

    #[test]
    fn catches_unknown_task_type() {
        let f = findings(
            "Indexing is bad",
            vec![finding(
                "Many pages not indexed",
                "indexing_status shows 36 not_indexed",
                "critical",
                "hybrid",
            )],
            vec![proposal("not_a_real_task_type", "Do something")],
        );
        let violations = check_findings(&f, &Expect::default());
        assert!(
            violations.iter().any(|v| v.contains("unknown task_type")),
            "violations: {violations:?}"
        );
    }

    #[test]
    fn catches_healthy_site_with_proposals() {
        let f = findings(
            "Site looks fine overall",
            vec![finding(
                "No major issues",
                "indexing_status all indexed_pass; content_audit_report clean",
                "info",
                "informational",
            )],
            vec![proposal("ctr_audit", "Run CTR audit just in case")],
        );
        let expect = Expect {
            zero_proposals: true,
            max_proposals: 5,
            required_task_types_any: vec![],
            required_evidence_tools_any: vec![],
        };
        let violations = check_findings(&f, &expect);
        assert!(
            violations
                .iter()
                .any(|v| v.contains("expected zero proposed_tasks")),
            "violations: {violations:?}"
        );
    }

    #[test]
    fn catches_missing_tool_evidence_citation() {
        let f = findings(
            "Something is wrong",
            vec![finding(
                "Vague finding",
                "I just know titles are bad somehow",
                "warning",
                "developer_actionable",
            )],
            vec![],
        );
        let violations = check_findings(&f, &Expect::default());
        assert!(
            violations
                .iter()
                .any(|v| v.contains("known investigation tool")),
            "violations: {violations:?}"
        );
    }

    #[test]
    fn clean_findings_pass() {
        let f = findings(
            "Indexing coverage is the primary issue; titles and audit look healthy.",
            vec![finding(
                "High not-indexed rate",
                "indexing_status: 36/48 URLs not indexed (crawled_currently_not_indexed dominant)",
                "critical",
                "hybrid",
            )],
            vec![proposal(
                "indexing_health_campaign",
                "Recover non-indexed URLs",
            )],
        );
        let expect = Expect {
            zero_proposals: false,
            max_proposals: 5,
            required_task_types_any: vec!["indexing_health_campaign".to_string()],
            required_evidence_tools_any: vec!["indexing_status".to_string()],
        };
        let violations = check_findings(&f, &expect);
        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:?}"
        );
    }

    #[test]
    fn catches_duplicate_proposals_and_invalid_enums() {
        let f = findings(
            "Template and duplicates",
            vec![finding(
                "Bad severity",
                "article_title_scan found literal vars",
                "urgent",
                "maybe_fixable",
            )],
            vec![
                proposal("ctr_audit", "Same"),
                proposal("ctr_audit", "Same"),
            ],
        );
        let violations = check_findings(&f, &Expect::default());
        assert!(
            violations.iter().any(|v| v.contains("duplicate proposal")),
            "violations: {violations:?}"
        );
        assert!(
            violations.iter().any(|v| v.contains("invalid severity")),
            "violations: {violations:?}"
        );
        assert!(
            violations.iter().any(|v| v.contains("invalid fix_type")),
            "violations: {violations:?}"
        );
    }

    #[test]
    fn format_evidence_includes_tool_names() {
        let evidence = json!({
            "indexing_status": { "not_indexed": 10 },
            "article_title_scan": { "literal_var_titles": 2 }
        });
        let analysis = format_tool_evidence_as_analysis(&evidence);
        assert!(analysis.contains("indexing_status"));
        assert!(analysis.contains("article_title_scan"));
        assert!(analysis.contains("not_indexed"));
    }
}
