# Agentic Investigation Feature Spec

> **Status:** Archival design notes. Desktop UI and Tauri `commands/` were removed in **#184**. Implement via domain APIs + thin `pageseeds-cli` only; do not reintroduce React/`src/` or a commands layer under core.

## Problem

The Health Dashboard and its underlying tasks (`content_review`, `ctr_audit`, `cannibalization_audit`, `indexing_health_campaign`) run **pre-defined checks**. They find what you told them to find. They are excellent for recurring monitoring — "are we still within the guardrails?" — but they cannot discover novel issues.

The original daystoexpiry.com investigation succeeded because an agent had **access to the project's data** and could explore freely:
- Read GSC performance data → saw 0.1% CTR with 649k impressions
- Read article frontmatter → discovered `| Brand |` literal text in all 150 titles
- Hashed article bodies → found 6 byte-for-byte duplicates
- Read framework layout files → found the template variable substitution bug

None of these insights came from pre-coded checks. They came from an agent connecting dots across data sources that were never explicitly configured to be checked together.

## The Pattern: `know` Repo's Tool Catalog

The `know` repo (`config/tool_catalog.toml`) defines every data access tool an agent can use. Each tool entry describes:

```toml
[tools.portfolio_snapshot]
command = "know portfolio snapshot"
purpose = "Pull latest portfolio snapshot"
output_format = "json or human-readable table"
mutates = false
when_to_use = "Need current holdings, total value, P&L, or allocation"
when_not_to_use = "Do not use to modify holdings; read-only"
```

The agent loads this catalog into its context, then the user asks: *"Why is my portfolio underperforming?"* The agent checks available tools, calls `portfolio_snapshot` → `trades_active` → `options_iv` → searches the knowledge base, and returns a synthesized answer.

**The catalog is the bridge between "run pre-defined checks" and "explore freely."**

## What We Build

A tool catalog + Rig tool implementations that give a Kimi/Claude agent access to all project data. The user asks questions like:

- *"Why am I plateauing at 10K impressions?"*
- *"Why is my CTR dropping despite position improving?"*
- *"Are there any structural issues with my site?"*
- *"Which articles should I consolidate or improve?"*

The agent explores freely, calling tools as needed, and returns insights.

## Architecture

```
User: "Why am I plateauing at 10K impressions?"
    │
    ▼
┌──────────────────────────────────────────────┐
│ investigate command                          │
│ 1. Loads tool_catalog.toml → agent preamble  │
│ 2. Builds Rig agent with tools attached       │
│ 3. Agent calls tools freely:                  │
│    - get_gsc_performance() → impressions flat │
│    - scan_article_titles() → brand duplicated │
│    - hash_article_bodies() → 6 exact dupes    │
│    - read_framework_files() → template bug    │
│ 4. Synthesizes findings → structured output   │
│ 5. Saves evidence packet + answer markdown    │
└──────────────────────────────────────────────┘
    │
    ▼
InvestigationResult {
    answer: String,       // natural language synthesis
    evidence: Value,      // structured data the agent collected
    tools_called: Vec<String>,
    saved_at: PathBuf,    // .github/automation/investigations/
}
```

## Tool Catalog (`crates/pageseeds-core/config/tool_catalog.toml`)

Bundled into the binary via `include_str!()` and loaded by `engine/tools/investigate/catalog.rs`. TOML is authoritative for preamble text and `mutates` flags (Full vs ReadOnly is derived from `mutates`). Each tool has a corresponding Rust constructor in `engine/tools/investigate` implementing `rig::tool::Tool`. Multi-turn tool-agent attach runs through `rig::provider::run_tool_equipped_agent` (unsupported backends return typed `ToolAgentError::Unsupported`).

### Read-Only Data Tools

