# Universal Agent CLI Wrapper — Design Spec (Simplified)

## Problem Statement

Multiple applications need to invoke AI agent CLIs (Kimi Code, GitHub Copilot, Claude Code) in a consistent, language-agnostic way. Currently, each app re-implements CLI detection, process invocation, and output parsing.

## Solution

A single Rust binary that wraps agent CLIs and returns structured JSON. Language-specific packages are thin wrappers that shell out to this binary.

---

## Interface

### Commands

```bash
# Run a prompt
agent-wrapper --provider kimi --prompt "Generate a README" --work-dir ./project

# Via stdin (for large prompts)
cat prompt.txt | agent-wrapper --provider kimi --work-dir ./project

# Detect available agents (cached for 5 minutes)
agent-wrapper detect

# Detect with cache-bypass
agent-wrapper detect --refresh
```

### Output Format (JSON to stdout)

```json
{
  "success": true,
  "provider": "kimi",
  "exit_code": 0,
  "raw_output": "...markdown text...",
  "structured": {
    "extraction_method": "json_block",
    "data": { ... }
  },
  "duration_ms": 4500
}
```

### Error Format (JSON to stdout, success=false)

```json
{
  "success": false,
  "error": "AGENT_NOT_FOUND",
  "message": "Binary 'kimi' not found on PATH",
  "provider": "kimi"
}
```

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success (agent ran, output returned) |
| 1 | Error (agent not found, timeout, or execution failed) |

---

## Defaults (Hardcoded, No Config Files)

| Setting | Default | Override |
|---------|---------|----------|
| Timeout | 15 minutes | `--timeout-secs 900` |
| Output format | JSON | none (always JSON) |
| Session prefix | `agent-wrapper-{timestamp}` | none |
| Detection cache | 5 minutes | `--refresh` flag |

---

## Architecture

### Package Structure

```
agent-wrapper/
├── Cargo.toml
├── src/
│   ├── main.rs          # CLI entry
│   ├── lib.rs           # Public API for Rust embedding
│   ├── agent.rs         # Core: run_agent, detect_agents
│   ├── normalize.rs     # JSON extraction from output
│   ├── detect.rs        # CLI detection with caching
│   └── providers.rs     # Provider-specific command builders
├── bindings/
│   └── python/          # pip install agent-wrapper
│       ├── agent_wrapper.py
│       └── setup.py
└── install.sh           # curl | sh installer
```

### Rust API (for direct embedding)

```rust
use agent_wrapper::{run_agent, detect_agents, AgentOptions};

// Simple API
let result = run_agent("kimi", "Generate README", ".")?;
println!("{}", result.raw_output);

// With options
let result = run_agent_with_opts("kimi", AgentOptions {
    prompt: "Generate README".to_string(),
    work_dir: "./project".into(),
    timeout_secs: 900,
})?;

if let Some(json) = result.structured {
    println!("{}", json.data);
}
```

### Python Wrapper

```python
from agent_wrapper import run

# Simple
result = run("kimi", "Generate README", work_dir="./project")
print(result.raw_output)

# Check what agents are available
available = detect_agents()  # ["kimi", "copilot"]

# Access structured data
if result.structured:
    print(result.structured["data"])
```

---

## Implementation

### Detection Caching

Cache file: `~/.cache/agent-wrapper/detect.json`

```json
{
  "timestamp": "2026-03-30T12:00:00Z",
  "agents": [
    {"name": "kimi", "available": true, "version": "1.2.3"},
    {"name": "copilot", "available": false, "version": null}
  ]
}
```

- Cache valid for 5 minutes
- `agent-wrapper detect --refresh` bypasses cache
- Auto-refresh on first run if cache missing/expired

### Provider Command Mapping

```rust
fn build_command(provider: &str, prompt: &str, work_dir: &Path) -> Command {
    match provider {
        "kimi" => {
            cmd.arg("--print")
               .arg("--no-thinking")
               .arg("--output-format").arg("text")
               .arg("--final-message-only")
               .arg("--session").arg(format!("aw-{}", timestamp()))
               .arg("--work-dir").arg(work_dir)
               .arg("-p").arg(prompt)
        }
        "copilot" => {
            cmd.arg("--allow-all-tools")
               .arg("--deny-tool=shell(git:*)")
               .arg("--output-format").arg("text")
               .arg("-p").arg(prompt)
               .current_dir(work_dir)
        }
        "claude" => {
            cmd.arg("--output-format").arg("text")
               .arg("-p").arg(prompt)
               .current_dir(work_dir)
        }
        _ => panic!("Unknown provider: {}", provider)
    }
}
```

### Normalization (same as current PageSeeds)

1. Try extract from ` ```json ` fenced block
2. Try extract bare JSON object/array
3. Try first line that parses as JSON
4. Return raw only (structured: null)

---

## Distribution

### Phase 1: GitHub Releases
- Pre-built binaries for macOS (x64, ARM64), Linux (x64), Windows (x64)
- `install.sh`: detects platform, downloads, installs to `~/.local/bin` or `/usr/local/bin`

### Phase 2: Cargo
```bash
cargo install agent-wrapper
```

### Phase 3: Python
```bash
pip install agent-wrapper
# Downloads embedded binary on first import
```

---

## What We're NOT Building

- ❌ Streaming output (PageSeeds doesn't need it)
- ❌ Config files (reasonable defaults only)
- ❌ Security sandboxing (pass-through to agent CLI)
- ❌ Plugin system
- ❌ FFI bindings (subprocess only)
- ❌ Windows registry or macOS app bundles

---

## Migration from PageSeeds Current Code

Extract these files as-is, minimal changes:

1. `agent.rs` → `src/agent.rs` (remove Tauri-specific logging)
2. `normalizer.rs` → `src/normalize.rs` (unchanged)
3. Add CLI argument parsing with `clap`
4. Add JSON output wrapper

Roughly 400 lines of Rust becomes the core library.

---

## Success Criteria

- [ ] Binary builds for macOS, Linux, Windows
- [ ] Can detect and invoke Kimi CLI
- [ ] Returns valid JSON output
- [ ] Extraction works for fenced JSON blocks
- [ ] Detection results cached for 5 min
- [ ] Python package installs and works
- [ ] PageSeeds can replace its internal agent module with this
