# CTR Typed Fix Generation Spec

Status: Draft  
Owner: TBD  
Created: 2026-04-29

## Summary

`fix_ctr_article_generate` currently asks an LLM for a structured `CtrFixPatch`, but it runs through the generic raw-text `StepKind::Agentic` path. The downstream apply step then extracts JSON from prose, parses it, validates it, and writes the MDX file if the patch is acceptable.

That is backwards for a structured-output workflow. The generation step should return a typed `CtrFixPatch` through Rig structured extraction, then the deterministic apply step should normalize and validate as the final safety boundary.

This spec promotes CTR fix generation from raw agent prose to a typed workflow step with schema-backed extraction, one repair attempt, strict validation, and a typed artifact handoff to apply.

## Problem

The current workflow has three separate failure modes that share the same root cause: a structured contract is enforced too late.

- The model sometimes wraps JSON in markdown or explanatory prose.
- The model returns valid JSON with values that miss hard limits by a few characters or words.
- The model can ignore or override the `ctr_recommendations` artifact, such as returning `title: null` even when `title_rewrite` was requested.

The apply step must continue to protect files from bad writes, but it should not be the first place the app learns whether the agent followed the output contract.

## Current Flow

1. `fix_ctr_article` workflow plans `ctr_analyze_single` as `StepKind::CtrAnalyze`.
2. `ctr_analyze_single` produces a single `CtrRecommendation` artifact.
3. `fix_ctr_article_generate` runs as generic `StepKind::Agentic` with skill `ctr-fix-apply`.
4. `exec_agentic` builds a prompt and calls `agent::run_agent`.
5. The raw model text becomes `latest_raw_output`.
6. `fix_ctr_article_apply` parses `latest_raw_output` into `CtrFixPatch`.
7. Apply normalizes small near misses, validates, then writes or fails without writing.

## Target Flow

1. `fix_ctr_article` workflow plans `ctr_analyze_single` as it does today.
2. `ctr_analyze_single` produces a single typed `CtrRecommendation` artifact.
3. `fix_ctr_article_generate` runs as a new CTR-specific step kind, `CtrFixGenerate`.
4. `CtrFixGenerate` builds a focused prompt from the recommendation and content excerpt.
5. `CtrFixGenerate` calls `rig::extraction::extract_structured::<CtrFixPatch>()`.
6. The typed patch is normalized and validated immediately.
7. If validation fails, the step performs one targeted repair extraction with the validation errors.
8. The final typed `CtrFixPatch` is stored as a `ctr_fix_patch` artifact and returned as JSON.
9. `CtrFixApply` reads the typed patch artifact first, falling back to `latest_raw_output` only for legacy compatibility.
10. `CtrFixApply` keeps deterministic normalization, validation, and no-write-on-invalid protection.

## Goals

- Make CTR fix generation schema-backed instead of prose-backed.
- Fail earlier with clear validation errors when the model cannot produce a valid patch.
- Preserve the deterministic apply boundary that prevents bad file writes.
- Reduce wasted queue runs caused by small length/count misses.
- Enforce that generated patches correspond to the requested fixes.
- Keep the change narrow to the `fix_ctr_article` workflow.

## Non-Goals

- Rewriting all agentic steps to use Rig extraction.
- Removing the generic `StepKind::Agentic` path.
- Loosening CTR health thresholds.
- Replacing the existing `ctr_analyze_single` recommendation step.
- Moving content editing into the model. Rust continues to apply edits deterministically.

## Design Decisions

### Keep Apply Validation

Structured extraction can enforce JSON shape, but it cannot reliably enforce prose quality, character counts, word counts, or target keyword presence. The final apply step must continue to validate before writing.

The current normalization in `exec_ctr_fix_apply` remains useful and should stay as the last line of defense.

### Add A Domain Step Instead Of Overloading Generic Agentic

`fix_ctr_article_generate` has a known input and output type. That makes it a poor fit for generic raw agent execution.

