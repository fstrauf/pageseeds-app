# CLI release

How to ship **`pageseeds-cli`** from this repo. Desktop Mac app releases are discontinued.

---

## Version source of truth

Workspace package version in root **`Cargo.toml`**:

```toml
[workspace.package]
version = "0.1.0"
```

Do **not** invent a second version store. `package.json` may mirror the same semver for convenience; Cargo is authoritative for tags and assets.

---

## Release procedure

1. **Bump** `[workspace.package] version` in root `Cargo.toml` (and lockfile if needed).
2. **Commit** the version bump on `main` (or a release PR merged to `main`).
3. **Tag** matching Cargo: `cli-vX.Y.Z` (example: Cargo `0.1.0` → tag `cli-v0.1.0`).
4. **Push** the tag:

   ```bash
   git tag cli-v0.1.0
   git push origin cli-v0.1.0
   ```

5. **GitHub Actions** [`.github/workflows/release-cli.yml`](../.github/workflows/release-cli.yml) builds on `macos-14` and publishes a GitHub Release on **this** repo (`fstrauf/pageseeds-app`) with:
   - `pageseeds-cli-{semver}-aarch64-apple-darwin.tar.gz`
   - `pageseeds-cli-{semver}-aarch64-apple-darwin.tar.gz.sha256`

   The workflow **fails** if the tag semver does not match workspace Cargo version.

### Dry-run (draft release)

Use **Actions → Release CLI → Run workflow** (`workflow_dispatch`). Leave version empty to read Cargo, or pass a semver that matches Cargo. The job creates a **draft** release for inspection (no customer install impact until you publish a real `cli-v*` tag).

---

## Customer install (stable URL)

Install script URL is stable and must not change:

```bash
curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash
```

- Prebuilt: macOS Apple Silicon only (`aarch64-apple-darwin`).
- Optional pin: `VERSION=0.1.0 curl -fsSL ... | bash`
- Other platforms: `FROM_SOURCE=1` from a checkout (requires Rust).

Details: [CLI_GETTING_STARTED.md](./CLI_GETTING_STARTED.md).

---

## Desktop (discontinued)

- **No new** Tauri/React desktop builds from this repo (see #184).
- Historical Mac app channel was **`fstrauf/pageseeds-releases`** with tags `v*` (DMG updater feed). Local `latest.json` updater feed is **obsolete** and not shipped.
- Product development ship gate is **CLI only**, run locally: `pnpm run test:cli` / `pnpm test:all`.

---

## Related

| Path | Role |
|------|------|
| [`.github/workflows/release-cli.yml`](../.github/workflows/release-cli.yml) | Tag / dispatch release job |
| [`scripts/install-cli.sh`](../scripts/install-cli.sh) | Customer + contributor install |
| [CLI Getting Started](./CLI_GETTING_STARTED.md) | Operator install and first desk read |
