# Reddit Config: Replace Deterministic Parsing with Agentic Artifact Consumption

## Problem

The reddit workflow has an agentic config parse step (`reddit_config_parse_stage`) that correctly extracts `product_name`, `mention_stance`, and other fields from raw markdown via LLM. The output is stored as a task artifact in `RedditSearchParams` JSON format.

Step 2 (`reddit_search`) reads this artifact correctly. But two downstream consumers **ignore it** and re-parse the raw markdown deterministically via `reddit::config::parse_reddit_config()`:

1. **`exec_reddit_enrich`** (step 3 of the workflow) — re-reads `reddit_config.md` from disk, parses it with regex/string-matching heuristics
2. **`draft_reddit_reply`** (UI command for re-drafting a single reply) — same deterministic parse

The deterministic parser breaks on common markdown patterns:
- `**RECOMMENDED** - description` → bold markers + trailing text cause mention_stance to default to `Optional`
- Missing `## Product Name` section → product_name falls to `"the product"` even when the H1 title contains the name

This means the LLM drafting replies with `Product name: the product` and `Optional` stance — effectively removing the project name from replies.

## Current Flow

```
Step 1: reddit_config_parse (agentic)
  → reads project.md + reddit_config.md
  → LLM extracts: { product_name, mention_stance, trigger_topics, ... }
  → stored as artifact "reddit_config_parse_stage"

Step 2: reddit_search (deterministic)
  → reads artifact from step 1 ✅
  → calls Reddit API

Step 3: reddit_enrich (agentic)
  → IGNORES step 1 artifact ❌
  → re-reads reddit_config.md from disk
  → calls parse_reddit_config() deterministically
  → builds prompt with potentially wrong product_name/mention_stance

UI: draft_reddit_reply (command)
  → IGNORES any prior artifact ❌
  → re-reads reddit_config.md from disk
  → calls parse_reddit_config() deterministically
  → same problem
```

## Target Flow

```
Step 1: reddit_config_parse (agentic) — unchanged

Step 2: reddit_search (deterministic) — unchanged

Step 3: reddit_enrich (agentic)
  → reads product_name + mention_stance from step 1 artifact ✅
  → falls back to deterministic parse only if no artifact exists

UI: draft_reddit_reply (command)
  → reads product_name + mention_stance from the opportunity's DB row ✅
  → (mention_stance already stored by enrich step)
  → for product_name: read from most recent config parse artifact in DB
  → falls back to deterministic parse only if nothing in DB
```

## Changes

### 1. `exec_reddit_enrich` — Read artifact instead of re-parsing

**File:** `src-tauri/src/engine/exec/reddit.rs` (~line 735)

Currently loads config with:
```rust
let cfg = crate::reddit::config::parse_reddit_config(&reddit_config_raw);
let product_name = cfg.product_name.as_deref().unwrap_or("the product").to_string();
let mention_stance_str = cfg.mention_stance.as_str().to_string();
```

Change to:
- Accept `task: &Task` parameter (currently only receives `project_id`)
- Look for `reddit_config_parse_stage` in `task.artifacts`
- Deserialize as `RedditSearchParams`
- Use `params.product_name` and `params.mention_stance`
- Fall back to deterministic `parse_reddit_config()` only if no artifact found

This requires updating the calling signature in `executor.rs` where `exec_reddit_enrich` is called — pass `&task` instead of just `&task.project_id`.

### 2. `draft_reddit_reply` command — Use DB-stored values

**File:** `src-tauri/src/commands/reddit.rs` (~line 220)

The opportunity row already has `mention_stance` stored (written by the enrich step). For `product_name`, two options:

**Option A (simpler):** Store `product_name` on the opportunity row during enrichment. Add a `product_name` column to `reddit_opportunities`.

**Option B (no schema change):** Query the most recent `reddit_config_parse_stage` artifact for this project from the `task_artifacts` table.

**Recommended: Option A.** The product_name is already known at enrich time and storing it on the row is one UPDATE. This avoids a cross-table join and works even if the original task is deleted.

Schema migration (new `MIGRATION_VXX`):
```sql
ALTER TABLE reddit_opportunities ADD COLUMN product_name TEXT;
```

Then `draft_reddit_reply`:
- Read `opp.product_name` and `opp.mention_stance` from the DB row
- Pass directly to `build_draft_reply_prompt` (update prompt builder to accept strings instead of `&RedditProjectConfig`)
- Fall back to deterministic parse only on NULL values (pre-migration rows)

### 3. `build_draft_reply_prompt` — Accept product_name/stance directly

**File:** `src-tauri/src/reddit/prompts.rs`

Currently takes `&RedditProjectConfig` (the deterministic struct). Change to accept `product_name: &str` and `mention_stance: &str` directly, so callers don't need to construct the config struct.

### 4. Store product_name during enrichment

**File:** `src-tauri/src/engine/exec/reddit.rs` (enrich UPDATE statement)

Add `product_name` to the UPDATE that runs after enrichment, writing the value from the step 1 artifact.

### 5. Cleanup — remove previous parser fixes

**File:** `src-tauri/src/reddit/config.rs`

Revert the H1-title fallback and `normalize_stance_value` additions from the previous session. With agentic parsing as the primary path, these deterministic workarounds are unnecessary. Keep the base parser as a backward-compat fallback only — it doesn't need to handle every markdown edge case since it's no longer the primary path.

## Files Touched

| File | Change |
|------|--------|
| `src-tauri/src/engine/exec/reddit.rs` | Enrich reads artifact; stores product_name in DB |
| `src-tauri/src/engine/executor.rs` | Pass `&task` to `exec_reddit_enrich` |
| `src-tauri/src/commands/reddit.rs` | `draft_reddit_reply` reads from DB row |
| `src-tauri/src/reddit/prompts.rs` | Accept strings instead of config struct |
| `src-tauri/src/db/mod.rs` | New migration: add `product_name` column |
| `src-tauri/src/reddit/config.rs` | Revert unnecessary parser fixes |
| `src-tauri/src/models/reddit.rs` | Add `product_name` field to `RedditOpportunity` |

## Validation

1. `cargo check` passes
2. Run a reddit_opportunity_search task end-to-end → verify `product_name` appears in enriched reply_text
3. Use `draft_reddit_reply` UI button on an enriched opportunity → verify product name appears
4. Test with a pre-migration opportunity (no product_name in DB) → verify fallback works
