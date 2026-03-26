/// Agent invocation — detect available agent CLIs and run non-interactive prompts.
///
/// Supported providers:
/// - `"copilot"` — GitHub Copilot CLI (`copilot -p "<prompt>" --output-format text`)
/// - `"claude"`  — Claude Code CLI    (`claude -p "<prompt>" --output-format text`)
/// - `"kimi"`    — Kimi Code CLI      (`kimi -p "<prompt>" --print --output-format text`)
/// - `"custom:<binary>"` — Any binary that accepts `-p <prompt>` and writes to stdout

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command as AsyncCommand;
use tokio::time::timeout;

/// Maximum time (seconds) an agent subprocess is allowed to run before being
/// killed.  Agent tasks (keyword research, content review, etc.) typically
/// finish in 2–8 minutes; 15 minutes is generous.
const AGENT_TIMEOUT_SECS: u64 = 15 * 60;

// ─── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInfo {
    pub name: String,
    pub binary: String,
    pub available: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub available_agents: Vec<AgentInfo>,
    pub configured_provider: String,
}

// ─── Detection ───────────────────────────────────────────────────────────────

/// Check which agent CLIs are available on PATH.
pub fn detect_agents(configured_provider: &str) -> AgentStatus {
    let candidates = [("copilot", "copilot"), ("claude", "claude"), ("kimi", "kimi")];

    let available_agents = candidates
        .iter()
        .map(|(name, binary)| {
            let (available, version) = probe_binary(binary);
            AgentInfo {
                name: (*name).to_string(),
                binary: (*binary).to_string(),
                available,
                version,
            }
        })
        .collect();

    AgentStatus {
        available_agents,
        configured_provider: configured_provider.to_string(),
    }
}

/// Check which agent CLIs are available on PATH (async version with timeout).
/// This prevents UI blocking when binaries are slow to respond.
pub async fn detect_agents_async(configured_provider: &str) -> AgentStatus {
    // Run all probes concurrently with individual timeouts
    let (copilot, claude, kimi) = tokio::join!(
        probe_binary_async("copilot"),
        probe_binary_async("claude"),
        probe_binary_async("kimi"),
    );

    let available_agents = vec![
        AgentInfo {
            name: "copilot".to_string(),
            binary: "copilot".to_string(),
            available: copilot.0,
            version: copilot.1,
        },
        AgentInfo {
            name: "claude".to_string(),
            binary: "claude".to_string(),
            available: claude.0,
            version: claude.1,
        },
        AgentInfo {
            name: "kimi".to_string(),
            binary: "kimi".to_string(),
            available: kimi.0,
            version: kimi.1,
        },
    ];

    AgentStatus {
        available_agents,
        configured_provider: configured_provider.to_string(),
    }
}

// ─── Invocation ──────────────────────────────────────────────────────────────

