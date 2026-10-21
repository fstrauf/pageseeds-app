# PageSeeds skill pack

Marketing for developers who'd rather be coding. An agent that grows your side project's traffic and sends you pull requests.

PageSeeds Operator is a CLI plus a set of agent skills. The CLI reads your Google Search Console data and your site's content. The skills tell your coding agent (Claude Code, or any agent that reads `SKILL.md` folders) how to run an SEO pass: look at what is happening, pick the pages to create, merge or noindex, and hand the work back as pull requests you can read and merge. SEO is the method; the point is that you keep building.

This pack is the skills and the free-tier docs. The engine that writes, fixes and merges content stays in the licensed CLI.

## 1. Install the CLI

```bash
curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash
# make sure ~/.local/bin is on your PATH
pageseeds-cli --help
```

The prebuilt binary is macOS Apple Silicon only. Other platforms build from source.

## 2. Install the skills

Pick one layout.

**Claude Code plugin** (this repository is also a plugin marketplace):

```
/plugin marketplace add fstrauf/pageseeds-app
/plugin install pageseeds@pageseeds
```

Plugin skills are namespaced, so `pageseeds-run` runs as `/pageseeds:pageseeds-run`. The skill bodies write the short form (`/pageseeds-run`); read that as the namespaced command.

**Plain copy** (Claude Code, Cursor, or any agent that loads `SKILL.md` folders): copy the folders from `.agents/skills/` into your project's skills directory (for Claude Code that is `.claude/skills/`, or `~/.claude/skills/` for all projects). Check your agent's documentation for where it looks for skills.

## 3. Get your first free report

From your site's repository:

```bash
cd /path/to/your-site
pageseeds-cli setup --path . --yes
pageseeds-cli connect gsc
pageseeds-cli site-overview
```

Or ask your agent to run the `onboarding` skill, which does the same steps and tells you what is missing.

`site-overview` prints JSON read from your own Search Console data and article catalog: totals (clicks, impressions), top pages and top movers, a sample of pages Google has not indexed, published pages with zero impressions, pages sitting just off page one ("striking distance"), pages competing for the same query, and pages whose impressions are declining. It reads and reports. It does not change your site, and it makes no claim about future rankings or traffic. If data is missing or stale, the report says so.

## What is in the pack

| Skill | What it does |
|-------|--------------|
| `onboarding` | Installs and sets up the CLI for your project, connects Search Console, and reads your first site report. |
| `pageseeds-run` | The SEO pass: plans a worklist of pages to create, merge and noindex from your Search Console data, writes each one, and opens one pull request that holds only those files. |
| `keyword-explore` | Scouts keyword opportunities for your site and reports what it would take to win the best ones. Report only. |
| `competitor-review` | Researches one competitor into a comparison matrix and drafts the page for it. |
| `competitor-catalog-hub` | Packages a hub page that ties your competitor comparison pages together. |
| `reddit-engage` | Finds Reddit threads where your project is relevant and drafts replies for you to approve. |

## Requirements

- A terminal, and a coding agent that can run shell commands.
- Your own API keys. PageSeeds does not resell data or LLM access.
- Google Search Console access for the site you want to look at (the `connect gsc` step).
- macOS on Apple Silicon for the prebuilt CLI; other platforms build from source.
- A git repository for the site, so changes can arrive as pull requests.

## Free and paid

Free: see what is going on. Paid: research, write, fix, merge, and the scheduled operator run. One annual license covers the paid tools; read-only Search Console and site reports work without one and keep working if a license lapses.

The authoritative list lives in `docs/CLI_COMMERCIAL.md` in the PageSeeds repository and is reproduced here from it.

**Free (no license):**

`list-projects`, `create-project`, `delete-project`, `setup`, `gsc-connect`, `connect`, `sync-site-urls`, `site-overview`, `articles`, `article`, `article-list`, `article-frontmatter`, `article-body-hash`, `article-title-scan`, `article-link-graph`, `framework-files`, `video-clip-context`, `youtube-analytics`, `video-clip-list`, `gsc-performance`, `gsc-queries`, `gsc-movers`, `gsc-daily`, `bing-daily`, `traffic-channels`, `bing-performance`, `bing-queries`, `bing-url-info`, `bing-crawl-issues`, `bing-submit-urls`, `bing-submit-sitemap`, `indexnow-submit`, `indexing-status`, `source-gate`, `ai-visibility-audit`, `site-health-audit`, `article-quality-reviews`, `strategy`, `project-config-status`, `competitor-catalog-status`, `migrate-project-config`, `list-tasks`, `get-task`, `validate-article`, `verify-redirects`

**Paid (license required):**

`write-context`, `write-submit`, `publish-content`, `fix-context`, `fix-submit`, `merge-context`, `merge-submit`, `keyword-explore`, `page-fetch`, `reddit-search`, `reddit-posted-brief`, `reddit-listen`, `create-task`, `execute-task`, `cancel-tasks`, `update-task-status`, `set-task-status`, `create-reddit-replies`, `write-feature-spec`, `compare-rendered`, `reindex-article-evidence`, `operator-runs`, `worktree`, `program`, `run`

The skills start with free desk reads. Where a skill reaches a paid tool (writing or fixing content, research pulls, scheduling), the CLI tells you a license is required and stops; activate one with `pageseeds-cli license activate <key>`. See https://pageseeds.com.

## What this does not do

- It does not promise rankings, traffic numbers or guaranteed results.
- It is not an agency or a done-for-you service, and there is no hosted dashboard to log into.
- Autonomy is bounded: a run ships only the pages its worklist names, sensitive moves are surfaced for your decision, and everything lands as commits in your own repository.
