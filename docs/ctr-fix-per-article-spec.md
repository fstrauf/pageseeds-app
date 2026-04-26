# CTR Fix: Per-Article Deterministic Pipeline — Implementation Spec

**Status:** Ready for implementation  
**Branch:** `seo-improvement-workflows`  
**Replaces:** Batch `fix_title_meta` / `fix_faq_schema` / `fix_snippet_bait` tasks  
**Goal:** One `fix_ctr_article` task per article. Agent produces structured JSON patches; Rust applies them deterministically; Rust verifies them deterministically.

---

## 1. Problem Statement

The current batch-by-fix-type model creates up to 3 tasks per audit (`fix_title_meta`, `fix_faq_schema`, `fix_snippet_bait`), each containing multiple articles. This causes:

- **Partial failure ambiguity:** One bad article poisons the whole batch.
- **No verification:** The agent reports what it changed, but the system never re-runs health checks.
- **No file integrity guard:** Agents occasionally corrupt MDX frontmatter. There is no snapshot/restore.
- **Misaligned thresholds:** Health checker says meta ≥ 50 chars is OK; skill says 140–155. The agent undershoots and verifier has no teeth.
- **Duplicate field mess:** Some files have both `description:` and `metaDescription:`. The deterministic replacer must canonicalize.

The DB already contains failed `fix_ctr_article` tasks from a transient dev build. The source code for that build was lost, but the failure patterns are known:
- `file_integrity_failed: Frontmatter section suspiciously long (>5000 chars)` — false positive on files with large inline FAQ YAML blocks (legitimately 13K–23K chars).
- `CTR verification complete: 1 fixed, 1 still failing` — meta description 116 chars, verifier requires 130–155.

---

## 2. Target Architecture

```
ctr_audit workflow
├── ctr_build_context    (deterministic — already exists)
├── ctr_analyze          (agentic — already exists)
└── create_ctr_fix_tasks (NEW — per-article, replaces batch logic)

fix_ctr_article workflow  (NEW task type + handler)
├── fix_ctr_article_generate  (agentic, skill="ctr-fix-apply")
│   └── Agent reads file + recommendations, returns CtrFixPatch JSON
├── fix_ctr_article_apply     (deterministic, NEW StepKind::CtrFixApply)
│   └── Rust applies patch: title, description, first_paragraph, FAQ schema
│   └── Snapshot original → apply → validate → restore on corruption
└── fix_ctr_article_verify    (deterministic, NEW StepKind::CtrVerifyFix)
    └── Rust re-runs health checks, produces CtrVerificationReport
```

### Task lifecycle

| Status | Meaning |
|--------|---------|
| `todo` | Task created, waiting for queue |
| `in_progress` | Running generate → apply → verify |
| `done` | All fixes verified |
| `review` | Verification found remaining issues (soft failure, retryable) |
| `failed` | File integrity failed (restored from snapshot) or agent returned unparseable patch |

### Queue semantics

- A `fix_ctr_article` task failing with `review` (threshold miss) **does NOT halt the queue**.
- A `fix_ctr_article` task failing with `failed` (integrity corruption, missing file) **does NOT halt the queue**.
- The queue halts only for **system failures** (DB error, skill missing, unhandled panic).

---

## 3. Data Models

### `models/ctr.rs` (already exists — wire it in)

```rust
pub struct CtrFixPatch {
    pub article_id: i64,
    pub file: String,
    pub error: Option<String>,
    pub changes: CtrFixPatchChanges,
}

pub struct CtrFixPatchChanges {
    pub title: Option<String>,
    pub description: Option<String>,
    pub first_paragraph: Option<String>,
    pub faq_questions: Option<Vec<CtrFixPatchFaqQuestion>>,
}

pub struct CtrFixPatchFaqQuestion {
    pub question: String,
    pub answer: String,
}
```

Add `#[ts(export)]` so TypeScript bindings are auto-generated.

### `models/ctr.rs` — NEW verification report

```rust
pub struct CtrFixVerificationReport {
    pub article_id: i64,
    pub file: String,
    pub overall_status: String, // "verified" | "partial" | "failed"
    pub checks: Vec<CtrFixCheckResult>,
}

pub struct CtrFixCheckResult {
    pub check_type: String, // "title" | "description" | "snippet" | "faq"
    pub status: String,     // "pass" | "fail" | "skip"
    pub expected: String,
    pub actual: String,
    pub detail: Option<String>,
}
```

---

## 4. File Changes

### 4.1 `engine/workflows/step_kind.rs`

Add two new step kinds:

```rust
/// Deterministic application of agent-generated CTR fix patch.
CtrFixApply,
/// Deterministic verification that applied CTR fixes meet health thresholds.
CtrVerifyFix,
```

