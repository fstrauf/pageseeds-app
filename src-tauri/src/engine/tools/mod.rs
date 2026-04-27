//! Keyword research tools — rig-native `Tool` trait implementations.
//!
//! The old custom `Tool` trait, `ToolRegistry`, and `HttpToolAgent` have been
//! removed in favor of rig's built-in `tool::Tool` and `tool::ToolSet`.
//!
//! These tools are designed to be attached to a rig `Agent` for multi-turn
//! conversations where the LLM decides when to call the Ahrefs API.

mod keywords;
pub use keywords::{
    boxed_keyword_tools,
    KeywordDifficultyArgs,
    KeywordDifficultyOutput,
    KeywordDifficultyTool,
    KeywordGeneratorArgs,
    KeywordGeneratorOutput,
    KeywordGeneratorTool,
    KeywordIdea,
    KeywordToolError,
    SerpEntry,
};
