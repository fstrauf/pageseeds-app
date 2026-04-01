/// Agent with tool calling capabilities
///
/// Uses raw HTTP for OpenAI-compatible API with tool calling support.
/// Designed to work with kimi-acp-openai-bridge for local Kimi Code MCP integration.
pub mod http_client;

pub use http_client::{HttpToolAgent, AgentResult, AgentError, AgentConfig};

// Alias for backward compatibility
pub type ToolCallingAgent = HttpToolAgent;
