/// Shared helpers for workflow step executors.
///
/// Reduces duplicated JSON file I/O and error-handling boilerplate across
/// `engine/exec/` modules.

use std::path::Path;
use serde::{de::DeserializeOwned, Serialize};
use crate::engine::workflows::StepResult;

/// Read a JSON file from disk and deserialize it.
///
/// Returns a `StepResult` error on failure so callers can propagate directly
/// from a workflow step handler.
pub fn read_json<T: DeserializeOwned>(path: &Path, context: &str) -> Result<T, StepResult> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            return Err(StepResult {
                success: false,
                message: format!("{}: failed to read {}: {}", context, path.display(), e),
                output: None,
            });
        }
    };
    match serde_json::from_str(&content) {
        Ok(v) => Ok(v),
        Err(e) => Err(StepResult {
            success: false,
            message: format!("{}: invalid JSON in {}: {}", context, path.display(), e),
            output: None,
        }),
    }
}

/// Serialize a value to pretty JSON and write it to disk.
///
/// Returns a `StepResult` error on failure so callers can propagate directly
/// from a workflow step handler.
pub fn write_json<T: Serialize>(path: &Path, value: &T, context: &str) -> Result<(), StepResult> {
    let json = match serde_json::to_string_pretty(value) {
        Ok(j) => j,
        Err(e) => {
            return Err(StepResult {
                success: false,
                message: format!("{}: failed to serialize: {}", context, e),
                output: None,
            });
        }
    };
    match std::fs::write(path, json) {
        Ok(()) => Ok(()),
        Err(e) => Err(StepResult {
            success: false,
            message: format!("{}: failed to write {}: {}", context, path.display(), e),
            output: None,
        }),
    }
}