```toml
[tools.gsc_performance]
purpose = "Get Google Search Console page-level performance data (clicks, impressions, CTR, position) for the project"
when_to_use = "When investigating impression trends, CTR changes, or ranking movements"
when_not_to_use = "Do not use if GSC is not connected or the user hasn't asked about search performance"
mutates = false

[tools.article_list]
purpose = "List all articles in the project with their metadata (title, slug, file, status, published_date, target_keyword, word_count)"
when_to_use = "When you need to know what articles exist, their status, or basic metadata"
when_not_to_use = ""
mutates = false

[tools.article_frontmatter]
purpose = "Read frontmatter (title, description, date, keywords) and body word count for one or all articles from MDX files"
when_to_use = "When checking individual article metadata, frontmatter completeness, or keyword usage"
when_not_to_use = ""
mutates = false

[tools.article_body_hash]
purpose = "Compute SHA-256 hashes of all normalized article bodies to find exact duplicate content"
when_to_use = "When investigating duplicate content, SSR fallback pages, or content quality"
when_not_to_use = ""
mutates = false

[tools.article_title_scan]
purpose = "Scan all article titles for patterns: duplicated tokens, literal template variables, missing titles, truncation risk"
when_to_use = "When investigating title quality, template bugs, or SERP truncation"
when_not_to_use = ""
mutates = false

[tools.content_audit_report]
purpose = "Return the full content_audit.json report for the project (21 deterministic checks per article)"
when_to_use = "When you need the comprehensive article health data (score, issues, temporal URLs, bloat, duplicates)"
when_not_to_use = ""
mutates = false

[tools.cannibalization_clusters]
purpose = "Return cannibalization clusters and merge recommendations from cannibalization_strategy.json"
when_to_use = "When investigating keyword cannibalization, content consolidation, or topic overlap"
when_not_to_use = ""
mutates = false

[tools.indexing_status]
purpose = "Return GSC URL indexing status (indexed, not indexed, reasons) for the project"
when_to_use = "When investigating indexing problems, sitemap gaps, or crawl issues"
when_not_to_use = ""
mutates = false

[tools.ctr_health]
purpose = "Return CTR health summary (title length, meta quality, snippet optimization, FAQ presence) per article"
when_to_use = "When investigating CTR underperformance or on-page optimization issues"
when_not_to_use = ""
mutates = false

[tools.framework_files]
purpose = "Read framework config files from the project: Next.js layouts, sitemap config, redirect rules, robots.txt"
when_to_use = "When investigating site-wide template bugs, sitemap gaps, redirect issues, or framework configuration problems"
when_not_to_use = "Do not use if the project structure doesn't include these framework files"
mutates = false

[tools.article_link_graph]
purpose = "Return the internal link graph (incoming/outgoing links per article, orphaned articles)"
when_to_use = "When investigating internal linking gaps, orphaned content, or site structure issues"
when_not_to_use = ""
mutates = false
```

### Action Tools (Mutation)

```toml
[tools.run_content_audit]
purpose = "Run the deterministic content audit (21 checks) on the project. Writes content_audit.json."
when_to_use = "When you need fresh audit data or the current data is stale"
when_not_to_use = "If a recent audit exists (< 1 hour old), use the cached data"
mutates = true

[tools.create_task]
purpose = "Create a new task in the PageSeeds task system (e.g., fix_content_article, consolidate_cluster, content_cleanup)"
when_to_use = "When the investigation has found actionable issues that need automated fixing"
when_not_to_use = "Do not create tasks without explaining what each task will do and why"
mutates = true
```

## Rig Tool Implementations

Each tool in the catalog maps to a Rust struct implementing `rig::tool::Tool`. The tools are **thin wrappers** around existing Rust module functions — no new business logic.

```
crates/pageseeds-core/config/tool_catalog.toml          # Authoritative catalog (preamble + mutates)
crates/pageseeds-core/src/rig/provider.rs               # run_tool_equipped_agent, backend_supports_tool_calling
crates/pageseeds-core/src/engine/tools/investigate/
├── mod.rs              # Kit API, name→Tool constructors
├── catalog.rs          # include_str! TOML load, Full/RO catalog text
├── gsc.rs / articles.rs / audit.rs / project.rs / shared.rs
└── …
```

