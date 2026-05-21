# PageSeeds Investigation — Agent Skill

You have access to a PageSeeds investigation CLI that can analyze any project
connected to PageSeeds. This skill describes what tools are available, how to
invoke them, and how to interpret results.

## When to Use This Skill

- User asks about a specific site's SEO performance ("Why is X stuck at Y impressions?")
- User asks to investigate content quality, cannibalization, or site structure
- User pastes a GSC URL and wants analysis
- User mentions a project connected to PageSeeds

## Available Investigation Tools

The investigation CLI has access to these data sources (same tools the PageSeeds
desktop app uses):

| Tool | What it returns | Use when |
|------|----------------|----------|
| `gsc_performance` | Page-level clicks, impressions, CTR, position | Impression trends, CTR issues |
| `gsc_queries` | Search queries driving traffic per page | Keyword cannibalization, low-CTR queries |
| `gsc_movers` | Gaining/declining pages vs previous period | Plateau detection, post-change impact |
| `article_list` | All articles with metadata | Content inventory overview |
| `article_frontmatter` | Title, description, date from MDX files | Per-article metadata |
| `article_body_hash` | SHA-256 hashes of all article bodies | Exact duplicate detection |
| `article_title_scan` | Title patterns: dupes, literal vars, truncation | Template bugs, title quality |
| `content_audit_report` | 21-check per-article health report | Comprehensive article quality |
| `run_content_audit` | Runs fresh audit and writes JSON | When cached data is stale |
| `cannibalization_clusters` | Cannibalization clusters + merge recs | Content consolidation |
| `indexing_status` | GSC URL indexing status | Indexing problems, sitemap gaps |
| `ctr_health` | Per-article CTR health | CTR underperformance |
| `framework_files` | Layouts, sitemap config, robots.txt | Template bugs, redirect issues |
| `article_link_graph` | Internal link graph, orphans | Linking gaps, site structure |
| `create_task` | Creates fix tasks in PageSeeds | Actionable content fixes |
| `write_feature_spec` | Writes developer spec to target repo | Code-level fixes for devs |

## How to Run an Investigation

```bash
cargo run --bin investigate -- \
  --project-path ~/code/<project-name> \
  --project-id <project-id> \
  "User's full question here"
```

### Finding the project-id

The project-id is in the PageSeeds SQLite database. Run:

```bash
sqlite3 ~/Library/Application\ Support/com.pageseeds.app/pageseeds.db \
  "SELECT id, name, path FROM projects"
```

Use the `id` column value as `--project-id`.

### Changing the agent provider

Default is `kimi` (Kimi bridge at localhost:8080). To use Claude:

```bash
PAGESEEDS_AGENT_PROVIDER=claude cargo run --bin investigate -- -p ~/code/site -i abc123 "question"
```

Valid providers: `kimi`, `claude`, `openai`, `ollama`.

## Output

The investigation:
1. Prints the agent's natural language answer to stdout
2. Saves structured evidence to `<project>/.github/automation/investigations/<id>/evidence.json`
3. Saves the answer as markdown to `<project>/.github/automation/investigations/<id>/answer.md`
4. Writes any developer feature specs to `<project>/.github/automation/seo_feature_spec.md`

## What to Do After Investigation

1. **Code-level issues** (template bugs, redirects): Read the feature spec, apply fixes in the target repo
2. **Content issues** (duplicate articles, temporal URLs): The agent can create fix tasks via `create_task`
3. **Informational findings**: Share the answer with the user; offer to dig deeper

## Notes

- The agent must have GSC connected to use `gsc_*` tools. If GSC isn't configured, the tools will return errors — the agent will work with the other tools instead.
- The investigation is read-mostly. Only `run_content_audit`, `create_task`, and `write_feature_spec` modify anything.
- The agent is bounded to ~20 tool calls per investigation.
- All tools are thin wrappers around existing Rust functions in `src-tauri/src/`.
