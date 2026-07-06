use crate::clarity::models::{ClarityCollection, ClaritySummary};
use crate::error::Result;
use std::path::Path;

const COLLECTION_FILE: &str = "clarity_collection.json";
const SUMMARY_FILE: &str = "clarity_summary.json";

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Write the collection artifact to the automation directory.
pub fn write_collection(automation_dir: &Path, collection: &ClarityCollection) -> Result<()> {
    let path = automation_dir.join(COLLECTION_FILE);
    ensure_parent(&path)?;
    let json = serde_json::to_string_pretty(collection)?;
    std::fs::write(&path, json)?;
    Ok(())
}

/// Read the collection artifact if it exists.
#[allow(dead_code)]
pub fn read_collection(automation_dir: &Path) -> Result<Option<ClarityCollection>> {
    let path = automation_dir.join(COLLECTION_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let collection = serde_json::from_str(&raw)?;
    Ok(Some(collection))
}

/// Write the summary artifact to the automation directory.
pub fn write_summary(automation_dir: &Path, summary: &ClaritySummary) -> Result<()> {
    let path = automation_dir.join(SUMMARY_FILE);
    ensure_parent(&path)?;
    let json = serde_json::to_string_pretty(summary)?;
    std::fs::write(&path, json)?;
    Ok(())
}

/// Read the summary artifact if it exists.
pub fn read_summary(automation_dir: &Path) -> Result<Option<ClaritySummary>> {
    let path = automation_dir.join(SUMMARY_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let summary = serde_json::from_str(&raw)?;
    Ok(Some(summary))
}