Add `StepKind::CtrFixGenerate` and a dedicated executor function, likely `exec_ctr_fix_generate`, under `src-tauri/src/engine/exec/ctr_audit/`.

### Store A Typed Artifact

The generation step should append a `TaskArtifact`:

```json
{
  "key": "ctr_fix_patch",
  "artifact_type": "json",
  "source": "ctr_fix_generate",
  "content": "{...CtrFixPatch...}"
}
```

`CtrFixApply` should prefer this artifact over raw `latest_raw_output`. This removes the hidden dependency on step ordering and raw text extraction.

### One Repair Attempt Only

If the first structured patch fails deterministic validation, the generation step should make one repair call. More than one retry risks long-running queue stalls and provider loops.

The repair prompt should include:

- the original recommendation
- the invalid patch
- the exact validation errors
- the instruction to return only corrected `CtrFixPatch`

If repair fails, the task should fail/review with a precise message and no file write.

## Implementation Plan

## Phase 0: Preserve The Current Guardrail

Status: mostly done.

Keep the deterministic patch normalization and validation in `exec_ctr_fix_apply`.

Acceptance:

- Near-miss title/meta/snippet values can be normalized before write.
- Clearly invalid patch values still fail before write.
- `cargo test --manifest-path src-tauri/Cargo.toml exec_ctr_fix_apply_` passes.

## Phase 1: Make CTR Patch Models Schema-Extractable

Files:

- `src-tauri/src/models/ctr.rs`
- `src-tauri/Cargo.toml`

Tasks:

1. Add `schemars::JsonSchema` derives to:
   - `CtrFixPatch`
   - `CtrFixPatchChanges`
   - `CtrFixPatchFaqQuestion`
   - `CtrSnippetPatch`
   - `CtrSnippetFormat`
2. Confirm `schemars = "1.0"` is already available in `src-tauri/Cargo.toml`.
3. Keep existing `serde` and `ts-rs` derives.

Acceptance:

- `cargo check` passes.
- Existing ts-rs binding export tests still pass.

## Phase 2: Extract Patch Validation Into A Reusable Helper

Files:

- `src-tauri/src/engine/exec/ctr_audit/apply.rs`
- optionally `src-tauri/src/engine/exec/ctr_audit/patch.rs`

Tasks:

1. Move or expose patch validation so both generate and apply can use it.
2. Return structured validation results rather than only formatted strings.
3. Keep the apply-step user message unchanged or very similar.

Suggested API:

```rust
pub(crate) struct CtrPatchValidation {
    pub errors: Vec<String>,
    pub repairs: Vec<String>,
}

pub(crate) fn normalize_patch_before_validation(
    patch: &mut CtrFixPatch,
    task: &Task,
) -> Vec<String>;

pub(crate) fn validate_patch_before_write(
    patch: &CtrFixPatch,
    task: &Task,
    original_content: &str,
) -> Vec<String>;
```

Acceptance:

- Apply behavior does not change.
- Existing CTR apply tests pass.
- New helper can be called from the generate step without reading or writing the file twice unnecessarily.

## Phase 3: Add `StepKind::CtrFixGenerate`

Files:

- `src-tauri/src/engine/workflows/step_kind.rs`
- `src-tauri/src/engine/workflows/handlers.rs`
- `src-tauri/src/engine/step_registry.rs`
- `src-tauri/src/engine/exec/ctr_audit/mod.rs`
- new `src-tauri/src/engine/exec/ctr_audit/generate.rs`

Tasks:

1. Add `CtrFixGenerate` to `StepKind`.
2. Add string mappings:
   - `as_str() -> "ctr_fix_generate"`
   - `FromStr` mapping from `"ctr_fix_generate"`
3. Add the variant to step-kind round-trip tests.
4. Update the `fix_ctr_article` workflow plan:

