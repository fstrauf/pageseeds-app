//! Post-action: Spawn fix tasks from strategy.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use rusqlite::Connection;

use crate::engine::project_paths::ProjectPaths;
use crate::engine::workflows::StepResult;
use crate::models::task::{FollowUpPolicy, Task, TaskReviewSurface};

// ═══════════════════════════════════════════════════════════════════════════════
// Step 5: Create Fix Tasks
// ═══════════════════════════════════════════════════════════════════════════════

/// No longer auto-spawns destructive fix tasks.
///
/// Phase 2 requires explicit approval via the review UI before any merge
/// or hub tasks are created. The strategy is persisted as an
/// artifact and in `cannibalization_strategy.json` for review.
pub(crate) fn create_can_fix_tasks(
    _conn: &Connection,
    parent_task: &Task,
    _project_path: &str,
) -> Vec<String> {
    log::info!(
        "[cannibalization_audit] Task {} completed. Review required before spawning fix tasks.",
        parent_task.id
    );
    Vec::new()
}

