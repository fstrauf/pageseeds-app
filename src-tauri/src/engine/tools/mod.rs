//! Agent tools — rig-native `Tool` trait implementations.
//!
//! Tool families:
//! - `investigate` — Project data tools for agentic investigation (GSC, articles, audit, etc.)
//! - `feature_spec` — Tools for feature spec generation
//! - `plateau_analysis` — GSC plateau analysis
//!
//! Tools are designed to be attached to a rig `Agent` for multi-turn
//! conversations where the LLM decides when to call data access functions.

pub mod investigate;
pub use investigate::{
    investigation_read_only_tools,
    investigation_tools,
    InvestigationContext,
    InvestigationToolError,
};

pub mod feature_spec;
pub use feature_spec::{
    feature_spec_tools,
    ArticleIndexTool,
    ReadArticleTool,
    GscNotIndexedTool,
    AnalyzeTitleTool,
    CheckTemporalUrlTool,
    LinkGraphSummaryTool,
};

pub mod plateau_analysis;
pub use plateau_analysis::{
    analyze_plateau,
    run_plateau_analysis_from_json,
    GscPageMetric,
    PlateauAnalysis,
};
