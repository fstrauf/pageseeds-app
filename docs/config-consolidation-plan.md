# Config File Consolidation Plan

Merge `project_summary.md` + `brandvoice.md` + `seo_content_brief.md` → single `project.md`.  
Keep `reddit_config.md` as channel-specific config (strip duplicated product info).  
Delete dead duplicate `_reply_standards.md`.

---

## Part 1: Code Changes (pageseeds-app)

All changes support both old filenames (fallback) and new `project.md` so migration can happen project-by-project.

### 1.1 `src-tauri/src/engine/exec/reddit.rs`

**`exec_reddit_config_parse()` (~line 199–207)**  
Currently reads 3 files: `reddit_config.md`, `project_summary.md`, `brandvoice.md`.

Change to:
```rust
// Primary: project.md (consolidated). Fallback: legacy files.
let project_context = std::fs::read_to_string(automation_dir.join("project.md"))
    .or_else(|_| {
        // Legacy fallback: stitch old files together
        let summary = std::fs::read_to_string(automation_dir.join("project_summary.md")).unwrap_or_default();
        let brand = std::fs::read_to_string(automation_dir.join("brandvoice.md")).unwrap_or_default();
        let brief = std::fs::read_to_string(automation_dir.join("seo_content_brief.md")).unwrap_or_default();
        Ok::<String, std::io::Error>(format!("{}\n\n{}\n\n{}", summary, brand, brief))
    })
    .unwrap_or_default();
let reddit_config = std::fs::read_to_string(automation_dir.join("reddit_config.md"))
    .unwrap_or_default();
```

Update the prompt template to send `{project_context}` + `{reddit_config}` (2 blocks instead of 3).

**`exec_reddit_enrich()` (~line 715–720)**  
Same pattern: read `project.md` with fallback to old files. Remove separate `brandvoice.md` read.

### 1.2 `src-tauri/src/engine/exec/keywords.rs`

**`derive_themes_from_project()` (~line 810–840)**  
Currently: priority `seo_content_brief.md` → `project_summary.md` → `articles.json`.

Change to:
```
priority: project.md → seo_content_brief.md (legacy) → project_summary.md (legacy) → articles.json
```

The `extract_from_brief()` and `extract_from_summary()` parsers stay — they just also try to parse sections from the consolidated `project.md`.

### 1.3 `src-tauri/src/engine/exec/research.rs`

**`build_research_prompts()` → `research_seed_extraction` (~line 137)**  
Currently: `find_file(&paths.automation_dir, "seo_content_brief.md")`.

Change to:
```rust
let brief_content = std::fs::read_to_string(paths.automation_dir.join("project.md"))
    .or_else(|_| find_file(&paths.automation_dir, "seo_content_brief")
        .and_then(|p| std::fs::read_to_string(&p).ok())
        .ok_or(std::io::Error::new(std::io::ErrorKind::NotFound, "")))
    .unwrap_or_else(|_| "(no brief found)".to_string());
```

### 1.4 `src-tauri/src/engine/executor.rs` (~line 907)

Currently writes output to `seo_content_brief.md`.

Change to: write to `project.md` if it exists (update the `## Content Clusters` section in place), otherwise write `seo_content_brief.md` for legacy compat.

### 1.5 `src-tauri/src/reddit/config.rs`

**`required_config_files()` (~line 55–62)**  
Change from:
```rust
vec![
    automation_dir.join("project_summary.md"),
    automation_dir.join("reddit_config.md"),
    automation_dir.join("brandvoice.md"),
    automation_dir.join("reddit").join("_reply_guardrails.md"),
]
```
To:
```rust
vec![
    automation_dir.join("project.md"),          // consolidated
    automation_dir.join("reddit_config.md"),
    automation_dir.join("reddit").join("_reply_guardrails.md"),
]
```

Add `missing_config_files()` fallback logic: if `project.md` missing, check legacy files before reporting missing.

### 1.6 `src-tauri/src/engine/setup_check.rs` (~lines 204–320)

Replace three separate file checks (`seo_content_brief`, `project_summary`, `brandvoice`) with one `project.md` check. Add a "legacy files detected" advisory detail when old files exist but `project.md` doesn't.

### 1.7 `src-tauri/src/commands/reddit.rs` (~line 201)

`_reply_guardrails.md` read stays unchanged (that file keeps its current path).

### 1.8 Tests

Update fixtures in:
- `src-tauri/src/engine/exec/reddit_test.rs` — create `project.md` instead of 3 files
- `src-tauri/src/engine/exec/keywords.rs` — test fixtures in `#[cfg(test)]` blocks
- `scripts/test_kimi_*.sh` — update `$AUTOMATION_DIR` file reads

### 1.9 Docs

Update `AGENTS.md` directory map to show `project.md` instead of 3 files.

---

## Part 2: Project Migrations

### Target structure (per project)

```
.github/automation/
├── project.md              # NEW — consolidated from 3 files
├── reddit_config.md        # KEPT — channel-specific only (dupes stripped)
├── reddit/
│   └── _reply_guardrails.md  # KEPT — unchanged
├── manifest.json           # KEPT
├── articles.json           # KEPT
├── task_list.json          # KEPT
└── (other .json files)     # KEPT
```