```rust
WorkflowStep::new("fix_ctr_article_generate", StepKind::CtrFixGenerate)
    .with_param(step_params::SKILL, "ctr-fix-apply")
```

5. Register the new handler in `StepRegistry`.

Acceptance:

- Workflow step-kind tests pass.
- `fix_ctr_article` no longer uses generic `StepKind::Agentic` for patch generation.

## Phase 4: Implement Typed CTR Fix Generation

Files:

- `src-tauri/src/engine/exec/ctr_audit/generate.rs`
- `src-tauri/src/engine/exec/ctr_audit/apply.rs`
- `src-tauri/src/engine/exec/audit_health.rs`
- `src-tauri/src/rig/extraction.rs` only if a small adapter is needed

Tasks:

1. Load the single `CtrRecommendation` from `task.artifacts["ctr_recommendations"]`.
2. Resolve and read the target MDX file.
3. Build a focused prompt containing:
   - skill content from `ctr-fix-apply`
   - the `CtrRecommendation`
   - current title, description, first paragraph, FAQ state, and relevant body excerpt
   - explicit instruction that every requested fix must be represented unless already satisfied by current file state
4. Call `extract_structured::<CtrFixPatch>()`.
5. Normalize and validate the patch.
6. If invalid, make exactly one repair extraction call.
7. Return success only with a valid typed patch JSON.

Pseudo-code:

```rust
pub(crate) async fn exec_ctr_fix_generate(
    task: &Task,
    project_path: &str,
    agent_provider: &str,
) -> StepResult {
    let rec = load_ctr_recommendation(task)?;
    let original_content = read_recommended_file(project_path, &rec.file)?;
    let prompt = build_ctr_fix_patch_prompt(&rec, &original_content)?;

    let mut patch = extract_structured::<CtrFixPatch>(
        agent_provider,
        &prompt,
        Some("Return only the CtrFixPatch by calling the submit tool."),
    ).await?;

    let repairs = normalize_patch_before_validation(&mut patch, task);
    let errors = validate_patch_before_write(&patch, task, &original_content);

    if !errors.is_empty() {
        patch = repair_ctr_fix_patch(agent_provider, &prompt, &patch, &errors).await?;
        normalize_patch_before_validation(&mut patch, task);
        let errors = validate_patch_before_write(&patch, task, &original_content);
        if !errors.is_empty() {
            return failed_no_write(errors);
        }
    }

    success_with_patch_json(patch, repairs)
}
```

Acceptance:

- A mocked provider/tool-call response returns a typed `CtrFixPatch`.
- Invalid first response plus valid repair response succeeds.
- Invalid first response plus invalid repair response fails without writing.
- No raw markdown/prose JSON extraction is needed in the generate step.

## Phase 5: Prefer The Typed Patch Artifact In Apply

Files:

- `src-tauri/src/engine/exec/ctr_audit/apply.rs`
- `src-tauri/src/engine/executor.rs` or `src-tauri/src/engine/post_actions.rs`

Tasks:

1. Ensure `CtrFixGenerate` output is appended as artifact key `ctr_fix_patch`.
2. Update `exec_ctr_fix_apply` to read `ctr_fix_patch` from task artifacts first.
3. Keep legacy fallback to `latest_raw_output` during transition.
4. Log when legacy fallback is used.

Acceptance:

- Apply succeeds using the typed artifact even if `latest_raw_output` is empty.
- Legacy raw output tests still pass until removed.

## Phase 6: Enforce Recommendation/Patch Consistency

Files:

- `src-tauri/src/engine/exec/ctr_audit/generate.rs`
- `src-tauri/src/engine/exec/ctr_audit/apply.rs` or shared patch validation module

Tasks:

1. Validate that `patch.article_id == recommendation.article_id`.
2. Validate that `patch.file == recommendation.file` or resolves to the same file.
3. Validate that each requested fix is represented:
   - `title_rewrite` requires `changes.title` unless current title already passes.
   - `meta_description` requires `changes.description` unless current description already passes.
   - `snippet_bait` requires `changes.first_paragraph` or `changes.snippet_patch` unless current snippet already passes.
   - `faq_schema` requires `changes.faq_questions` unless existing frontmatter FAQ should be preserved.
