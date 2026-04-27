# Rig.rs Integration Specification

**Status:** Draft  
**Target:** Replace ad-hoc LLM/agent primitives with `rig-core` APIs while preserving the PageSeeds workflow engine.  
**Scope:** Backend Rust (`src-tauri/src/`) only. No frontend changes unless noted.  
**Estimated effort:** 4–6 development days (phased).

---

## Table of Contents

1. [Goal & Principles](#1-goal--principles)
2. [Prerequisites](#2-prerequisites)
3. [Phase 1 — Provider Abstraction](#3-phase-1--provider-abstraction)
4. [Phase 2 — Structured Output (Extractor)](#4-phase-2--structured-output-extractor)
5. [Phase 3 — Tool System Migration](#5-phase-3--tool-system-migration)
6. [Phase 4 — Embeddings & Vector Store](#6-phase-4--embeddings--vector-store)
7. [Phase 5 — RAG for Content & History](#7-phase-5--rag-for-content--history)
8. [Phase 6 — Step Consolidation](#8-phase-6--step-consolidation)
9. [Testing Strategy](#9-testing-strategy)
10. [Rollback Plan](#10-rollback-plan)

---

## 1. Goal & Principles

### Goal
Replace PageSeeds' hand-rolled LLM client, JSON extractor, tool registry, and embedding system with idiomatic `rig-core` primitives. The workflow engine (`executor.rs`, `handlers.rs`, `step_registry.rs`) and task lifecycle remain unchanged.

### Principles
1. **One phase at a time.** Each phase ships, passes tests, and is used in production before the next begins.
2. **Keep subprocess fallback.** Until Phase 3 is complete, retain `agent_wrapper` behind a feature flag or enum so CLI-based providers still work.
3. **No frontend changes unless required.** The `ExecutionResult` IPC contract is preserved.
4. **Type-safe extraction over regex.** Every new agentic step uses `Extractor<T>`. Legacy normalizer steps are migrated or deprecated.

---

## 2. Prerequisites

### 2.1 Dependency update
In `src-tauri/Cargo.toml`:
```toml
[dependencies]
# Remove:
# rig-core = "0.5"
# agent-wrapper = { git = "..." }

# Add:
rig-core = "0.10"   # or latest stable; verify docs match
schemars = "1.0"
# Keep reqwest, tokio, serde, serde_json — already present
```

> **Docs reference:** https://docs.rig.rs/docs/getting-started/installation  
> **Action:** Run `cargo check` after updating. Fix any breaking API changes from 0.5 → current.

### 2.2 Provider API keys
Direct HTTP providers need API keys. Ensure `config/env_resolver.rs` can resolve:
- `KIMI_API_KEY` (or `KIMI_BASE_URL` if self-hosted bridge)
- `OPENAI_API_KEY` (for Copilot-compatible endpoint, if applicable)
- `ANTHROPIC_API_KEY` (for Claude)
- `OLLAMA_BASE_URL` (already used in `skills_search.rs`)

### 2.3 Module declaration
Add `mod rig_integration;` to `src-tauri/src/lib.rs` (or create `src-tauri/src/rig/`). This is the integration layer — rig types are **not** scattered across existing modules.

---

## 3. Phase 1 — Provider Abstraction

**Objective:** Replace `engine/agent.rs` subprocess calls with native HTTP completion models via rig.  
**Files:** `src-tauri/src/engine/agent.rs`, `src-tauri/src/rig/provider.rs` (new)  
**Rig docs:** https://docs.rig.rs/docs/concepts/completion

### 3.1 Current state
```rust
// engine/agent.rs
pub fn run_agent(provider: &str, prompt: &str, project_path: &Path) -> Result<String, String> {
    let result = agent_wrapper::run_agent(provider, prompt, project_path)...
}
```
- Provider string values: `"kimi"`, `"copilot"`, `"claude"`
- Synchronous subprocess invocation via `agent-wrapper` crate
- No token usage, no streaming, no retry logic

### 3.2 Target state
```rust
// engine/agent.rs
pub async fn run_agent(
    provider: &LlmProvider,
    prompt: &str,
    system_preamble: Option<&str>,
) -> Result<AgentResponse, AgentError> {
    provider.complete(prompt, system_preamble).await
}

pub struct AgentResponse {
    pub content: String,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
}
```

### 3.3 Implementation steps

| Step | File | Action |
|---|---|---|
| 1 | `src-tauri/src/rig/provider.rs` | Create `LlmProvider` enum: `OpenAi`, `Anthropic`, `Kimi`, `Claude`, `Ollama`. |
| 2 | `src-tauri/src/rig/provider.rs` | Implement `From<&str>` for `LlmProvider` mapping existing provider strings. |
| 3 | `src-tauri/src/rig/provider.rs` | Create `CompletionClient` struct wrapping `rig::completion::CompletionModel` boxed trait object. |
| 4 | `src-tauri/src/rig/provider.rs` | For `OpenAi`: use `rig::providers::openai::Client::new(&api_key)`.completion_model(&model_name). |
| 5 | `src-tauri/src/rig/provider.rs` | For `Kimi`: if Kimi has OpenAI-compatible endpoint, reuse `providers::openai` with custom `base_url`. If not, implement custom `CompletionModel` (see rig docs "Provider Integration"). |
| 6 | `src-tauri/src/rig/provider.rs` | For `Claude`: use `rig::providers::anthropic::Client::new(&api_key)`. |
| 7 | `src-tauri/src/rig/provider.rs` | Implement `complete(prompt: &str, preamble: Option<&str>) -> Result<AgentResponse>` using `CompletionRequestBuilder`. |
| 8 | `engine/agent.rs` | Replace body of `run_agent` to call `CompletionClient::from_provider(provider).complete(prompt, None).await`. |
| 9 | `engine/agent.rs` | Update `AgentStatus` to include `token_usage` field (optional). |
| 10 | `step_registry.rs` | Remove `tokio::task::spawn_blocking` wrapper from `StepKind::Agentic` — the call is now async-native. |

> **Docs reference:** https://docs.rig.rs/docs/concepts/completion#provider-integration  
> **Key rig types:** `CompletionModel`, `CompletionRequestBuilder`, `CompletionResponse`, `Message`

### 3.4 Acceptance criteria
- [ ] `cargo check` passes.
- [ ] Existing agentic steps (`content_write_stage`, `research_seed_extraction`, etc.) execute successfully.
- [ ] Token usage is logged (even if not yet displayed in UI).
- [ ] Fallback: if `agent_provider` is unknown, error message suggests valid values.

---

## 4. Phase 2 — Structured Output (Extractor)

**Objective:** Replace regex-based `normalizer.rs` with rig's type-safe `Extractor<T>`.  
**Files:** `src-tauri/src/engine/normalizer.rs`, `src-tauri/src/rig/extraction.rs` (new), one handler as pilot  
**Rig docs:** https://docs.rig.rs/docs/concepts/extractors

### 4.1 Current state
```rust
// engine/normalizer.rs
pub struct NormalizedArtifact {
    pub raw_output: String,
    pub json_artifact: Option<Value>,
    pub extraction_method: String,
    pub success: bool,
}
```
- Heuristic extraction: fenced block → bare JSON → first JSON line → none
- No schema validation. No retry.

### 4.2 Target state
For the **pilot workflow** (`research_keywords`):
```rust
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct KeywordResearchOutput {
    pub themes: Vec<String>,
    pub validated_seeds: Vec<ValidatedSeed>,
    pub selections: Vec<KeywordSelection>,
}

// In the executor:
let extractor = client.extractor::<KeywordResearchOutput>(model);
let structured: KeywordResearchOutput = extractor
    .extract("Extract structured keyword research from this data", &raw_data)
    .await?;
```

### 4.3 Implementation steps

| Step | File | Action |
|---|---|---|
| 1 | `src-tauri/src/rig/extraction.rs` | Create `RigExtractor` wrapper that takes `&CompletionClient` + `JsonSchema` target type. |
| 2 | `src-tauri/src/rig/extraction.rs` | Implement `extract<T>(prompt: &str, context: &str) -> Result<T, ExtractionError>` using `rig::extractor::Extractor`. |
| 3 | `engine/normalizer.rs` | Keep `normalize_agent_output` for legacy steps. Mark with `#[deprecated(note = "Use rig extractor")]`. |
| 4 | `models/` (new) | Add `KeywordResearchOutput` struct with `#[derive(Deserialize, schemars::JsonSchema)]`. |
| 5 | `engine/exec/research.rs` | Modify `exec_research_final_selection` (or create new version) to call `RigExtractor::extract::<KeywordResearchOutput>` instead of returning raw text for normalizer. |
| 6 | `engine/workflows/handlers.rs` | Update `ResearchHandler::plan` for `research_keywords`: remove the `Normalizer` step. The deterministic `ResearchFinalSelection` step now calls the extractor directly and returns JSON. |
| 7 | `engine/executor.rs` | Remove special-casing for `research_ahrefs_pipeline` → `research_normalize` chaining. |

> **Docs reference:** https://docs.rig.rs/docs/concepts/extractors#target-data-requirements  
> **Key rig types:** `Extractor<T>`, `ExtractionError`, `schemars::JsonSchema`

### 4.4 Acceptance criteria
- [ ] `research_keywords` workflow produces `KeywordResearchOutput` without a separate normalizer step.
- [ ] Malformed LLM output triggers `ExtractionError` with a descriptive message (not silent regex failure).
- [ ] Frontend receives identical JSON structure — no TS type changes needed.

---

## 5. Phase 3 — Tool System Migration

**Objective:** Replace experimental `HttpToolAgent` + `ToolRegistry` with rig's `Tool` trait and `Agent` multi-turn loop.  
**Files:** `src-tauri/src/engine/tools/`, `src-tauri/src/engine/tool_agent/`, `src-tauri/src/rig/tools.rs` (new)  
**Rig docs:** https://docs.rig.rs/docs/concepts/tools

### 5.1 Current state
```rust
// engine/tools/mod.rs
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;
    fn execute(&self, params: Value) -> Pin<Box<dyn Future<Output = ToolResult> + Send + '_>>;
}

// engine/tool_agent/http_client.rs
pub struct HttpToolAgent { ... }
// Manual message loop, prompt-based tool invocation, no native function calling
```
- `HttpToolAgent` is ~500 lines of manual HTTP + parsing
- Only tools: `KeywordGeneratorTool`, `KeywordDifficultyTool`
- Not wired into main executor

### 5.2 Target state
```rust
use rig::tool::Tool;

#[derive(Deserialize, schemars::JsonSchema)]
pub struct KeywordGeneratorArgs {
    pub keyword: String,
    pub country: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum KeywordToolError { ... }

pub struct KeywordGeneratorTool;

#[tool(description = "Generate keyword ideas from a seed keyword using Ahrefs API")]
impl KeywordGeneratorTool {
    async fn keyword_generator(&self, args: KeywordGeneratorArgs) -> Result<String, KeywordToolError> {
        // existing Ahrefs call
    }
}
```

Then in an agent:
```rust
let mut tool_set = ToolSet::new();
tool_set.add_tool(KeywordGeneratorTool);
tool_set.add_tool(KeywordDifficultyTool);

let agent = Agent::builder(model)
    .preamble("You are a keyword research assistant.")
    .tools(tool_set)
    .build();

let response = agent.chat("Research keywords for coffee", 5).await?;
```

### 5.3 Implementation steps

| Step | File | Action |
|---|---|---|
| 1 | `src-tauri/src/rig/tools.rs` | Create wrapper module. Re-export `rig::tool::ToolSet`. |
| 2 | `engine/tools/keywords.rs` | Rewrite `KeywordGeneratorTool` and `KeywordDifficultyTool` using `#[tool]` derive (or manual `Tool` trait impl). Use `schemars` for args schema. |
| 3 | `engine/tools/mod.rs` | Remove old `Tool` trait and `ToolRegistry`. Re-export rig-based tools. |
| 4 | `engine/tool_agent/` | Delete `http_client.rs` and `mod.rs`. The `ToolCallingAgent` type alias is no longer needed. |
| 5 | `engine/exec/research.rs` | Create `exec_keyword_research_with_tools` function that builds a rig `Agent` with the two tools and runs a multi-turn conversation. |
| 6 | `engine/workflows/handlers.rs` | Add a new task type or step kind (e.g., `KeywordResearchToolAgent`) that uses the tool-based executor. Keep old path behind a feature flag for A/B testing. |
| 7 | `Cargo.toml` | Add `rig-core/tool` feature if required for `#[derive(Tool)]`. |

> **Docs reference:** https://docs.rig.rs/docs/concepts/tools#deriving-json-schemas-with-macros  
> **Key rig types:** `Tool`, `ToolSet`, `ToolDefinition`, `#[tool]` macro

### 5.4 Acceptance criteria
- [ ] `HttpToolAgent` is fully deleted.
- [ ] Keyword research can execute via rig `Agent::chat` with native tool calls.
- [ ] Tool calls are logged with arguments and results.
- [ ] `cargo test` passes (update or delete ignored tests in `http_client.rs`).

---

## 6. Phase 4 — Embeddings & Vector Store

**Objective:** Replace `skills_search.rs` manual Ollama + SQLite blob math with rig's `EmbeddingModel` + `VectorStore`.  
**Files:** `src-tauri/src/engine/skills_search.rs`, `src-tauri/src/rig/embeddings.rs` (new), `src-tauri/src/db/mod.rs`  
**Rig docs:** https://docs.rig.rs/docs/concepts/embeddings, https://docs.rig.rs/docs/concepts/vector-stores

### 6.1 Current state
```rust
// engine/skills_search.rs
const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";
const OLLAMA_BASE_URL: &str = "http://localhost:11434";
// Manual HTTP, manual f32 serialization, manual cosine similarity
```
- `skill_embeddings` table stores raw `f32` blobs
- Cosine similarity computed in Rust
- No provider abstraction

### 6.2 Target state
```rust
use rig::embeddings::{Embed, EmbeddingModel, EmbeddingsBuilder};
use rig_sqlite::SqliteVectorStore; // or rig::vector_store::InMemoryVectorStore

let model = openai_client.embedding_model("text-embedding-3-small");
// or
let model = ollama_client.embedding_model("nomic-embed-text");

let embeddings = EmbeddingsBuilder::new(model)
    .documents(skills)?
    .build()
    .await?;

store.add_rows(embeddings).await?;
let results = store.top_n(query, 5).await?;
```

### 6.3 Implementation steps

| Step | File | Action |
|---|---|---|
| 1 | `src-tauri/src/rig/embeddings.rs` | Create `EmbeddingProvider` enum (`Ollama`, `OpenAi`). Factory resolves from env/config. |
| 2 | `src-tauri/src/rig/embeddings.rs` | Create `SkillEmbedder` struct wrapping `EmbeddingModel` + `VectorStore`. |
| 3 | `Cargo.toml` | Add `rig-sqlite` dependency (or equivalent vector store crate). Verify it works with bundled `rusqlite`. |
| 4 | `db/mod.rs` | Add migration `MIGRATION_V15` to create `rig_skill_embeddings` table with rig-sqlite expected schema (or keep existing table if rig-sqlite allows custom table names). |
| 5 | `engine/skills_search.rs` | Rewrite `index_skills_blocking` to use `EmbeddingsBuilder` + `store.add_rows()`. |
| 6 | `engine/skills_search.rs` | Rewrite `search_skills_blocking` to use `store.top_n()`. Remove `cosine_similarity`, `serialize_embedding`, `deserialize_embedding`. |
| 7 | `engine/skills.rs` | Implement `Embed` for `Skill` (or map `Skill` → `Embedding::from` document). |

> **Docs reference:** https://docs.rig.rs/docs/concepts/embeddings#the-embedding-process  
> **Key rig types:** `EmbeddingModel`, `Embed`, `EmbeddingsBuilder`, `VectorStore`, `Embedding`

### 6.4 Acceptance criteria
- [ ] `index_skills_blocking` and `search_skills_blocking` use rig APIs.
- [ ] No manual `f32` byte serialization remains.
- [ ] Fallback behavior (Ollama unavailable) is preserved via error handling.
- [ ] Existing `ScoredSkill` JSON returned to frontend is unchanged.

---

## 7. Phase 5 — RAG for Content & History

**Objective:** Extend embeddings beyond skills to articles, GSC logs, and task artifacts using rig's dynamic context.  
**Files:** `src-tauri/src/rig/rag.rs` (new), `src-tauri/src/engine/prompts.rs`, `src-tauri/src/content/`  
**Rig docs:** https://docs.rig.rs/docs/concepts/agent#automatic-context-fetching

### 7.1 Current state
```rust
// engine/prompts.rs
let preview = if c.len() > 500 {
    format!("{}… [truncated]", crate::engine::text::char_prefix(c, 500))
} else { c.clone() };
```
- Artifacts truncated to 500 chars
- No semantic retrieval of past tasks, articles, or logs
- All context is static (full SKILL.md + full project context)

### 7.2 Target state
```rust
let agent = Agent::builder(model)
    .preamble(&skill.content)
    .context(build_project_context(...))
    .dynamic_context(artifact_store, 3) // top-3 relevant artifacts
    .build();
```

### 7.3 Implementation steps

| Step | File | Action |
|---|---|---|
| 1 | `src-tauri/src/rig/rag.rs` | Create `ContentStore` and `ArtifactStore` wrappers around `SqliteVectorStore`. |
| 2 | `content/ops.rs` or new module | Add `index_articles_for_rag(conn, project_id)` that embeds all `Article` bodies/descriptions. |
| 3 | `db/mod.rs` | Add `article_embeddings` table via migration if not using rig-sqlite generic table. |
| 4 | `engine/prompts.rs` | Refactor `build_prompt` to return `(String preamble, Vec<String> static_context)` instead of one giant string. |
| 5 | `engine/prompts.rs` | Add optional `dynamic_context: Vec<String>` parameter populated from `ArtifactStore::top_n(query, 3)`. |
| 6 | `engine/exec/content.rs` | In `exec_content_review_recommend`, build query from task description and fetch top-3 similar past content reviews to inject as dynamic context. |
| 7 | `engine/exec/ctr_audit.rs` | In `exec_ctr_analyze`, fetch top-performing article embeddings as examples of good metadata. |

> **Docs reference:** https://docs.rig.rs/docs/concepts/agent#rag-enabled-agent  
> **Key rig types:** `Agent::dynamic_context`, `VectorStore::top_n`

### 7.4 Acceptance criteria
- [ ] Agents receive semantically relevant past artifacts (not just truncated recent ones).
- [ ] Token usage per request does not exceed pre-RAG levels (dynamic context replaces static bloat).
- [ ] UI displays "Context sources" in task detail (optional enhancement — can be Phase 5.5).

---

## 8. Phase 6 — Step Consolidation

**Objective:** Collapse `Agentic` → `Normalizer` two-step chains into single `Agentic` steps using `Extractor<T>`.  
**Files:** `src-tauri/src/engine/workflows/handlers.rs`, `src-tauri/src/engine/step_registry.rs`  
**Rig docs:** https://docs.rig.rs/docs/concepts/extractors

### 8.1 Current state
Many handlers follow this pattern:
```rust
vec![
    WorkflowStep::new("something_agent", StepKind::Agentic),    // raw prose
    WorkflowStep::new("something_normalize", StepKind::Normalizer), // regex JSON
]
```

### 8.2 Target state
```rust
vec![
    WorkflowStep::new("something_extract", StepKind::AgenticExtract)
        .with_param("output_schema", "KeywordResearchOutput"),
]
```

### 8.3 Implementation steps

| Step | File | Action |
|---|---|---|
| 1 | `engine/workflows/step_kind.rs` | Add `AgenticExtract(String)` variant holding the target schema/type name. Or reuse `Agentic` with a `schema` param. |
| 2 | `engine/step_registry.rs` | Add handler for `AgenticExtract` that looks up the schema name, builds the prompt, calls `RigExtractor::extract::<T>`, and returns serialized JSON. |
| 3 | `engine/workflows/handlers.rs` | Migrate `ResearchHandler` (already done in Phase 2). |
| 4 | `engine/workflows/handlers.rs` | Migrate `ContentReviewHandler`: replace `content_review_recommend` + normalizer with single extract step using `ContentReviewOutput` struct. |
| 5 | `engine/workflows/handlers.rs` | Migrate `InvestigationHandler`: replace `investigate_gsc_agent` + normalizer with single extract step. |
| 6 | `engine/workflows/handlers.rs` | Migrate `RedditHandler`: replace `reddit_enrich` loop + normalizer with extract step. |
| 7 | `engine/normalizer.rs` | Once all call sites migrated, delete `normalizer.rs` entirely. |

### 8.4 Acceptance criteria
- [ ] No handler uses `StepKind::Normalizer` for new workflows.
- [ ] `normalizer.rs` is deleted or marked fully deprecated.
- [ ] All migrated workflows return identical JSON shapes to the frontend.

---

## 9. Testing Strategy

### 9.1 Unit tests
- `rig/provider.rs`: Mock `CompletionModel` using a test double that returns fixed strings. Verify `LlmProvider::from_str` mappings.
- `rig/extraction.rs`: Use `wiremock` (already a dev dependency) to mock LLM responses. Verify `Extractor<T>` deserializes correctly and fails gracefully on malformed JSON.
- `rig/tools.rs`: Unit-test each tool's `execute` logic independently of the agent loop.

### 9.2 Integration tests
- `executor.rs` already has an ignored test for keyword research (`execute_task_keyword_research_full_flow_with_mocked_http`). **Update this test** to use rig's `CompletionModel` instead of `agent-wrapper`. Remove the `#[ignore]` once Phase 1 is stable.
- Add a new integration test: `test_rig_extractor_keyword_research` that mocks the provider HTTP endpoint and verifies end-to-end `Extractor<KeywordResearchOutput>`.

### 9.3 Manual QA checklist
- [ ] Run `write_article` task end-to-end.
- [ ] Run `content_review` task end-to-end.
- [ ] Run `research_keywords` task end-to-end.
- [ ] Verify token usage appears in logs.
- [ ] Verify skills search still returns ranked results when Ollama is running.
- [ ] Verify skills search falls back gracefully when Ollama is stopped.

---

## 10. Rollback Plan

| Phase | Rollback trigger | Action |
|---|---|---|
| 1 | Provider HTTP errors / auth failures | Revert `agent.rs` to call `agent_wrapper`. Keep `CompletionClient` behind an enum: `LlmBackend::Rig` vs `LlmBackend::Subprocess`. |
| 2 | Extraction schema mismatches | Keep `Normalizer` step as optional fallback. If `Extractor` fails, fall back to `normalize_agent_output`. |
| 3 | Tool loop infinite loops | Reduce `max_turns` to 1. Revert to deterministic pipeline if needed. |
| 4 | SQLite vector store incompatibility | Keep old `skill_embeddings` table. Dual-write during transition. |

---

## Appendix A: File Inventory

### Files to create
```
src-tauri/src/rig/
├── mod.rs           # Re-exports, feature flags
├── provider.rs      # LlmProvider, CompletionClient
├── extraction.rs    # RigExtractor<T>
├── tools.rs         # Rig Tool wrappers, ToolSet builder
├── embeddings.rs    # EmbeddingProvider, SkillEmbedder
└── rag.rs           # ContentStore, ArtifactStore
```

### Files to modify
```
src-tauri/Cargo.toml
src-tauri/src/lib.rs
src-tauri/src/engine/agent.rs
src-tauri/src/engine/normalizer.rs          # deprecate → delete
src-tauri/src/engine/prompts.rs
src-tauri/src/engine/skills_search.rs       # rewrite
src-tauri/src/engine/tools/mod.rs           # rewrite
src-tauri/src/engine/tools/keywords.rs      # rewrite
src-tauri/src/engine/tool_agent/mod.rs      # delete
src-tauri/src/engine/tool_agent/http_client.rs # delete
src-tauri/src/engine/workflows/handlers.rs
src-tauri/src/engine/workflows/step_kind.rs
src-tauri/src/engine/step_registry.rs
src-tauri/src/engine/exec/research.rs
src-tauri/src/engine/exec/content.rs
src-tauri/src/engine/exec/ctr_audit.rs
src-tauri/src/engine/exec/cannibalization_audit.rs
src-tauri/src/db/mod.rs                     # migrations
```

### Files unchanged
```
src-tauri/src/engine/executor.rs            # orchestration stays
src-tauri/src/engine/task_store.rs          # SQLite CRUD stays
src-tauri/src/engine/spawner.rs             # task creation stays
src-tauri/src/engine/batch.rs               # batch loop stays
src-tauri/src/engine/scheduler.rs           # scheduler stays
src-tauri/src/commands/*.rs                 # thin wrappers stay
src/lib/tauri.ts                            # IPC stays
```

---

## Appendix D: Provider Architecture Deep Dive

This appendix addresses the specific question of **how PageSeeds should integrate Kimi** (which currently runs via CLI subprocess or via the `kimi-acp-openai-bridge`) into a rig-based provider abstraction.

### D.1 What We Learned from the Bridge Repo

The `kimi-acp-openai-bridge` (`/Users/fstrauf/01_code/kimi-acp-openai-bridge`) is a Python FastAPI server that:

1. Listens on `http://127.0.0.1:8080`
2. Exposes OpenAI-compatible endpoints: `/v1/chat/completions`, `/v1/models`, `/health`
3. Translates OpenAI format → Kimi ACP (Agent Client Protocol) → `kimi acp` subprocess
4. Supports streaming (SSE), tool calling, and structured output via **prompt-injected JSON schemas**
5. README literally shows the Rig integration pattern:

```rust
use rig::providers::openai;
let client = openai::Client::from_url(
    "http://localhost:8080/v1",
    "dummy-api-key"  // Bridge ignores this
);
let agent = client.agent("kimi-k2.5").build();
```

**This means: Rig already works with Kimi via the bridge — no custom provider needed.**

### D.2 Bridge Limitations (Why It Feels "Less Solid")

After reading the bridge source code, these are the actual limitations:

| Limitation | Location in bridge | Impact on PageSeeds |
|---|---|---|
| **Estimated token counts** | `translator.py:estimate_token_count()` uses `len(text) // 4` | Token usage metrics are rough, not billing-accurate |
| **Flattened message history** | `acp_client.py:_build_prompt_text()` folds all messages into one text block | Multi-turn conversation context is less precise than native chat |
| **Preamble ignored by ACP** | `acp_client.py` comment: "preamble is accepted by the bridge but currently ignored by Kimi ACP" | System prompts are prepended to user prompts as workaround |
| **JSON schema via prompt injection** | `translator.py:inject_response_format_to_prompt()` | Structured output relies on the model following instructions, not native API enforcement |
| **Single model support** | `server.py:AVAILABLE_MODELS` only lists `kimi-k2.5` | Cannot switch Kimi models via API |
| **Another process to manage** | Python FastAPI + `kimi acp` subprocess | Bridge must be running; crashes are opaque to Rust |

**Conclusion:** The bridge is functional but adds a translation layer with rough edges. Direct CLI invocation (`agent-wrapper`) is more robust for simple prompts but cannot support streaming, tools, or structured output.

### D.3 Recommended Provider Architecture

Do **not** write a custom Rig provider for Kimi. Use the bridge as the primary path and the direct CLI as the fallback. This gives you the best of both worlds.

```rust
// src-tauri/src/rig/provider.rs

use rig::providers::openai::{self, OpenAIClient};
use rig::providers::anthropic::{self, AnthropicClient};
use rig::completion::CompletionModel;

/// How to reach a given LLM.
pub enum LlmBackend {
    /// Kimi via the local ACP bridge (recommended when running).
    /// Maps to rig::providers::openai::Client::from_url("http://localhost:8080/v1", ...)
    KimiBridge { base_url: String },

    /// Kimi via direct CLI subprocess (fallback when bridge is down).
    /// Uses the existing agent-wrapper crate.
    KimiDirect,

    /// Claude via Anthropic API (native rig provider).
    Claude { api_key: String, model: String },

    /// OpenAI via native API (native rig provider).
    OpenAi { api_key: String, model: String },

    /// Ollama via OpenAI-compatible endpoint.
    /// Ollama exposes /v1/chat/completions natively.
    Ollama { base_url: String, model: String },
}

/// Resolved at runtime based on health checks and settings.
pub struct LlmProvider {
    pub backend: LlmBackend,
    pub model_name: String,
}

impl LlmProvider {
    /// Detect the best available Kimi backend.
    /// 1. Try bridge health check on localhost:8080
    /// 2. If healthy → KimiBridge
    /// 3. If unhealthy → KimiDirect (agent-wrapper)
    pub async fn resolve_kimi() -> Self { ... }

    /// Build a rig CompletionModel from this provider.
    pub async fn completion_model(&self) -> Box<dyn CompletionModel> {
        match &self.backend {
            LlmBackend::KimiBridge { base_url } => {
                let client = openai::Client::from_url(base_url, "dummy");
                Box::new(client.completion_model(&self.model_name))
            }
            LlmBackend::Claude { api_key, model } => {
                let client = anthropic::Client::new(api_key);
                Box::new(client.completion_model(model))
            }
            LlmBackend::OpenAi { api_key, model } => {
                let client = openai::Client::new(api_key);
                Box::new(client.completion_model(model))
            }
            LlmBackend::Ollama { base_url, model } => {
                let client = openai::Client::from_url(base_url, "dummy");
                Box::new(client.completion_model(model))
            }
            LlmBackend::KimiDirect => {
                panic!("KimiDirect does not implement CompletionModel; use run_agent_direct()")
            }
        }
    }

    /// For the direct CLI fallback, keep the old subprocess path.
    pub fn run_agent_direct(&self, prompt: &str, project_path: &Path) -> Result<String, String> {
        agent_wrapper::run_agent("kimi", prompt, project_path)
            .map(|r| r.raw_output)
            .map_err(|e| e.to_string())
    }
}
```

### D.4 Health Check & Auto-Detection

```rust
// src-tauri/src/rig/provider.rs

async fn check_bridge_health(base_url: &str) -> bool {
    let client = reqwest::Client::new();
    match client
        .get(format!("{}/health", base_url.trim_end_matches("/v1")))
        .timeout(Duration::from_secs(2))
        .send()
        .await
    {
        Ok(resp) => {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                json.get("kimi_available").and_then(|v| v.as_bool()).unwrap_or(false)
            } else {
                false
            }
        }
        Err(_) => false,
    }
}
```

**Settings UI implication:** The user selects a provider (`kimi`, `claude`, `openai`, `ollama`). For `kimi`, the backend auto-detects bridge vs direct. A status indicator shows which path is active.

### D.5 When to Use Which Path

| Scenario | Recommended Path | Why |
|---|---|---|
| Bridge running, need tools/extraction/streaming | `KimiBridge` | Full rig feature set |
| Bridge running, simple one-shot prompt | `KimiBridge` or `KimiDirect` | Either works; bridge has token estimation overhead |
| Bridge not running, user needs quick result | `KimiDirect` | No setup, most reliable |
| User has Claude/OpenAI API key | `Claude` / `OpenAi` | Native rig providers, no bridge needed |
| Local-only, no cloud | `Ollama` | Self-hosted, OpenAI-compatible endpoint |

### D.6 Why Not Write a Custom Rig Provider for Kimi?

You could implement `CompletionModel` for Kimi ACP directly in Rust (spawning `kimi acp` and speaking JSON-RPC over stdio). This would eliminate the Python bridge entirely. However:

1. **The bridge already works.** It handles edge cases (permission requests, tool call deltas, session lifecycle) that you would have to reimplement.
2. **ACP is undocumented/unstable.** The bridge author reverse-engineered it; keeping that logic in one place (the bridge) is safer.
3. **Rig's OpenAI provider is battle-tested.** Using it via the bridge gives you streaming, structured output, and tool calling for free.
4. **Fallback exists.** If the bridge breaks, `KimiDirect` keeps the app working.

**Revisit a native Rust ACP provider only if:**
- The bridge becomes unmaintained
- You need to ship a single binary with no Python dependency
- ACP is officially documented by Moonshot AI

### D.7 Updated Phase 1 Steps (Provider)

Replace the Phase 1 table in the main spec with these refined steps:

| Step | File | Action |
|---|---|---|
| 1 | `src-tauri/src/rig/provider.rs` | Create `LlmBackend` enum with `KimiBridge`, `KimiDirect`, `Claude`, `OpenAi`, `Ollama`. |
| 2 | `src-tauri/src/rig/provider.rs` | Implement `LlmProvider::resolve_kimi()` with bridge health check + fallback. |
| 3 | `src-tauri/src/rig/provider.rs` | Implement `completion_model()` returning `Box<dyn CompletionModel>` for rig-backed backends. |
| 4 | `src-tauri/src/rig/provider.rs` | Implement `run_agent_direct()` for `KimiDirect` using existing `agent-wrapper`. |
| 5 | `engine/agent.rs` | Replace `run_agent()` with async version that calls `LlmProvider::resolve_kimi()` → `.completion_model().complete(...).await`. |
| 6 | `engine/agent.rs` | On `CompletionModel` failure, log error and **do not** auto-fallback to `KimiDirect` — let the executor decide whether to retry. |
| 7 | `config/env_resolver.rs` | Add `KIMI_BRIDGE_URL` env var resolution (default: `http://localhost:8080/v1`). |
| 8 | `db/global_settings.rs` | Add `kimi_backend_mode` setting: `"auto"`, `"bridge"`, `"direct"`. Default `"auto"`. |

---

## Appendix B: Rig Documentation Quick Reference

| Topic | URL |
|---|---|
| Agents | https://docs.rig.rs/docs/concepts/agent |
| Completions | https://docs.rig.rs/docs/concepts/completion |
| Tools | https://docs.rig.rs/docs/concepts/tools |
| Extractors | https://docs.rig.rs/docs/concepts/extractors |
| Embeddings | https://docs.rig.rs/docs/concepts/embeddings |
| Loaders | https://docs.rig.rs/docs/concepts/loaders |

---

## Appendix C: Glossary

| Term | Meaning in this spec |
|---|---|
| **Agentic step** | A workflow step that invokes an LLM (currently `StepKind::Agentic`). |
| **Deterministic step** | A workflow step that runs pure Rust code (API calls, file I/O). |
| **Extractor** | Rig's type-safe structured output mechanism (`Extractor<T>`). |
| **RAG** | Retrieval-Augmented Generation — fetching relevant documents from a vector store at query time. |
| **Tool** | A capability an agent can invoke (e.g., call Ahrefs API). |
| **Vector Store** | Database for embedding vectors + similarity search. |
