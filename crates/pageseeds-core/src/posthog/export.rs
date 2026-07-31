use crate::error::Result;
use crate::posthog::models::PosthogCollection;
use std::path::Path;

const COLLECTION_FILE: &str = "posthog_collection.json";

fn ensure_parent(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// Write the collection artifact to the automation directory.
pub fn write_collection(automation_dir: &Path, collection: &PosthogCollection) -> Result<()> {
    let path = automation_dir.join(COLLECTION_FILE);
    ensure_parent(&path)?;
    let json = serde_json::to_string_pretty(collection)?;
    std::fs::write(&path, json)?;
    Ok(())
}

/// Read the collection artifact if it exists.
#[allow(dead_code)]
pub fn read_collection(automation_dir: &Path) -> Result<Option<PosthogCollection>> {
    let path = automation_dir.join(COLLECTION_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)?;
    let collection = serde_json::from_str(&raw)?;
    Ok(Some(collection))
}