Add string mappings:
- `"ctr_fix_apply"` → `CtrFixApply`
- `"ctr_verify_fix"` → `CtrVerifyFix`

### 4.2 `engine/workflows/handlers.rs`

In `ImplementationHandler::plan`, add:

```rust
"fix_ctr_article" => vec![
    WorkflowStep::new("fix_ctr_article_generate", StepKind::Agentic.as_ref())
        .with_param(step_params::SKILL, "ctr-fix-apply"),
    WorkflowStep::new("fix_ctr_article_apply", StepKind::CtrFixApply.as_ref()),
    WorkflowStep::new("fix_ctr_article_verify", StepKind::CtrVerifyFix.as_ref()),
],
```

Remove the old batch task types (`fix_title_meta`, `fix_faq_schema`, `fix_snippet_bait`) if they were ever explicitly listed. They currently fall through to the generic `fix_*` catch-all; leave that as a safety net for any in-flight tasks.

### 4.3 `engine/step_registry.rs`

Register two new handlers:

```rust
handlers.insert(StepKind::CtrFixApply, Box::new(|_step, ctx| {
    let task = ctx.task.clone();
    let project_path = ctx.project_path.to_string();
    let latest_raw = ctx.latest_raw.map(|s| s.to_string());
    Box::pin(async move {
        tokio::task::spawn_blocking(move || {
            crate::engine::exec::ctr_audit::exec_ctr_fix_apply(
                &task, &project_path, latest_raw.as_deref(),
            )
        })
        .await
        .unwrap_or_else(|e| StepResult {
            success: false,
            message: format!("Step panicked: {}", e),
            output: None,
        })
    })
}));

handlers.insert(StepKind::CtrVerifyFix, Box::new(|_step, ctx| {
    let task = ctx.task.clone();
    let project_path = ctx.project_path.to_string();
    Box::pin(async move {
        tokio::task::spawn_blocking(move || {
            crate::engine::exec::ctr_audit::exec_ctr_verify_fix(&task, &project_path)
        })
        .await
        .unwrap_or_else(|e| StepResult {
            success: false,
            message: format!("Step panicked: {}", e),
            output: None,
        })
    })
}));
```

### 4.4 `engine/exec/ctr_audit.rs` — NEW functions

#### `exec_ctr_fix_apply`

```rust
pub(crate) fn exec_ctr_fix_apply(
    task: &Task,
    project_path: &str,
    latest_raw: Option<&str>,
) -> StepResult {
    // 1. Parse CtrFixPatch from latest_raw (agent output)
    // 2. Resolve absolute file path from project_path + patch.file
    // 3. Read original file content
    // 4. Snapshot to {file}.backup
    // 5. parse_frontmatter → (fm, body)
    // 6. Apply changes deterministically:
    //    - title → replace_frontmatter_field(fm, "title", new_value)
    //    - description → replace_frontmatter_field(fm, "description", new_value)
    //      ALSO remove any `metaDescription:` or `meta_description:` aliases
    //    - first_paragraph → replace_first_paragraph(body, new_value)
    //    - faq_questions → insert_faq_schema(body, questions)
    // 7. rebuild_mdx(new_fm, new_body) → write file
    // 8. validate_mdx_structure(new_content) → if fail, restore snapshot, return failed
    // 9. Return success with summary
}
```

**Error handling:**
- Agent output is not valid JSON → `failed`, message: "Agent did not return valid CtrFixPatch JSON"
- File not found → `failed`, message: "File not found: {path}"
- `validate_mdx_structure` fails → restore from `.backup`, return `failed` with integrity message
- Partial application (e.g., only title changed, description missing) → still `success` at apply stage; verifier catches the rest

#### `exec_ctr_verify_fix`

```rust
pub(crate) fn exec_ctr_verify_fix(
    task: &Task,
    project_path: &str,
) -> StepResult {
    // 1. Extract article info from task artifacts
    //    - Read the ctr_recommendations artifact to get article_id, file, target_keyword
    // 2. Read the CURRENT file from disk (post-apply)
    // 3. Run read_article_excerpt → (title, meta, first_paragraph, h1, has_faq, file_found)
    // 4. Run check_article_health with the SAME thresholds as the audit
    // 5. Compare against the fixes that were requested:
    //    - If fix requested title_rewrite → check title length ≤ 55
    //    - If fix requested meta_description → check meta length 130–155
    //    - If fix requested snippet_bait → check first paragraph 40–60 words + keyword
    //    - If fix requested faq_schema → check has_faq_schema
    // 6. Build CtrFixVerificationReport
    // 7. If ALL requested fixes pass → success, status done
    //    If SOME fail → success=false (soft), message includes per-fix detail, status review
    //    If file missing → failed
}
```

