//! PageSeeds domain library — business logic with zero UI/Tauri dependencies.
//!
//! Used by `pageseeds-cli` only (desktop removed #184).

// rig-derive expands to `::rig_core::…` when it cannot resolve the crate name
// via proc_macro_crate (lib name is `rig`, package name is `rig-core`).
// Keep this alias so `#[derive(rig::Embed)]` compiles.
#[allow(unused_extern_crates)]
extern crate rig as rig_core;

pub mod cannibalization;
pub mod clarity;
pub mod config;
pub mod content;
pub mod db;
pub mod engine;
pub mod error;
pub mod gsc;
pub mod license;
pub mod live_site;
pub mod logging;
pub mod models;
pub mod project_config;
pub mod reddit;
pub mod rig;
pub mod seo;
pub mod social;
pub mod strategy;
pub mod video;

// Embedded prompt templates live under `src/prompts/` and are included via
// `include_str!` from engine modules; not a Rust module itself.

#[cfg(test)]
pub mod test_support;
