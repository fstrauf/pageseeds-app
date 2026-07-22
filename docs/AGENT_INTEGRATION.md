# Agent Integration

PageSeeds uses LLM agents for judgment-heavy tasks. This document covers how agents are invoked, how prompts are structured, and how responses are normalized.

For the rules that govern when to use agents vs. deterministic code, see [`AGENTS.md`](../AGENTS.md) → **Choose Execution Mode Deliberately** and **RIG / LLM Integration**.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         AGENT INTEGRATION                               │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│   ┌──────────────┐    ┌──────────────┐    ┌──────────────────┐         │
│   │   Handler    │───▶│   Rig        │───▶│   Artifact       │         │
│   │   (planner)  │      │  Provider  │      │   (JSON)         │         │
│   └──────────────┘    └──────────────┘    └──────────────────┘         │
│         │                              │                                │
│         │                              ▼                                │
│         │                        ┌──────────────┐                       │
│         │                        │ Rig Tools    │                       │
│         │                        │ (optional)   │                       │
│         │                        └──────────────┘                       │
│         │                                                               │
│         ▼                                                               │
│   ┌──────────────────────────────────────────────────────────────┐     │
│   │  SKILL.md (loaded from project automation dir or app defaults)│    │
│   │  - reddit_config.md                                          │     │
│   │  - content optimization instructions                         │     │
│   │  - apply_fix skill                                           │     │
│   └──────────────────────────────────────────────────────────────┘     │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

The canonical flow:

1. A workflow handler declares an `Agentic` step and names a `skill`.
2. The executor loads the skill, assembles context, and calls `engine::agent::run_agent_with_skill`.
3. `engine::agent` tries a **Rig-backed provider first** and falls back to the legacy CLI wrapper only if Rig signals fallback.
4. Raw output is returned to the executor or a typed `Extractor<T>` is used for structured output.

---

## Agent Providers

