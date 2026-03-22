use chrono::Utc;
use std::collections::HashMap;
use std::path::Path;

use crate::error::{Error, Result};
use crate::models::gsc::InspectionRecord;

/// Generate a Markdown indexing report and save both .md and .json artifacts.
/// Returns the path of the saved markdown file.
pub fn generate_and_save_indexing_report(
    records: &[InspectionRecord],
    site_url: &str,
    artifacts_dir: &Path,
) -> Result<String> {
    std::fs::create_dir_all(artifacts_dir)
        .map_err(|e| Error::Other(format!("Cannot create artifacts dir: {}", e)))?;

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let stem = format!("gsc_indexing_report_{}", timestamp);

    let md = render_markdown(records, site_url, &timestamp);
    let md_path = artifacts_dir.join(format!("{}.md", stem));
    std::fs::write(&md_path, &md)
        .map_err(|e| Error::Other(format!("Failed to write report MD: {}", e)))?;

    let json_path = artifacts_dir.join(format!("{}.json", stem));
    let json = serde_json::to_string_pretty(records)
        .map_err(|e| Error::Other(format!("JSON serialize error: {}", e)))?;
    std::fs::write(&json_path, &json)
        .map_err(|e| Error::Other(format!("Failed to write report JSON: {}", e)))?;

    Ok(md_path.to_string_lossy().into_owned())
}

fn render_markdown(records: &[InspectionRecord], site_url: &str, timestamp: &str) -> String {
    // Group by verdict
    let mut by_verdict: HashMap<&str, Vec<&InspectionRecord>> = HashMap::new();
    for r in records {
        by_verdict.entry(r.verdict.as_deref().unwrap_or("UNKNOWN")).or_default().push(r);
    }

    let total = records.len();
    let pass = by_verdict.get("PASS").map(|v| v.len()).unwrap_or(0);
    let fail = by_verdict.get("FAIL").map(|v| v.len()).unwrap_or(0);
    let neutral = by_verdict.get("NEUTRAL").map(|v| v.len()).unwrap_or(0);

    let mut md = format!(
        "# GSC Indexing Report\n\n\
         **Site:** {}\n\
         **Generated:** {}\n\
         **URLs inspected:** {}\n\n\
         ## Summary\n\n\
         | Verdict | Count |\n\
         |---------|-------|\n\
         | PASS (indexed) | {} |\n\
         | FAIL (not indexed) | {} |\n\
         | NEUTRAL (unknown) | {} |\n\n",
        site_url, timestamp, total, pass, fail, neutral
    );

    // FAIL section with action items — sorted by priority desc
    if let Some(mut fails) = by_verdict.get("FAIL").cloned() {
        fails.sort_by(|a, b| b.priority.cmp(&a.priority));
        md.push_str("## Action Required\n\n");
        md.push_str("| URL | Coverage State | Action | Priority |\n");
        md.push_str("|-----|---------------|--------|----------|\n");
        for r in &fails {
            md.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                r.url,
                r.coverage_state.as_deref().unwrap_or(""),
                r.action.as_deref().unwrap_or(""),
                r.priority
            ));
        }
        md.push('\n');
    }

    // NEUTRAL
    if let Some(neutrals) = by_verdict.get("NEUTRAL") {
        md.push_str("## Monitor\n\n");
        md.push_str("| URL | Coverage State |\n");
        md.push_str("|-----|---------------|\n");
        for r in neutrals.as_slice() {
            md.push_str(&format!("| {} | {} |\n", r.url, r.coverage_state.as_deref().unwrap_or("")));
        }
        md.push('\n');
    }

    // PASS — compact list
    if let Some(passed) = by_verdict.get("PASS") {
        md.push_str(&format!("## Indexed ({} pages)\n\n", passed.len()));
        for r in passed.as_slice() {
            md.push_str(&format!("- {}\n", r.url));
        }
    }

    md
}