**Threshold alignment:** Use the SAME constants as `audit_health.rs`:
- `TITLE_MAX_LEN = 55`
- `META_MIN_LEN = 130`
- `META_MAX_LEN = 155`
- `SNIPPET_MIN_WORDS = 40`
- `SNIPPET_MAX_WORDS = 60`

### 4.5 `engine/exec/ctr_audit.rs` — MODIFY `create_ctr_fix_tasks`

Replace the batch logic with per-article task creation:

```rust
pub(crate) fn create_ctr_fix_tasks(
    conn: &Connection,
    parent_task: &Task,
    project_path: &str,
) -> Vec<String> {
    // 1. Parse recommendations (same as before)
    // 2. Group by article_id (instead of by fix_type)
    // 3. For each article with at least one fix:
    //    - Build a single-recommendation artifact
    //    - Idempotency key: "ctr_fix:article:{project_id}:{article_id}:{parent_task_id}"
    //    - Task type: "fix_ctr_article"
    //    - Title: "CTR fix: {url_slug}"
    //    - Description: "Apply CTR fixes to article {article_id} ({url_slug})"
    //    - depends_on: [parent_task.id]
    //    - execution_mode: Automatic
    // 4. Spawn each task via TaskSpawner
}
```

**Idempotency:** Using `article_id` + `parent_task_id` means re-running the same audit won't create duplicates, but a new audit will create new fix tasks for still-unhealthy articles.

### 4.6 `content/cleaner.rs` — NEW deterministic editing utilities

```rust
/// Replace a frontmatter field value. Returns new frontmatter string.
/// Handles quoted and unquoted values. Preserves field order.
/// If field doesn't exist, inserts it after "title" if present.
pub fn replace_frontmatter_field(fm: &str, key: &str, value: &str) -> String;

/// Find byte range of first paragraph in MDX body (after H1, before first blank line or heading).
pub fn find_first_paragraph_range(body: &str) -> Option<(usize, usize)>;

/// Replace first paragraph with new text.
pub fn replace_first_paragraph(body: &str, new_text: &str) -> String;

/// Insert JSON-LD FAQPage schema before last `---` separator or at end of body.
pub fn insert_faq_schema(body: &str, questions: &[FaqQuestion]) -> String;

/// Reconstruct MDX file from frontmatter and body.
pub fn rebuild_mdx(fm: &str, body: &str) -> String;

/// Validate MDX structure after edits. Returns Ok(()) or descriptive error.
/// Checks:
/// - Starts with ---\n
/// - Has closing \n---\n
/// - Body is not empty
/// - Frontmatter length is NOT checked (removed — large inline FAQ YAML is legitimate)
pub fn validate_mdx_structure(content: &str) -> Result<(), String>;
```

**Frontmatter length check:** REMOVE the 5000-char threshold. It produces false positives on files with large inline FAQ YAML arrays. The integrity check should only verify structural validity (delimiters exist, body not empty).

**Duplicate meta field handling:** In `replace_frontmatter_field` for `description`, also scan for and remove `metaDescription:` and `meta_description:` aliases.

### 4.7 `engine/exec/audit_health.rs` — ALIGN thresholds

Update `check_article_health` to use strict SERP-ready thresholds:

```rust
pub const TITLE_MAX_LEN: usize = 55;
pub const META_MIN_LEN: usize = 130;
pub const META_MAX_LEN: usize = 155;
pub const SNIPPET_MIN_WORDS: usize = 40;
pub const SNIPPET_MAX_WORDS: usize = 60;
```

Update the health checker to use these constants instead of hard-coded values.

### 4.8 `models/mod.rs`

Add:
```rust
pub mod ctr;
```

### 4.9 Skills

#### `.github/skills/ctr-fix-apply/SKILL.md` (app-level fallback: `src-tauri/skills/ctr-fix-apply/SKILL.md`)

Update the meta description instruction to be crystal clear:

```markdown
### `meta_description`
- Return the new meta description text in `changes.description`.
- **Hard limits: 130–155 characters.** Minimum 130, maximum 155.
- **Aim for 145–150 characters.** Undershooting 130 is a verification failure.
- Count characters carefully. Do not return descriptions under 130 chars.
```

Also update `title_rewrite`:
```markdown
### `title_rewrite`
- Return the new title text in `changes.title`.
- Hard limit: 55 characters max (not 60).
```

And `snippet_bait`:
```markdown
### `snippet_bait`
- Return the new first paragraph text in `changes.first_paragraph`.
- Hard limits: 40–60 words (minimum 40, maximum 60).
```

---

## 5. Implementation Order

