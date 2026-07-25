//! Test-only shared utilities. Nothing here ships in the binary.
#![cfg(test)]

use std::sync::Mutex;

/// Serializes tests that mutate process-global environment variables
/// (`PAGESEEDS_DB_PATH`, `KIMI_BRIDGE_URL`, …).
///
/// Rust runs tests on parallel threads within one process, so an env mutation
/// in one test is visible to every other concurrently running test — a test
/// that believes it mocked its backend or DB can silently resolve against
/// another test's (or the real) environment. Every test that calls
/// `std::env::set_var` / `remove_var` MUST hold this lock for its entire body.
/// A lock that only some tests honor is no lock at all.
pub static ENV_LOCK: Mutex<()> = Mutex::new(());
