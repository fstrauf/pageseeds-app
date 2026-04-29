# Agent Integration

PageSeeds uses LLM agents (Kimi, Copilot) for judgment-heavy tasks. This document covers how agents are invoked, how prompts are structured, and how responses are normalized.

---

## Architecture

```
┌─────────────────────────────────────────────────────────────────────────┐
│                         AGENT INTEGRATION                               │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│   ┌──────────────┐    ┌──────────────┐    ┌──────────────────┐         │
│   │   Handler    │───▶│   Agent      │───▶│   Artifact       │         │
│   │   (planner)  │    │   (LLM call) │    │   (JSON)         │         │
│   └──────────────┘    └──────────────┘    └──────────────────┘         │
│         │                                                               │
│         ▼                                                               │
│   ┌──────────────────────────────────────────────────────────────┐     │
│   │  SKILL.md (loaded from project automation dir)               │     │
│   │  - reddit_config.md                                          │     │
│   │  - content optimization instructions                         │     │
│   │  - apply_fix skill                                           │     │
│   └──────────────────────────────────────────────────────────────┘     │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Agent Providers

Currently supported:
- **Kimi** (`kimi` binary) — Primary, local CLI
- **Copilot** (`copilot` binary) — GitHub Copilot CLI

Future:
- Claude API
- OpenAI API

### Provider Selection

```rust
// engine/agent.rs
pub enum AgentProvider {
    Kimi,
    Copilot,
}
```

Set via task's `agent_policy` field or default in settings.

---

## Step Types

### 1. Agentic Step

Calls the LLM with a prompt, stores raw output.

```rust
WorkflowStep::new("analyze_content", StepKind::Agentic)
    .with_param("skill", "content_analysis")  // Loads SKILL.md
```

**Executor behavior:**
1. Load SKILL.md from `{automation_dir}/SKILL.md` (or named skill)
2. Assemble context (task details, prior artifacts)
3. Call agent provider
4. Store raw output in `latest_raw_output`
5. Return StepResult

---

## Prompt Assembly

### SKILL.md Loading

Skills are loaded from the project's automation directory:

```rust
// engine/skills.rs
pub fn load_skill(project_path: &Path, skill_name: &str) -> Result<String>;
```

- Default: `SKILL.md`
- Named: `{skill_name}.md`

### Context Assembly

```rust
// engine/prompts.rs
pub fn assemble_agent_prompt(
    skill: &str,
    task: &Task,
    artifacts: &[TaskArtifact],
    output_contract: &str,
) -> String;
```

The prompt includes:
1. **SKILL.md content** — domain instructions
2. **Task context** — title, description, type
3. **Prior artifacts** — structured data from previous steps
4. **Output contract** — expected JSON schema

### Output Contract

Every agentic step MUST document its expected output:

```rust
// Example output contract
const RECOMMENDATIONS_CONTRACT: &str = r#"
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
"#;
```

---

## JSON Extraction

Agent output often contains markdown fences or explanatory text. The shared helper in `engine/text.rs` handles this:

```rust
// engine/text.rs

pub fn extract_json(text: &str) -> Option<Value> {
    // 1. Whole text is JSON
    // 2. Fenced code block (```json ... ```)
    // 3. Bare JSON object/array
}
```

For typed extraction, use:

```rust
pub fn extract_json_as<T: serde::de::DeserializeOwned>(text: &str) -> Option<T> {
    extract_json(text)?.as_object()?. ...
}
```

### Extraction Strategies

1. **Clean JSON** — Direct parse
2. **Markdown fences** — Extract from ```json ... ```
3. **Brace matching** — Find first `{` to last `}`

---

## Reddy-Specific Agent Flow

Reddit has the most complex agent integration:

```
1. Config Parse (Agentic)
   Input: reddit_config.md (free-form markdown)
   Output: Structured RedditConfig JSON
   
2. Search (Deterministic)
   Input: RedditConfig
   Output: Raw posts from Reddit API
   
3. Enrichment (Agentic, batched)
   Input: Batch of posts
   Output: Scored opportunities with reply drafts
```

### Config Parsing Example

```rust
// engine/exec/reddit.rs

pub fn extract_reddit_config(raw: &str) -> Result<RedditConfig> {
    let prompt = format!(
        "Extract structured Reddit search config from this markdown:\n\n{}\n\n{}",
        raw,
        REDDIT_CONFIG_CONTRACT
    );
    
    let response = call_agent(&prompt)?;
    normalize_reddit_config(&response)
}
```

### Batched Enrichment

```rust
// After reddit_search step, executor runs inline enrichment loop

loop {
    let pending = reddit::db::get_pending_opportunities(conn, project_id)?;
    if pending.is_empty() { break; }
    
    // Batch process 5-10 at a time
    let batch: Vec<_> = pending.into_iter().take(10).collect();
    exec_reddit_enrich(conn, project_id, project_path, &batch, agent_provider)?;
}
```

---

## Safety & Constraints

### Permission Flags

Agent calls use restricted permissions:

```bash
# Kimi
copilot -p "$PROMPT" --allow-all-tools --deny-tool='shell(git:*)'

# Copilot
(similar restrictions)
```

### No Shell Escapes

Agents must NOT:
- Execute arbitrary shell commands
- Access files outside project directory
- Make network requests (use deterministic steps for APIs)

### Timeout Handling

Agent calls have default timeouts:
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
    
    let result = normalize_json_output(raw).unwrap();
    assert_eq!(result["score"], 85);
}
```

### Integration Tests

Test with real agent calls (marked `#[ignore]` for CI):

```rust
#[test]
#[ignore] // Requires Kimi CLI
fn test_reddit_config_parsing_with_real_kimi() {
    let config_md = fs::read_to_string("test_config.md").unwrap();
    let config = extract_reddit_config(&config_md).unwrap();
    
    assert!(!config.trigger_keywords.is_empty());
    assert!(!config.seed_subreddits.is_empty());
}
```

---

## Common Pitfalls

### 1. Sending Raw SKILL.md as Prompt

**Wrong:**
```rust
// Don't do this
let prompt = fs::read_to_string("SKILL.md").unwrap();
```

**Right:**
```rust
// SKILL.md is instructions, not the prompt
let skill = load_skill(project_path, "content_analysis")?;
let context = build_context(task, artifacts)?;
let prompt = assemble_agent_prompt(&skill, &context, &output_contract)?;
```

### 2. Not Validating Output

Always normalize and validate agent output before using it:

```rust
let raw = call_agent(&prompt).await?;
let parsed = normalize_json_output(&raw)?;
// Validate against expected schema
validate_recommendations(&parsed)?;
```

### 3. Missing Output Contracts

Every agentic step must document expected output:

```rust
// In handler or step definition:
// Output: JSON with { themes[], total_candidates, new_keywords[] }
```

### 4. Calling Agents for Deterministic Work

**Don't use agents for:**
- API calls (use `reqwest` directly)
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
| Prompt assembly | `src-tauri/src/engine/prompts.rs` |
| JSON extraction | `src-tauri/src/engine/text.rs` |
| Skill loading | `src-tauri/src/engine/skills.rs` |
| Reddit execution | `src-tauri/src/engine/exec/reddit.rs` |
| Content execution | `src-tauri/src/engine/exec/content.rs` |

---

## See Also

- [Workflow Engine](./WORKFLOW_ENGINE.md) — How agentic steps fit into workflows
- [Business Processes](./BUSINESS_PROCESSES.md) — Which processes use agents