The primary integration is through [`rig-core`](https://github.com/0xPlaygrounds/rig) providers in `src-tauri/src/rig/`.

### Current Providers

- **Grok CLI** (default) — native `grok -p --always-approve` subprocess (`src-tauri/src/rig/grok_cli.rs`). Agentic file tools run with `--cwd` / process CWD = project root. Requires `grok` on PATH. Same shape as Kimi CLI for write_article / agentic steps. **Not** Rig multi-turn tool-capable for PageSeeds investigation tools — `content_review` investigate falls back to scripted recommend (same as Kimi CLI).
- **Kimi CLI** — native `kimi -p` subprocess (agentic file tools; investigate falls back to scripted recommend)
- **Claude** — Anthropic API via Rig (`ANTHROPIC_API_KEY`); pure completion, no project file I/O; Rig tool-capable
- **OpenAI** — OpenAI API via Rig (`OPENAI_API_KEY`); pure completion, no project file I/O; Rig tool-capable
- **Ollama** — local Ollama via Rig; Rig tool-capable
- **Kimi Bridge** — HTTP bridge to Kimi (legacy, opt-in); Rig tool-capable
- **Legacy CLI fallback** — `kimi` / `copilot` binaries via `agent-wrapper` (kept for compatibility)

### Provider Selection

Provider is resolved from:

1. Task's `agent_policy` field
2. Project legacy `agent_provider` if set and valid (prefer clear this; global is the intended control)
3. Global setting (`agent_provider` in `global_settings` table, default `"grok"`)

The resolved provider string is passed to `engine::agent::run_agent`.

API keys for Claude / OpenAI are loaded via `EnvResolver` (secrets.env → project `.env.local` / `.env` → shell). Grok CLI uses the local binary (no `XAI_API_KEY` required for the CLI path).

For Kimi specifically, the global `kimi_backend_mode` setting controls which backend is used: `"cli"` (the default) spawns `kimi -p` directly and enforces no prompt byte cap; `"bridge"` is legacy/opt-in and its retired 20 KB prompt limit no longer applies anywhere in the live pipeline — prompt sizes are governed by the shared 80 KB target / 90 KB hard budget (`config/prompt_budget.rs`).

**content_review agentic RO tool-loop** (PageSeeds investigation tools) requires a Rig tool-capable backend: Claude, OpenAI, Ollama, or Kimi Bridge — not Grok/Kimi CLI. Tool-equipped agents run through `run_tool_equipped_agent` with an `INVESTIGATION_MAX_TURNS` (20) multi-turn budget (aligned with BUSINESS_PROCESSES ≤20 tool calls); without it, rig-core 0.35 defaults to 0 turns and aborts with `MaxTurnError`.

---

## Step Types

### Agentic Step

Calls the LLM with a prompt assembled from a skill.

```rust
WorkflowStep::new("analyze_content", StepKind::Agentic)
    .with_param("skill", "content_analysis")  // Loads SKILL.md
```

**Executor behavior:**
1. Load SKILL.md from project `.github/skills/{skill}/SKILL.md` or app defaults (`src-tauri/src/skills/`)
2. Assemble context (task details, prior artifacts)
3. Call `run_agent_with_skill`
4. Store raw output in `latest_raw_output`
5. Return `StepResult`

For structured output, use `Extractor<T>` in the exec function rather than parsing `latest_raw_output` manually.

---

## Prompt Assembly

### SKILL.md Loading

Skills are loaded by `engine::skills::load_skill`:

```rust
pub fn load_skill(project_path: &Path, skill_name: &str) -> Result<Skill, Error>;
```

- Project skills override embedded defaults
- A skill file is markdown instructions, not the final prompt

### Context Assembly

The standard entry point is `engine::agent::run_agent_with_skill`:

```rust
pub fn run_agent_with_skill(
    skill_name: &str,
    repo_root: &Path,
    context: &str,
    agent_provider: &str,
    output_contract: Option<&str>,
) -> Result<String, String>;
```

It builds a prompt containing:
1. **SKILL.md content** — domain instructions
2. **Task context** — title, description, type, structured artifacts
3. **Output contract** (optional override)

### Output Contract

Every agentic step MUST document its expected output. Prefer putting the contract in the skill file itself. Only pass an explicit `output_contract` when the same skill needs different schemas in different workflows.

```rust
// Example output contract (usually lives in SKILL.md)
Return ONLY valid JSON matching this schema:
{
  "generated_at": "<ISO timestamp>",
  "articles": [
    {
      "article_id": <number>,
      "suggestions": [
        {
          "category": "title|meta_description|intro|...",
          "current": "<text>",
          "proposed": "<text>",
          "reason": "<text>"
        }
      ]
    }
  ]
}
```

---

## Structured Extraction

For typed agent output, use the Rig extraction wrapper:

```rust
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
pub struct ContentFixPatch {
    pub title: Option<String>,
    pub meta_description: Option<String>,
    pub changes: Vec<ContentChange>,
}

let patch = crate::rig::extraction::extract_with_backend::<ContentFixPatch>(
    agent_provider,
    &prompt,
    Some("direct"),
).await?;
```

This enforces the JSON schema and typically includes an automatic repair retry.

### Legacy JSON Extraction

For unstructured legacy steps, `engine::text::extract_json` handles common output formats:

```rust
pub fn extract_json(text: &str) -> Option<Value>;
```

Strategies:
1. Whole text is JSON
2. Fenced code block (```json ... ```)
3. Bare JSON object/array via brace matching

---

## Rig Tools

For agentic investigation or multi-tool workflows, expose deterministic capabilities as Rig tools in `src-tauri/src/engine/tools/`:

```rust
use rig::tool::{Tool, ToolDefinition};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Deserialize, JsonSchema)]
pub struct GscPerformanceArgs {
    keyword_filter: Option<String>,
    limit: Option<usize>,
}

pub struct GscPerformanceTool;

impl Tool for GscPerformanceTool {
    const NAME: &'static str = "gsc_performance";
    type Args = GscPerformanceArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition { ... }
    async fn call(&self, args: Self::Args) -> Result<Self::Output, rig::tool::ToolError> { ... }
}
```

Tools should be thin wrappers around existing domain functions — no new business logic.

---

## Safety & Constraints

### No Shell Escapes

Agents must NOT:
- Execute arbitrary shell commands
- Access files outside the project directory
- Make network requests directly (use deterministic steps/tools for APIs)

### Timeout Handling

Agent calls have default timeouts controlled by the provider/backend:
- Standard: 60 seconds
- Complex analysis: 120 seconds
- Batch operations: 30 seconds per item

---

## Testing Agent Integration

### Unit Tests

Test JSON extraction without calling agents:

```rust
#[test]
fn test_json_extraction_from_kimi_output() {
    let raw = r#"
    Here's the analysis:
    ```json
    {"score": 85, "issues": []}
    ```
    Hope this helps!
    "#;

    let result = engine::text::extract_json(raw).unwrap();
    assert_eq!(result["score"], 85);
}
```

### Integration Tests

Tests that require real provider credentials, local machine paths, or external APIs must be `#[ignore]`:

```rust
#[test]
#[ignore] // Requires Kimi bridge credentials
fn test_reddit_config_parsing_with_real_kimi() {
    let config_md = fs::read_to_string("test_config.md").unwrap();
    let config = extract_reddit_config(&config_md).unwrap();

    assert!(!config.trigger_keywords.is_empty());
    assert!(!config.seed_subreddits.is_empty());
}
```

### Eval Regression Tests (live, `rig::evals`)

Live eval suites regression-test the core agentic steps against committed fixtures.
They exist to catch **prompt/skill regressions** — run them after editing any
`src-tauri/skills/**/SKILL.md` or a prompt builder (`build_ctr_fix_prompt`,
`build_fix_prompt`, `build_single_article_prompt`).

```
./scripts/run-evals.sh          # or: cargo test evals -- --ignored --nocapture
# also runs as part of: pnpm test:all  (auto-skips when no provider is available;
# EVALS_REQUIRED=1 turns the skip into a failure)
```

Layout:

- `src-tauri/src/evals/` — harness + one suite per pipeline (`ctr_fix`, `content_fix`,
  `content_review`). Test-only (`#[cfg(test)]`), nothing ships in the binary.
- `src-tauri/fixtures/evals/<pipeline>/case-*.json` — hand-authored cases: realistic
  input (recommendation/context + article MDX) plus the slugs the model may link to.

Each case runs the real production step against a live backend, then two check layers:

1. **Deterministic contract checks** — length limits, keyword presence, echo of article
   identity, no hallucinated internal-link slugs. Free, reproducible, always gating.
2. **LLM judge** — `rig::evals::LlmScoreMetric` (0–1, threshold 0.7) for quality.
   Skipped when no judge key is set; deterministic checks still gate.

Env config: `EVAL_PROVIDER` (generation, default `kimi` via the CLI connector), `EVAL_JUDGE_PROVIDER`
plus `ANTHROPIC_API_KEY`/`OPENAI_API_KEY` for the judge. Add a case by dropping a new
`case-*.json` into the pipeline's fixture dir — no code changes needed.

---

## Common Pitfalls

### Structured extract tool schemas must be sanitized

Every structured-extract path must build tool/function parameters with
`crate::rig::schema_sanitize::schemars_tool_parameters::<T>()` (or
`sanitize_tool_parameters` on an existing schemars value). Raw schemars
output for nested `Option<Struct>` uses `anyOf` + `$ref`, which OpenAI-shaped
providers reject as `invalid_function_parameters` (e.g. `CtrFixPatch`).

**Do not** use unsanitized rig `Extractor<T>` for production patch types on
Claude / OpenAI / Ollama — `extract_with_backend` routes those providers through
`rig/openai_compatible_extract.rs` with sanitized schemas instead.

### 1. Sending Raw SKILL.md as Prompt

**Wrong:**
```rust
let prompt = fs::read_to_string("SKILL.md").unwrap();
```

**Right:**
```rust
let skill = load_skill(project_path, "content_analysis")?;
let context = build_context(task, artifacts)?;
let raw = run_agent_with_skill("content_analysis", project_path, &context, agent_provider, None)?;
```

### 2. Not Validating Output

Always normalize and validate agent output before using it:

```rust
let raw = run_agent_with_skill(...)?;
let parsed = extract_json(&raw).ok_or("invalid json")?;
validate_recommendations(&parsed)?;
```

### 3. Missing Output Contracts

Every agentic step must document expected output, either in the skill or in the handler comment:

```rust
// Output: JSON with { themes[], total_candidates, new_keywords[] }
```

### 4. Calling Agents for Deterministic Work

**Don't use agents for:**
- API calls (use `reqwest` directly or a Rig tool)
- Sorting/filtering (use Rust iterators)
- Date arithmetic (use `chrono`)

**Do use agents for:**
- Theme curation from ambiguous input
- Prioritization requiring judgment
- Prose generation
- Content quality assessment

---

## Files

| Component | Path |
|-----------|------|
| Agent invocation | `src-tauri/src/engine/agent.rs` |
| Rig provider layer | `src-tauri/src/rig/provider.rs` |
| Rig extraction | `src-tauri/src/rig/extraction.rs` |
| Prompt assembly | `src-tauri/src/engine/prompts.rs` |
| JSON extraction | `src-tauri/src/engine/text.rs` |
| Skill loading | `src-tauri/src/engine/skills.rs` |
| Rig tools | `src-tauri/src/engine/tools/` |

---

## See Also

- [Workflow Engine](./WORKFLOW_ENGINE.md) — How agentic steps fit into workflows
- [Business Processes](./BUSINESS_PROCESSES.md) — Which processes use agents
- [AGENTS.md](../AGENTS.md) — Rules for deterministic vs agentic work
