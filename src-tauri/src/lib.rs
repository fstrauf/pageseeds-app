//! Desktop shell library — **temporarily non-building** (issue **#184**).
//!
//! Domain logic was moved to `crates/pageseeds-core` and the operator CLI to
//! `crates/pageseeds-cli` in **#183**. Domain modules under `src-tauri/src/` were
//! deleted so this tree is no longer a dual source of truth.
//!
//! What remains here is the Tauri shell surface only:
//! - `commands/` — `#[tauri::command]` IPC bindings (will rewire to core in #184)
//! - `main.rs` / this `lib.rs` — entrypoints
//! - `build.rs`, `tauri.conf.json`, icons
//!
//! Do **not** re-introduce domain modules (engine, content, db, models, …) here.
//! Edit domain code under `crates/pageseeds-core` only.

#![allow(dead_code)]

// Domain crates are the SoT. This package is intentionally not a workspace
// member and does not compile until #184 wires commands → pageseeds-core.
compile_error!(
    "src-tauri desktop shell is temporarily non-building after the #183 crate split. \
     Domain SoT is crates/pageseeds-core + crates/pageseeds-cli. \
     Rebuild the shell against pageseeds-core in #184. \
     Operator gates: cargo test -p pageseeds-core / cargo build -p pageseeds-cli / pnpm test:cli"
);

/// Placeholder so `main.rs` keeps a stable symbol name until #184.
pub fn run() {
    unreachable!("desktop shell deferred to #184");
}

/// Placeholder CLI entry used by legacy `main.rs` arg routing; operator CLI is
/// `crates/pageseeds-cli`.
pub fn run_cli(_args: Vec<String>) -> Result<(), String> {
    Err("desktop shell CLI stubs removed; use pageseeds-cli (crates/pageseeds-cli)".into())
}

// Keep the commands tree on disk for #184 rewiring, but do not `mod commands`
// here — commands still import deleted domain paths and must not be compiled
// until they depend on pageseeds-core.
