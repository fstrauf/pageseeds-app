# src-tauri (desktop shell) — temporarily non-building

**Status (post-#183):** Domain source of truth was **deleted** from this tree.
There is no dual SoT under `src-tauri/src/` anymore.

| Path | Role |
|------|------|
| `crates/pageseeds-core` | **SoT** domain library (`pageseeds_core`) — **no Tauri** |
| `crates/pageseeds-cli` | **SoT** operator CLI bin |
| `src-tauri/` | Desktop shell only — **non-building until #184** |

## What remains here

Shell-only artifacts kept for the future desktop rebuild:

- `src/commands/` — `#[tauri::command]` IPC bindings (not wired; do not treat as SoT)
- `src/lib.rs`, `src/main.rs` — stubs that document breakage (`compile_error!` pending #184)
- `build.rs`, `tauri.conf.json`, `icons/`, `capabilities/`, `Cargo.toml`

## What was removed

Moved domain (engine, content, db, models, seo, reddit, social, rig, clarity, gsc,
license, live_site, cannibalization, config, logging, prompts, skills, error,
test_support), embedded `skills/`, `config/tool_catalog.toml`, CLI/smoke bins under
`src/bin/`, and `examples/` — all live under `crates/pageseeds-core` /
`crates/pageseeds-cli` (or were obsolete after the split).

**Do not** re-add complete copies of engine/content/db under `src-tauri`.
**Do not** edit domain logic here. Edit under `crates/pageseeds-core/`.

```bash
# Operator / domain gates (from repo root):
cargo test -p pageseeds-core
cargo build -p pageseeds-cli
pnpm run test:cli
```

Issue **#184** will rewire this shell to depend on `pageseeds-core` and restore the
desktop build. Until then, this package is intentionally **not** a workspace member.
