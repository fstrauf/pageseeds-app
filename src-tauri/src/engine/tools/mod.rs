//! Agent tools — rig-native `Tool` trait implementations.
//!
//! Two tool families:
//! 1. `keywords` — Ahrefs API tools for keyword research (KeywordGeneratorTool, KeywordDifficultyTool)
//! 2. `investigate` — Project data tools for agentic investigation (GSC, articles, audit, etc.)
//!
//! Tools are designed to be attached to a rig `Agent` for multi-turn
//! conversations where the LLM decides when to call data access functions.

mod keywords;
pub use keywords::{
    boxed_keyword_tools, KeywordDifficultyArgs, KeywordDifficultyOutput, KeywordDifficultyTool,
    KeywordGeneratorArgs, KeywordGeneratorOutput, KeywordGeneratorTool, KeywordIdea,
    KeywordToolError, SerpEntry,
};

mod investigate;
pub use investigate::{
    investigation_tools,
    InvestigationContext,
    InvestigationToolError,
};