### Tool Implementation Pattern

Following the existing `KeywordGeneratorTool` pattern:

```rust
use rig::tool::{Tool, ToolDefinition};
use serde::{Deserialize, Serialize};
use schemars::JsonSchema;

#[derive(Deserialize, JsonSchema)]
pub struct GscPerformanceArgs {
    /// Optional: filter to articles matching this keyword in title or target_keyword
    keyword_filter: Option<String>,
    /// Optional: limit results (default 50)
    limit: Option<usize>,
}

pub struct GscPerformanceTool {
    project_id: String,
}

impl Tool for GscPerformanceTool {
    const NAME: &'static str = "gsc_performance";

    type Args = GscPerformanceArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.to_string(),
            description: "Get GSC page-level performance data...".to_string(),
            parameters: serde_json::to_value(schemars::schema_for!(GscPerformanceArgs))
                .unwrap_or_default(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, rig::tool::ToolError> {
        // Thin wrapper: call existing gsc::analytics functions
        let data = crate::gsc::analytics::get_page_metrics(&self.project_id, args.keyword_filter, args.limit)
            .map_err(|e| rig::tool::ToolError::ExecutionError(e.to_string()))?;
        Ok(serde_json::to_value(data).unwrap_or_default())
    }
}
```

## Operator API (domain + thin CLI)

```rust
// Domain functions in pageseeds-core (e.g. engine/exec/investigate.rs);
// CLI subcommands call these and print JSON — no Tauri commands layer.

/// Run an agentic investigation: the agent has access to data tools
/// and explores freely to answer the user's question.
pub async fn investigate(
    project_id: &str,
    question: &str,
) -> Result<InvestigationResult, Error>;

/// Get a list of available tools the agent can use.
pub fn get_available_tools() -> Result<Vec<ToolInfo>, Error>;

/// Get a previously saved investigation result.
pub fn get_investigation(
    project_id: &str,
    investigation_id: &str,
) -> Result<InvestigationResult, Error>;
```

## Investigation Result

```rust
#[derive(Serialize)]
pub struct InvestigationResult {
    pub id: String,
    pub question: String,
    pub answer: String,           // natural language synthesis
    pub summary: String,          // 1-2 sentence TL;DR
    pub evidence: serde_json::Value, // structured data collected by the agent
    pub tools_called: Vec<String>, // which tools the agent used
    pub findings: Vec<Finding>,   // structured issues found
    pub severity: Severity,
    pub created_at: String,
    pub saved_path: Option<String>, // path to saved markdown
}

#[derive(Serialize)]
pub struct Finding {
    pub title: String,
    pub description: String,
    pub evidence: String,         // what the tool returned to support this
    pub fix_type: FixType,
    pub auto_fix_task: Option<String>,
}

#[derive(Serialize)]
pub enum FixType {
    AutoFixable,
    DeveloperActionable,
    Hybrid,
    Informational,
}
```

## Execution Flow

```
1. User enters question → frontend calls investigate(projectId, question)
2. Backend loads embedded tool_catalog.toml → builds agent preamble (Full kit)
3. Backend gathers static evidence (project config, article count, etc.)
4. Backend calls `rig::provider::run_tool_equipped_agent`:
   - Provider: Kimi bridge / Claude / OpenAI / Ollama (tool-capable only)
   - Tools: Full investigation set (incl. mutators for standalone)
   - Preamble: tool catalog + usage rules + output contract
5. Agent runs:
   - Can call tools freely (up to 20 tool calls to prevent runaway)
   - Each tool call returns structured data
   - Agent interprets, synthesizes, calls more tools as needed
6. Agent returns structured InvestigationResult via Extractor<T>
7. Result saved to .github/automation/investigations/{id}/answer.md
8. Evidence saved to .github/automation/investigations/{id}/evidence.json
9. CLI prints InvestigationResult JSON (and/or writes answer.md + evidence.json)
```

