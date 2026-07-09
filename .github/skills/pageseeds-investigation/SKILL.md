# PageSeeds Investigation — Agent Skill

You have access to PageSeeds CLI tools for analyzing any project connected
to PageSeeds. Each tool is an individual subcommand that returns structured
JSON. You decide which tools to call and in what order — there is no single
"run everything" command.

## When to Use This Skill

- User asks about a specific site's SEO performance ("Why is X stuck at Y impressions?")
- User asks to investigate content quality, cannibalization, or site structure
- User pastes a GSC URL and wants analysis
- User mentions a project connected to PageSeeds

## How to Use

Run individual tools as needed:

```bash
cargo run --bin pageseeds-cli -- <tool> -i <project-id> -p <project-path> [args...]
```

**Example investigation flow:**
1. Start with `article-list` to see what content exists
2. Run `gsc-performance` to check impression trends
3. Run `article-title-scan` to check for title bugs
4. Run `article-body-hash` to find exact duplicates
5. If suspicious patterns found, inspect specific files with `framework-files`
6. Use `create-task` or `write-feature-spec` to act on findings

## Finding the project-id

```bash
sqlite3 ~/Library/Application\ Support/com.pageseeds.app/pageseeds.db \
  "SELECT id, name, path FROM projects"
```

## Available Tools

### GSC Data (requires GSC connected)

| Tool | Args | Returns |
|------|------|---------|
| `gsc-performance` | (none) | Page-level clicks, impressions, CTR, position |
| `gsc-queries` | `--page-url URL` (optional) | Search queries driving traffic |
| `gsc-movers` | (none) | Gaining/declining pages vs previous period |

### Article Data

| Tool | Args | Returns |
|------|------|---------|
| `article-list` | `--status published` (optional) | All articles with metadata |
| `article-frontmatter` | `--slug SLUG` | Title, date, word count for one article |
| `article-body-hash` | (none) | SHA-256 hashes, finds exact duplicates |
| `article-title-scan` | (none) | Title patterns: dupes, literal vars, truncation |

### Audit & Health

| Tool | Args | Returns |
|------|------|---------|
| `content-audit-report` | (none) | 21-check per-article health from disk |
| `run-content-audit` | (none) | Runs fresh audit, writes JSON |
| `cannibalization-clusters` | (none) | Cannibalization clusters + merge recs |
| `ctr-health` | (none) | Per-article CTR health summary |
| `create-task` | `-t seo_health_scan -T TITLE -r REASON --auto-enqueue` | Runs the unified SEO health scan |

### Site Structure

| Tool | Args | Returns |
|------|------|---------|
| `indexing-status` | (none) | GSC URL indexing status |
| `framework-files` | `--file PATH` (optional) | Read layout files, sitemap, robots.txt |
| `article-link-graph` | (none) | Internal link graph, orphan detection |

### Actions (mutable)

| Tool | Args | Returns |
|------|------|---------|
| `create-task` | `-t TYPE -T TITLE -r REASON` | Creates fix task in PageSeeds |
| `write-feature-spec` | `-T TITLE -s SEVERITY -m IMPACT -f FILE -c CURRENT -F FIXED` | Writes developer spec to target repo |

## Common Flags (all tools)

- `-i` / `--project-id` — PageSeeds project ID (required)
- `-p` / `--project-path` — Path to project repo (required, ~ expanded)

## Output

All tools print JSON to stdout. Errors go to stderr. Parse stdout as JSON to extract data.

## Notes

- Tools that read GSC data require GSC to be connected. If not configured, they'll return errors.
- `run-content-audit` and `create-task`/`write-feature-spec` are the only tools that mutate anything.
- The `article-body-hash` tool reads all MDX files and computes hashes — it's useful for finding exact duplicate content (often a sign of SSR fallback bugs).
- The `framework-files` tool lists available framework files; use `--file` to read a specific one.
- When investigating, start broad (article-list, gsc-performance) then narrow down (article-frontmatter, framework-files).