4. Treat unrequested changes as warnings or errors. Recommendation: error for title/meta/first paragraph changes not listed in fixes; allow FAQ preservation skips.

Acceptance:

- A patch that skips a requested broken title fails generation before apply.
- A patch for the wrong file or article ID fails generation before apply.

## Phase 7: Clean Up Prompt And Skill Boundaries

Files:

- `src-tauri/skills/ctr-fix-apply/SKILL.md`
- `.github/skills/ctr-fix-apply/SKILL.md` if project override should match
- `src-tauri/src/engine/exec/ctr_audit/generate.rs`

Tasks:

1. Move machine-contract details into Rust validation where possible.
2. Keep skill focused on writing useful replacement prose.
3. Add a short prompt section generated by Rust with exact current validation thresholds.
4. Ensure embedded app-level skill and project-level override are not unintentionally divergent.

Acceptance:

- Prompt contains current thresholds from constants, not duplicated magic numbers.
- Skill no longer has to carry every validation detail by hand.

## Testing Plan

### Unit Tests

Add or update tests for:

- `CtrFixPatch` schema extraction compatibility.
- `StepKind::CtrFixGenerate` string round trip.
- `exec_ctr_fix_generate` success with mocked structured extraction.
- one repair pass success.
- repair pass failure.
- recommendation/patch mismatch.
- apply reads `ctr_fix_patch` artifact before `latest_raw_output`.

### Integration Tests

- Run one `fix_ctr_article` workflow with mocked provider output.
- Confirm task artifacts include both `ctr_recommendations` and `ctr_fix_patch`.
- Confirm file writes only happen in `CtrFixApply`.
- Confirm verification still runs after apply.

### Commands

```bash
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml exec_ctr_fix_apply_
cargo test --manifest-path src-tauri/Cargo.toml ctr_fix_generate
cargo test --manifest-path src-tauri/Cargo.toml
```

If Rust models used by the frontend change, also run:

```bash
./scripts/sync-bindings.sh
./scripts/check-bindings.sh
pnpm exec tsc -b
```

## Rollout Plan

1. Ship Phase 0 guardrail first. This keeps live queues from failing on small count misses.
2. Add schema derives and shared validation with no workflow behavior change.
3. Add `CtrFixGenerate` behind the existing `fix_ctr_article` workflow.
4. Keep raw-output fallback in apply for one release cycle or until existing in-progress tasks are cleared.
5. Remove raw-output fallback once all queued `fix_ctr_article` tasks are generated through `CtrFixGenerate`.

## Risks

- Some providers or backend modes may not support structured extraction. `KimiDirect` is explicitly unsupported by `rig::extraction`.
- The Kimi bridge structured extraction path must stay healthy for this workflow to run automatically.
- Project-level skill overrides may diverge from embedded skill expectations.
- A repair pass can increase latency, so it must be capped at one attempt.

## Fallback Behavior

If structured extraction is unavailable:

- Preferred: fail `CtrFixGenerate` with a clear message telling the user to run Kimi bridge or select a structured-output provider.
- Temporary compatibility: allow generic `StepKind::Agentic` only behind an explicit feature flag or transition parameter.

Do not silently fall back to raw prose for this step once the typed path is active.

## Definition Of Done

- `fix_ctr_article_generate` no longer uses generic raw `StepKind::Agentic`.
- The generation step returns a typed `CtrFixPatch` via Rig structured extraction.
- Generated patches are normalized and validated before apply.
- Apply still validates and refuses bad writes.
- Requested fixes cannot be silently skipped by the generation step.
- The queue records clear failure messages for unsupported providers or invalid repaired patches.
- Full Rust tests pass.