## Operator surface (CLI)

Desktop Health Dashboard / "Ask AI" panel was removed in **#184**. Operators run investigations via CLI (question in → JSON / markdown artifacts out) or via embedded domain steps.

In addition, **`content_review` embeds the same read-only investigate loop** when
the configured backend supports tool calling (`ContentReviewInvestigate` step;
see issue #80). That path uses `InvestigationAccess::ReadOnly` (no create/enqueue
mutators), extracts typed `InvestigationFindings`, and stores them as the
`investigation_findings` artifact — it does not write `recommendations.json`.
When tools are unavailable (e.g. KimiCli), content_review falls back to the
scripted `content_review_recommend` path.

## Prevention: When to Use What

| Scenario | Use |
|---|---|
| Weekly health check | Desk + targeted audits / `content_review` when scoped |
| "Why is my traffic dropping?" | Investigate (agentic) |
| "Are there structural problems?" | Investigate (agentic) |
| "Which articles need work?" | Content audit → review priority issues / patterns |
| "Should I consolidate any content?" | Investigate (agentic) |
| Scheduled monitoring | Auto-enqueued collect_* + desk-first weekly path |
| Post-publish validation | Content audit / fix verify steps |

## Files to Create / Modify

### Domain (Rust)

| File | Change |
|------|--------|
| `crates/pageseeds-core/config/tool_catalog.toml` | Tool definitions with purpose/when_to_use/mutates |
| `engine/tools/investigate/catalog.rs` | Loads TOML via include_str!, builds agent preamble |
| `rig/provider.rs` | `run_tool_equipped_agent` + `ToolAgentError` |
| `engine/tools/mod.rs` | Re-exports investigation kit API |
| `engine/tools/gsc.rs` | New — GscPerformanceTool |
| `engine/tools/articles.rs` | New — ArticleListTool, ArticleFrontmatterTool, ArticleBodyHashTool, ArticleTitleScanTool |
| `engine/tools/audit.rs` | New — ContentAuditReportTool, RunContentAuditTool |
| `engine/tools/cannibalization.rs` | New — CannibalizationClustersTool |
| `engine/tools/indexing.rs` | New — IndexingStatusTool |
| `engine/tools/ctr.rs` | New — CtrHealthTool |
| `engine/tools/framework.rs` | New — FrameworkFilesTool |
| `engine/tools/linking.rs` | New — ArticleLinkGraphTool |
| `engine/tools/task.rs` | New — CreateTaskTool |
| `engine/exec/investigate.rs` | New — exec_investigate |
| Domain models for InvestigationResult | Typed JSON for CLI/artifacts |

### CLI

| File | Change |
|------|--------|
| `crates/pageseeds-cli` | Thin subcommand: parse args → call core → print JSON |

### Total new code: ~800–1100 lines Rust (domain + thin CLI); no React

Note: do **not** add `commands/investigate.rs` or any Tauri IPC registration.

## Success Metrics

1. Agent can answer "Why are my impressions plateauing?" with specific, evidence-backed findings
2. Agent discovers issues not caught by pre-defined checks (e.g., template bugs, literal variables)
3. Investigation completes in < 60 seconds
4. Agent makes ≤ 20 tool calls per investigation (bounded)
5. Results are saved for later review and comparison
6. The "Ask AI" panel is discoverable from the Health Dashboard
7. Users understand the difference: dashboard = monitoring, investigation = exploration

## Related Docs

- `SEO_AUDIT_ENGINE_SPEC.md` — the disaggregated audit (what the agent uses as data sources)
- `TOOLS.md` in `know` repo — reference for the tool catalog pattern
- `engine/tools/keywords.rs` — existing Rig tool implementation pattern to follow
- `engine/exec/research/mod.rs` — existing `exec_keyword_research_with_tools()` that attaches tools to an agent
