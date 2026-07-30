//! Closed-loop outcome aggregates for site overview (#302).
//!
//! Pure inventory pipeline over `content_outcome_results` + `ctr_outcomes`.
//! Called from [`super::builders::build_site_overview`] orchestration only.
//!
//! [`super::types::OutcomesInventory`] lives in `types.rs`.

use rusqlite::Connection;

use crate::db;

use super::types::OutcomesInventory;

/// Aggregate content_outcome_results + ctr_outcomes for the desk (#302).
pub(crate) fn build_outcomes_inventory(conn: &Connection, project_id: &str) -> OutcomesInventory {
    let mut inv = OutcomesInventory::default();

    if let Ok(content_rows) = db::list_content_outcome_results(conn, project_id) {
        inv.content_total = content_rows.len();
        for row in &content_rows {
            match row.classification.as_str() {
                "improved" => inv.content_improved += 1,
                "regressed" => inv.content_regressed += 1,
                "neutral" => inv.content_neutral += 1,
                "insufficient_data" => inv.content_insufficient_data += 1,
                _ => {}
            }
        }
    }

    if let Ok(ctr_rows) = db::list_ctr_outcomes(conn, project_id) {
        inv.ctr_total = ctr_rows.len();
        for row in &ctr_rows {
            match row.outcome_status.as_str() {
                "improved" => inv.ctr_improved += 1,
                "regressed" => inv.ctr_regressed += 1,
                "neutral" => inv.ctr_neutral += 1,
                "insufficient_data" => inv.ctr_insufficient_data += 1,
                "deployment_unverified" => {
                    inv.ctr_deployment_unverified += 1;
                    inv.ctr_stuck_pending += 1;
                }
                "pending" => {
                    inv.ctr_pending += 1;
                    inv.ctr_stuck_pending += 1;
                }
                "deployed" => inv.ctr_pending += 1,
                _ => {}
            }
        }
    }

    inv
}
