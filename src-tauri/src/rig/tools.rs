//! Rig tool system wrappers.
//!
//! Re-exports rig's native `Tool`, `ToolDyn`, and `ToolSet` types so that
//! PageSeeds tool implementations live in one place.
//!
//! Use this module when building agents with tools:
//! ```ignore
//! use crate::rig::tools::{Tool, ToolDyn};
//! use rig::tool::ToolSet;
//!
//! let tools: Vec<Box<dyn ToolDyn>> = vec![
//!     Box::new(MyTool),
//! ];
//! let agent = client.agent(model)
//!     .tools(tools)
//!     .build();
//! ```

pub use rig::tool::{Tool, ToolDyn, ToolSet, ToolSetBuilder};
pub use rig::completion::ToolDefinition;