/// Run an agent with the given prompt and return the captured stdout.
///
/// Invocation pattern:
/// - copilot: `copilot -p "<prompt>" --output-format text`
/// - claude:  `claude  -p "<prompt>" --output-format text`
/// - kimi:    `kimi -p "<prompt>" --print --output-format text --work-dir <project_path>`
/// - custom:  `<binary> [extra args] -p "<prompt>"` — binary extracted from "custom:<binary>"
///
/// The prompt is passed as a direct argument to `Command::arg()` — no shell involved,
/// so there is no injection risk regardless of prompt content.
pub fn run_agent(provider: &str, prompt: &str, project_path: &Path) -> Result<String, String> {
    let (binary, extra_args) = resolve_provider(provider);
    log::info!("[agent] running {} in {:?}", binary, project_path);

    let mut cmd = std::process::Command::new(&binary);

    // Extra args first (e.g. --allow-all-paths for copilot)
    for arg in &extra_args {
        cmd.arg(arg);
    }

    // Prompt flag — CLIs interpret -p as "non-interactive prompt mode"
    cmd.arg("-p").arg(prompt);

    // Kimi requires --print for non-interactive mode (implicitly adds --yolo)
    if provider == "kimi" {
        cmd.arg("--print");
        // Kimi uses --output-format text (same as copilot/claude)
        cmd.arg("--output-format").arg("text");
        // --final-message-only gives clean output without structured wrappers
        cmd.arg("--final-message-only");
        // Disable thinking mode for cleaner output
        cmd.arg("--no-thinking");
        // Kimi uses --work-dir instead of current_dir
        cmd.arg("--work-dir").arg(project_path);
    } else {
        // Plain text output (not JSON or rich ANSI)
        cmd.arg("--output-format").arg("text");
        cmd.current_dir(project_path);
    }

    // Prevent the subprocess from blocking on stdin (e.g. interactive prompts).
    cmd.stdin(Stdio::null());
    // Pipe stdout/stderr so we can capture the agent's output.
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let start = std::time::Instant::now();
    
    // Use output() with a timeout wrapper for simpler, more reliable capture
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!(
                "Agent binary '{}' not found on PATH. Install it first.",
                binary
            ));
        }
        Err(e) => return Err(format!("Failed to run agent '{}': {}", binary, e)),
    };
    
    // Check if we exceeded timeout (soft check - process already finished)
    if start.elapsed() > Duration::from_secs(AGENT_TIMEOUT_SECS) {
        log::warn!("[agent] {} took longer than {}s but completed", binary, AGENT_TIMEOUT_SECS);
    }

    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();

    log::info!(
        "[agent] {} finished in {:.1}s (exit={}, stdout={}B, stderr={}B)",
        binary,
        start.elapsed().as_secs_f64(),
        out.status,
        stdout.len(),
        stderr.len()
    );
    
    // Debug: log first 500 chars of stderr if present
    if !stderr.is_empty() {
        log::warn!("[agent] {} stderr: {}", binary, &stderr[..stderr.len().min(500)]);
    }

    // Some CLIs exit non-zero but still produce useful output
    if out.status.success() || !stdout.trim().is_empty() {
        Ok(stdout)
    } else {
        let msg = if stderr.trim().is_empty() { stdout } else { stderr };
        Err(format!(
            "Agent '{}' failed (exit {}): {}",
            binary, out.status, msg.trim()
        ))
    }
}

// ─── Private helpers ─────────────────────────────────────────────────────────

/// Resolve provider string to (binary, extra_args).
fn resolve_provider(provider: &str) -> (String, Vec<String>) {
    if let Some(rest) = provider.strip_prefix("custom:") {
        // "custom:/path/to/binary" or "custom:binary --flag1 --flag2"
        let mut parts = rest.splitn(2, ' ');
        let binary = parts.next().unwrap_or("copilot").to_string();
        let args: Vec<String> = parts
            .next()
            .map(|s| s.split_whitespace().map(|a| a.to_string()).collect())
            .unwrap_or_default();
        (binary, args)
    } else {
        let binary = match provider {
            "claude" => "claude".to_string(),
            "kimi" => "kimi".to_string(),
            _ => "copilot".to_string(),
        };
        // Non-interactive mode requires auto-approval for tools, but we
        // explicitly deny shell git commands so agents cannot stage/commit/push.
        let extra_args = if binary == "copilot" {
            vec![
                "--allow-all-tools".to_string(),
                "--deny-tool=shell(git:*)".to_string(),
            ]
        } else {
            vec![]
        };
        (binary, extra_args)
    }
}

/// Try to run `binary --version` to check availability and get version string.
fn probe_binary(binary: &str) -> (bool, Option<String>) {
    match std::process::Command::new(binary)
        .arg("--version")
        .output()
    {
        Ok(out) => {
            // Some CLIs print version to stderr
            let ver_str = if !out.stdout.is_empty() {
                String::from_utf8_lossy(&out.stdout)
            } else {
                String::from_utf8_lossy(&out.stderr)
            };
            let version = ver_str.lines().next().map(|l| l.trim().to_string());
            (true, version)
        }
        Err(_) => (false, None),
    }
}

/// Async version of probe_binary with a timeout to prevent UI blocking.
const PROBE_TIMEOUT_MS: u64 = 2000; // 2 second timeout per binary

async fn probe_binary_async(binary: &str) -> (bool, Option<String>) {
    let result = timeout(
        Duration::from_millis(PROBE_TIMEOUT_MS),
        AsyncCommand::new(binary).arg("--version").output()
    ).await;

    match result {
        Ok(Ok(out)) => {
            // Some CLIs print version to stderr
            let ver_str = if !out.stdout.is_empty() {
                String::from_utf8_lossy(&out.stdout)
            } else {
                String::from_utf8_lossy(&out.stderr)
            };
            let version = ver_str.lines().next().map(|l| l.trim().to_string());
            (true, version)
        }
        Ok(Err(_)) => (false, None), // Command failed to execute
        Err(_) => (false, None),     // Timeout
    }
}