### Phase 1: Foundation (no behavioral change yet)
1. Add `CtrFixApply` and `CtrVerifyFix` to `step_kind.rs`
2. Register handlers in `step_registry.rs`
3. Add `pub mod ctr` to `models/mod.rs`
4. Add `replace_frontmatter_field`, `replace_first_paragraph`, `insert_faq_schema`, `rebuild_mdx`, `validate_mdx_structure` to `content/cleaner.rs`
5. Align `audit_health.rs` thresholds to constants
6. Update both `ctr-fix-apply/SKILL.md` files with strict length guidance
7. `cargo check` — must pass before Phase 2

### Phase 2: Task creation + workflow handler
8. Rewrite `create_ctr_fix_tasks` to create per-article `fix_ctr_article` tasks
9. Add `fix_ctr_article` case to `ImplementationHandler::plan` in `handlers.rs`
10. `cargo check` + run existing tests

### Phase 3: Apply + verify executors
11. Implement `exec_ctr_fix_apply` in `ctr_audit.rs`
12. Implement `exec_ctr_verify_fix` in `ctr_audit.rs`
13. Add tests for `exec_ctr_fix_apply` (happy path, corrupted file restore, missing file)
14. Add tests for `exec_ctr_verify_fix` (all pass, partial fail, missing file)
15. Run full test suite: `cargo test`

### Phase 4: Frontend bindings
16. Run `./scripts/sync-bindings.sh` to generate TypeScript for new models
17. Verify `src/lib/types.ts` re-exports new CTR types
18. `pnpm exec tsc -b` — must pass

---

## 6. Testing Strategy

### Rust unit tests

| Test | File | What it checks |
|------|------|----------------|
| `replace_frontmatter_field_basic` | `content/cleaner.rs` | Replaces existing quoted field |
| `replace_frontmatter_field_insert` | `content/cleaner.rs` | Inserts missing field after title |
| `replace_frontmatter_field_canonicalizes_meta` | `content/cleaner.rs` | Replaces `description:` AND removes `metaDescription:` |
| `replace_first_paragraph` | `content/cleaner.rs` | Finds and replaces first text block after H1 |
| `insert_faq_schema` | `content/cleaner.rs` | Appends JSON-LD before closing separator |
| `validate_mdx_structure_pass` | `content/cleaner.rs` | Accepts 23K frontmatter |
| `validate_mdx_structure_missing_close` | `content/cleaner.rs` | Rejects malformed frontmatter |
| `exec_ctr_fix_apply_success` | `ctr_audit.rs` | Full apply pipeline with snapshot |
| `exec_ctr_fix_apply_corrupt_restore` | `ctr_audit.rs` | Restore on validation failure |
| `exec_ctr_fix_apply_missing_file` | `ctr_audit.rs` | Graceful file-not-found |
| `exec_ctr_verify_fix_all_pass` | `ctr_audit.rs` | Returns verified status |
| `exec_ctr_verify_fix_partial_fail` | `ctr_audit.rs` | Returns review status with detail |
| `create_ctr_fix_tasks_per_article` | `ctr_audit.rs` | Creates N tasks for N articles |

### Integration test

1. Create a temp project with 2 articles.
2. Run `ctr_audit` → produces recommendations.
3. `create_ctr_fix_tasks` → creates 2 `fix_ctr_article` tasks.
4. Simulate agent output (valid patch) for task 1.
5. Run `fix_ctr_article_apply` → file modified, backup exists.
6. Run `fix_ctr_article_verify` → status verified.
7. Simulate agent output (meta too short) for task 2.
8. Run `fix_ctr_article_apply` → file modified.
9. Run `fix_ctr_article_verify` → status review, detail: "meta is 116 chars, expected 130–155".

---

## 7. Migration Notes

### In-flight tasks

Existing tasks of type `fix_title_meta`, `fix_faq_schema`, `fix_snippet_bait` will continue to work because `ImplementationHandler` catches all `fix_*` tasks. They just won't get the new deterministic apply/verify behavior.

### DB tasks

The stale `fix_ctr_article` tasks in the DB from the lost dev build can be ignored or manually deleted. The new code uses the same task type name but a different idempotency key format, so there is no collision.

---

## 8. Acceptance Criteria

- [ ] `cargo test` passes (including new tests).
- [ ] `cargo check` passes.
- [ ] `pnpm run lint` passes.
- [ ] `pnpm exec tsc -b` passes.
- [ ] `create_ctr_fix_tasks` creates exactly one `fix_ctr_article` task per article with issues.
- [ ] Agent skill instructs 130–155 char meta, 55 char title, 40–60 word snippet.
- [ ] `exec_ctr_fix_apply` snapshots, applies, validates, and restores on corruption.
- [ ] `validate_mdx_structure` does NOT reject frontmatter >5000 chars.
- [ ] `replace_frontmatter_field` canonicalizes `metaDescription` → `description`.
- [ ] `exec_ctr_verify_fix` returns per-fix detail; task status is `review` for threshold misses, `done` for all pass.
- [ ] Queue continues past `fix_ctr_article` failures.
