# src-tauri (desktop shell) — temporarily non-building

**Status:** Domain modules and the operator CLI moved to the workspace crates in **#183**:

| Path | Role |
|------|------|
| `crates/pageseeds-core` | Domain library (`pageseeds_core`) — **no Tauri** |
| `crates/pageseeds-cli` | Operator CLI bin |

This directory still holds the Tauri GUI shell (`commands/`, `lib.rs` `run()`, `main.rs`, `build.rs`, icons, `tauri.conf.json`). It is **intentionally not a workspace member** and may not build until **#184** rewires the desktop app to depend on `pageseeds-core`.

**Do not** edit domain logic here. Edit under `crates/pageseeds-core/`.  
**Do not** treat copies of domain source under `src-tauri/src/` as source of truth.

```bash
# Operator / domain gates (from repo root):
cargo test -p pageseeds-core
cargo build -p pageseeds-cli
pnpm run test:cli
```
