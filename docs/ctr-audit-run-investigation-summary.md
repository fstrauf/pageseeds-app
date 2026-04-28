# CTR Audit Run Investigation Summary

Date: 2026-04-29  
Branch: `seo-improvement-workflows`  
Run: 76-task queue (1 schema renderer + ~70 fix_ctr_article tasks)

---

## 1. Schema Renderer Task — 500 Internal Server Error / Timeout

### Symptom
`fix_ctr_schema_renderer` task fails immediately:
```
Agentic step 'ctr_schema_plan' failed: Kimi bridge prompt failed:
Kimi API error 500 Internal Server Error
{"error":{"message":"Internal server error","type":"internal_error"}}
```

### Investigation
- Bridge log shows prompt size: **75,307 bytes** (75KB)
- Bridge timeout: 300 seconds — the Kimi CLI subprocess hangs processing the massive prompt
- Root cause: `exec_agentic` in `handlers.rs` injects the **full untruncated** `ctr_schema_detection` artifact into the prompt
- The artifact contains **115 articles**, each with URL, file path, and FAQ state — ~30-40KB of JSON
- `prompts.rs::build_task_section` *does* truncate artifacts to 500 chars, but `handlers.rs::exec_agentic` **duplicates** them with full content afterward

### Why It Matters
The agent doesn't need 115 articles to propose a schema renderer fix. It needs 2-3 representative examples to understand the pattern. The oversized prompt kills the bridge before the agent even starts reasoning.

### Fix Needed
Cap `ctr_schema_detection` output to a representative sample (e.g., 5 articles) in `template.rs`.

---

## 2. CTR Fix Tasks — Pre-Write Validation Rejections

### Symptom
Multiple `fix_ctr_article` tasks fail with:
```
Agent returned invalid CtrFixPatch values: description is 127 chars, expected 130-155.
No changes written.
```
```
Agent returned invalid CtrFixPatch values: first_paragraph is 32 words, expected 40-60.
No changes written.
```

### Investigation
- The `validate_patch_before_write()` function (added in latest commit) catches these **before** disk write
- The agent receives explicit instructions: "Hard limits: 130–155 characters" and "Hard limits: 40–60 words"
- **LLMs cannot reliably count characters or words.** They generate prose that *feels* approximately correct
- Examples from the run:
  - Description targeted ~145 chars → actual 127-129 chars (undershot by 1-3 chars)
  - First paragraph targeted ~50 words → actual 32-38 words (undershot by 2-8 words)

### Why It Matters
Strict validation protects file quality but creates a high failure rate. ~15-20% of CTR fix tasks may fail on length constraints alone. Each failure wastes 2 LLM calls (analyze + generate) with no file changes.

### Options
- **A)** Loosen bounds (e.g., description 120-160, first paragraph 35-65)
- **B)** Auto-truncate/pad deterministically in `apply.rs`
- **C)** Keep strict validation, accept manual re-run rate

---

## 3. CTR Fix Task — Verification Failure After Apply

### Symptom
`task-6100bc82` applies successfully but verification fails:
```
CTR verification found issues for ...: title is 90 chars, expected ≤ 55
```

### Investigation
- Traced the agent outputs:
  1. `ctr_analyze_single` recommended: `title_rewrite` (current title 90 chars)
  2. `fix_ctr_article_generate` returned: `"title": null` (decided to skip)
  3. `fix_ctr_article_apply` applied description only
  4. `fix_ctr_article_verify` checked title — still 90 chars → failed

This is an **agent inconsistency**: the analysis agent says "fix the title" but the generate agent says "skip it." They don't share state or enforce agreement.

### Why It Matters
The two-step agent pipeline (analyze → generate) is supposed to cascade recommendations, but the generate agent can override or ignore the analysis agent's fixes. This wastes the entire task pipeline.

### Fix Needed
The generate agent should be constrained to apply the fixes listed in `ctr_recommendations`, not re-evaluate them. Or the prompt should explicitly say "you MUST apply every fix listed — do not skip fixes."

---

## 4. Bridge Step Limit

### Symptom
`task-f2f470f5` fails with:
```
Agent did not return valid CtrFixPatch JSON — no JSON found
```
Agent output was literally: `"Max number of steps reached: 100"`

### Investigation
- Bridge log shows request duration: **242 seconds**
- The Kimi CLI (`kimi` binary) has an internal step counter (max 100 steps)
- The agent likely entered a long reasoning loop, consuming steps without producing output
- Prompt size was normal (~9KB), so this is an agent behavior issue, not a prompt size issue

### Why It Matters
The Kimi CLI backend is less predictable than API-based backends. Long-running prompts with complex instructions can trigger internal step limits.

---

## 5. FAQ Preservation — Working as Designed

### Good News
The Phase 1 FAQ preservation changes are active and effective:
- `has_frontmatter_faq()` correctly identifies articles with YAML `faq:`
- `task_spawner.rs` excludes `missing_faq_schema` when frontmatter FAQ exists
- `apply.rs` safety net skipped FAQ patches for files with existing frontmatter FAQ
- No rich FAQ content was overwritten with generic answers in this run

### One Edge Case
Some articles had inline JSON-LD from **previous** runs (before the fix). These still trigger `missing_faq_schema` because `has_faq_schema` returns true for JSON-LD, but `has_frontmatter_faq` returns false. The task spawner correctly spawns fix tasks for these — they need migration to frontmatter FAQ.

---

## Overall Run Health

| Metric | Count |
|---|---|
| Total tasks queued | 76 |
| Schema renderer | 1 (failed — timeout) |
| CTR fix tasks | ~70 |
| CTR outcome reviews | ~15 |
| Validation rejections (pre-write) | At least 3 observed |
| Verification failures (post-apply) | At least 1 observed |
| Bridge step limit | 1 observed |

**Success rate for CTR fix tasks**: ~80-85% (estimated from observed failures)

---

## Recommended Priority Order

1. **Fix schema renderer prompt size** — cap detection output to 5 articles
2. **Fix agent inconsistency** — constrain generate agent to apply all recommended fixes
3. **Adjust validation bounds** — loosen slightly or add deterministic trimming
4. **Monitor bridge step limits** — may need shorter prompts or fewer reasoning steps
