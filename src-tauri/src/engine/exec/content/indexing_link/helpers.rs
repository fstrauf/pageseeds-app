use crate::models::indexing_health::{IndexingLinkOutcome, IndexingLinkTarget};
use crate::models::task::Task;

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Shared fail message when `indexing_link_target` is missing or unparsable.
/// Keep all step failures DRY — do not invent execute-time slug discovery.
pub(crate) const MISSING_INDEXING_LINK_TARGET_MSG: &str =
    "Missing or invalid `indexing_link_target` artifact (key: indexing_link_target). \
     Create via a completed indexing_health_campaign / gsc_indexing_recovery child, \
     or operator path: create-task -t fix_indexing_internal_links -S <url-slug>";

/// Deserialize the typed target from the `indexing_link_target` artifact.
///
/// Accepts the stable document shape `{ "target": { ... } }`. Returns `None`
/// when the key is missing or the payload cannot be deserialized.
pub(crate) fn parse_target_artifact(task: &Task) -> Option<IndexingLinkTarget> {
    task.artifacts
        .iter()
        .find(|a| a.key == "indexing_link_target")
        .and_then(|a| a.content.as_ref())
        .and_then(|json| {
            // Prefer full document with outer `target` wrapper.
            if let Ok(doc) =
                serde_json::from_str::<crate::models::indexing_health::IndexingLinkTargetArtifact>(
                    json,
                )
            {
                return Some(doc.target);
            }
            // Fallback: bare target object (tests / legacy).
            serde_json::from_str::<IndexingLinkTarget>(json).ok()
        })
}

/// Read pipeline outcome from plan/apply step artifacts or free-form JSON.
pub(crate) fn outcome_from_json(v: &serde_json::Value) -> Option<IndexingLinkOutcome> {
    v.get("outcome")
        .and_then(|o| o.as_str())
        .and_then(IndexingLinkOutcome::parse)
}

/// Prefer apply artifact, then plan artifact, then on-disk plan file.
pub(crate) fn load_pipeline_outcome(
    task: &Task,
    plan: &serde_json::Value,
    apply: Option<&serde_json::Value>,
) -> Option<IndexingLinkOutcome> {
    apply
        .and_then(outcome_from_json)
        .or_else(|| outcome_from_json(plan))
        .or_else(|| {
            task.artifacts
                .iter()
                .find(|a| a.key == "indexing_link_apply")
                .and_then(|a| a.content.as_ref())
                .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
                .and_then(|v| outcome_from_json(&v))
        })
        .or_else(|| {
            task.artifacts
                .iter()
                .find(|a| a.key == "indexing_link_plan")
                .and_then(|a| a.content.as_ref())
                .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
                .and_then(|v| outcome_from_json(&v))
        })
}