Files to delete after migration:
- `project_summary.md`
- `brandvoice.md`
- `seo_content_brief.md` (content moves into `project.md`)
- `_reply_standards.md` (exact duplicate of `_reply_guardrails.md` in every project)
- Any variant names: `coffee.md`, `coffee_seo_content_brief.md`

### `project.md` template

```markdown
# {Project Name}

## Identity

- **URL:** {url}
- **Description:** {1-2 sentence description}

### Key Differentiators
- {bullet list — single source, not repeated in reddit_config}

### Search Keywords
- {merged + deduplicated from project_summary + reddit_config query keywords}

## Brand Voice

{current brandvoice.md content — tone, voice characteristics, language style, what to avoid}

## Content Clusters & Status

{current seo_content_brief.md content — clusters, status, gaps}
```

### `reddit_config.md` (slimmed)

Remove duplicated sections. New structure:

```markdown
# Reddit Config: {Project Name}

> Full project context: see `project.md` in this directory

## Mention Stance
{REQUIRED|RECOMMENDED|OPTIONAL|OMIT}

## Trigger Topics
{list}

## Target Subreddits
{list}

## Excluded Subreddits
{list}

## Query Keywords
{list — Reddit-specific search terms only}
```

Removed: `## Product Information`, `## Key differentiators` (now in `project.md`).

---

### Per-Project Migration Status

#### 1. Days to Expiry (`call-analyzer`)

| Old File | Status | Action |
|---|---|---|
| `project_summary.md` | exists | Merge into `project.md` § Identity + § Key Differentiators + § Search Keywords |
| `brandvoice.md` | exists | Merge into `project.md` § Brand Voice |
| `seo_content_brief.md` | exists | Merge into `project.md` § Content Clusters |
| `reddit_config.md` | exists | Strip duplicated product info, add `> See project.md` pointer |
| `reddit/_reply_standards.md` | exists, exact dupe of `_reply_guardrails.md` | Delete |
| `reddit/search_test_adapter.md` | exists | Keep (test fixture) |

#### 2. Brewedlate (`nz-coffee-hub`)

| Old File | Status | Action |
|---|---|---|
| `coffee.md` | exists (variant name for project summary) | Merge into `project.md` § Identity |
| `brandvoice.md` | exists | Merge into `project.md` § Brand Voice |
| `coffee_seo_content_brief.md` | exists (variant name) | Merge into `project.md` § Content Clusters |
| `reddit_config.md` | exists | Strip duplicated product info |
| `reddit/_reply_standards.md` | exists, exact dupe | Delete |

**Note:** No `project_summary.md` — this project used `coffee.md` as the summary. The variant filename is why `find_file_by_suffix()` exists in the codebase.

#### 3. Expense Sorted (`tx/txApp`)

| Old File | Status | Action |
|---|---|---|
| `project_summary.md` | exists | Merge into `project.md` |
| `brandvoice.md` | exists | Merge into `project.md` |
| `seo_content_brief.md` | exists | Merge into `project.md` |
| `reddit_config.md` | exists | Strip duplicated product info |
| `reddit/_reply_guardrails.md` | exists | Keep |

#### 4. Learned Late (`learnedlate`)

| Old File | Status | Action |
|---|---|---|
| `project_summary.md` | exists | Merge into `project.md` |
| `brandvoice.md` | exists | Merge into `project.md` |
| `seo_content_brief.md` | exists | Merge into `project.md` |
| `reddit_config.md` | exists | Strip duplicated product info |
| `reddit/` | exists (empty) | Keep dir |

#### 5. Pageseeds (`pageseeds`)

| Old File | Status | Action |
|---|---|---|
| `project_summary.md` | exists | Merge into `project.md` |
| `brandvoice.md` | exists | Merge into `project.md` |
| `seo_content_brief.md` | exists | Merge into `project.md` |
| `reddit_config.md` | exists | Strip duplicated product info |
| `reddit/` | exists (empty) | Keep dir |

#### 6. Supplylah (`bigPond`)

| Old File | Status | Action |
|---|---|---|
| No `.md` files | — | Create minimal `project.md` from `manifest.json` data |
| No `reddit_config.md` | — | Skip Reddit config |
| `reddit/` | exists (empty) | Keep dir |

**This is the lightest migration — just create a starter `project.md`.**

---

## Execution Order

1. **Code changes first** — implement fallback logic so both old and new filenames work
2. **`cargo check`** — verify compilation  
3. **Update tests** — make them pass with new filenames
4. **Migrate projects one at a time** — create `project.md`, verify in UI, then delete old files
5. **Remove fallback logic** — once all projects are migrated, strip the legacy `or_else` branches (optional, low priority)

---

## Risk Notes

- The keyword workflow currently *writes* `seo_content_brief.md` as output. After migration, it needs to update the `## Content Clusters` section inside `project.md` instead. This is the trickiest change — needs section-level replacement rather than full file overwrite.
- `nz-coffee-hub` uses non-standard names (`coffee.md`, `coffee_seo_content_brief.md`). The `find_file_by_suffix()` helper handles this today. After migration to `project.md`, those variant names go away.
- `_reply_guardrails.md` stays in `reddit/` subdir — it's cross-project boilerplate, separate concern from project identity.
