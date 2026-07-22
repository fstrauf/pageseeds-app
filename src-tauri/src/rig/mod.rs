//! Rig integration layer — provider abstraction and LLM utilities.
//!
//! This module wraps `rig-core` APIs and adapts them to PageSeeds' existing
//! `agent.rs` interface. It is the single point of contact between the
//! workflow engine and external LLM providers.

pub mod compat;
pub mod embeddings;
pub mod extraction;
pub mod grok_cli;
pub mod kimi_bridge;
pub mod kimi_cli;
pub mod provider;
pub mod tools;
