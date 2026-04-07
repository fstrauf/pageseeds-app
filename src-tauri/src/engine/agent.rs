/// Agent invocation — detect available agent CLIs and run non-interactive prompts.
///
/// This module is now a thin wrapper around the agent-wrapper crate.

use serde::{Deserialize, Serialize};
use std::path::Path;

pub use agent_wrapper::detect_agents_cached as detect_agents;

/// Information about an available agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    pub binary: String,
    pub available: bool,
    pub version: Option<String>,
}

/// Agent status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub available_agents: Vec<AgentInfo>,
    pub configured_provider: String,
}

/// Check which agent CLIs are available on PATH.
pub fn detect_agents_sync(configured_provider: &str) -> AgentStatus {
    let agents = agent_wrapper::detect_agents_cached(false);
    
    let available_agents = agents
        .into_iter()
        .map(|a| AgentInfo {
            name: a.name,
            binary: a.binary,
            available: a.available,
            version: a.version,
        })
        .collect();
    
    AgentStatus {
        available_agents,
        configured_provider: configured_provider.to_string(),
    }
}

/// Run an agent with the given prompt and return the captured stdout.
///
/// This is a thin wrapper around agent_wrapper::run_agent that maintains
/// the original interface for backward compatibility.
pub fn run_agent(provider: &str, prompt: &str, project_path: &Path) -> Result<String, String> {
    let result = agent_wrapper::run_agent(provider, prompt, project_path)
        .map_err(|e| format!("Agent wrapper error: {}", e))?;
    
    if result.success {
        Ok(result.raw_output)
    } else {
        Err(result.error.unwrap_or_else(|| "Unknown agent error".to_string()))
    }
}

/// Async version of agent detection (uses cached results).
pub async fn detect_agents_async(configured_provider: &str) -> AgentStatus {
    // Run detection in blocking thread since agent-wrapper is sync
    let provider = configured_provider.to_string();
    tokio::task::spawn_blocking(move || detect_agents_sync(&provider))
        .await
        .unwrap_or_else(|_| AgentStatus {
            available_agents: vec![],
            configured_provider: configured_provider.to_string(),
        })
}
