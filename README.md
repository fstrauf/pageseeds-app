# PageSeeds

**AI-powered SEO content operations for teams that want to grow organic traffic systematically.**

PageSeeds is the **Operator CLI** (`pageseeds-cli`) plus a Rust domain library. It automates the end-to-end SEO content lifecycle — from keyword research to published articles, from performance monitoring to autonomous optimization — from any customer project directory.

> **Desktop discontinued.** Historical Tauri/React desktop releases are archival only. Product development is CLI + domain crates only (issue #184).

---

## What We Do

Most SEO tools stop at data. They show you keywords, rankings, and traffic — then leave you to figure out what to do next. PageSeeds closes the loop by **executing** the work:

| Stage | Traditional SEO Tool | PageSeeds |
|-------|---------------------|-----------|
| **Research** | Show keyword volume & KD | Autonomously research, validate, and select keywords worth targeting |
| **Creation** | Brief templates, writer marketplace | AI writes full SEO-optimized articles with your brand voice |
| **Optimization** | Audit reports with 100+ issues | Prioritizes the 5-10 highest-impact fixes and applies them |
| **Monitoring** | Dashboards you check weekly | Detects indexing issues, CTR drops, and cannibalization automatically |
| **Promotion** | Manual social scheduling | Generates platform-native posts and finds Reddit conversations to join |

**The goal:** Turn SEO from a reactive reporting function into a proactive growth engine that runs on autopilot.

---

## Core Workflows

### 1. Keyword Research & Content Planning
Autonomous keyword discovery using Ahrefs data. The agent extracts themes from your project brief, discovers keywords with volume and manageable difficulty, validates them against your domain, and presents a curated shortlist for your approval. Supports both informational content (blog articles) and commercial intent (landing pages).

### 2. AI Content Creation
Writes full MDX articles from keyword targets using project-specific skills. Articles include proper frontmatter, internal linking, and structure optimized for search. Content is written directly to your repo and tracked in a centralized inventory.

### 3. Content Review & Optimization
Continuously monitors your content portfolio for decay. Combines GSC performance data, 21-rule health audits, and AI analysis to surface the highest-impact improvements — then applies them autonomously or queues them for your review.

### 4. Publishing & Date Management
Manages the transition from draft to published with intelligent date handling. Prevents duplicate publish dates, resolves year mismatches between titles and publication dates, and ensures clean frontmatter across your content directory.

### 5. GSC Monitoring & Technical SEO
Connects to Google Search Console to inspect indexing status, detect coverage issues (robots blocked, noindex tags, fetch errors), and spawn targeted fix tasks. Also syncs search analytics (clicks, impressions, CTR, position) back to your article inventory.

### 6. CTR Optimization
Analyzes title tags, meta descriptions, and snippet quality to identify articles with below-potential CTR. Generates and applies structured fixes to improve click-through rates from search results.

### 7. Cannibalization Detection & Consolidation
Identifies keyword cannibalization — multiple articles competing for the same query — and recommends merge or redirect strategies. Can autonomously consolidate overlapping content into authoritative cluster pages.

### 8. Reddit Opportunity Marketing
Finds relevant Reddit conversations where your content provides genuine value. Searches by keyword, scores opportunities by engagement and accessibility, drafts authentic replies, and tracks posting status.

### 9. Social Media Campaign Generation
Transforms articles and content into platform-native social posts (TikTok, Instagram). Generates hooks, captions, hashtags, and AI image prompts for external generation. Supports template-based campaigns and per-article one-offs.

### 10. Agentic Investigation
Ask natural-language questions about your site's performance and get evidence-backed answers. The agent explores freely across GSC data, content audits, link graphs, and framework files to discover issues pre-defined checks cannot catch.

---

## Architecture

| Layer | Technology |
|-------|-----------|
| **Domain** | Rust library `crates/pageseeds-core` |
| **Operator surface** | `pageseeds-cli` binary |
| **Runtime Store** | SQLite (bundled, no system dependency) |
| **Committed Data** | JSON files in your repo (`articles.json`, automation artifacts) |
| **AI Providers** | Kimi, Copilot, Claude (configurable) |

---

## Install & get started (CLI)

```bash
# Customer install (macOS Apple Silicon prebuilt; other platforms: FROM_SOURCE=1)
curl -fsSL https://raw.githubusercontent.com/fstrauf/pageseeds-app/main/scripts/install-cli.sh | bash

# In a customer project repo
pageseeds-cli setup --path . --yes
pageseeds-cli site-overview
```

→ **[CLI Getting Started](./docs/CLI_GETTING_STARTED.md)** · **[CLI Commercial (free vs paid)](./docs/CLI_COMMERCIAL.md)**

---

## Documentation

| Document | Purpose |
|----------|---------|
| [CLI Getting Started](./docs/CLI_GETTING_STARTED.md) | Install, license notes, desk read, weekly operator path |
| [CLI Release](./docs/CLI_RELEASE.md) | Version bump, `cli-v*` tags, GitHub release assets |
| [CLI Commercial](./docs/CLI_COMMERCIAL.md) | Free desk tools vs paid write/fix/research |
| [Business Processes](./docs/BUSINESS_PROCESSES.md) | Workflows, features, and process interconnections |
| [Workflow Engine](./docs/WORKFLOW_ENGINE.md) | How tasks are planned and executed |
| [Data Persistence](./docs/DATA_PERSISTENCE.md) | SQLite runtime state + JSON committed content |
| [Agent Integration](./docs/AGENT_INTEGRATION.md) | How LLM agents are invoked and responses normalized |
| [Agent Development Playbook](./docs/AGENT_DEVELOPMENT_PLAYBOOK.md) | Scenario-based guide for common development tasks |
| [CONTRACTS.md](./CONTRACTS.md) | Runtime invariants and hidden rules |
| [AGENTS.md](./AGENTS.md) | Agent orientation, core rules, DRY catalog, and pre-change checklist |
| [AI Quick Start](./AI_QUICK_START.md) | TL;DR orientation for AI agents working in this repo |

---

## Development

```bash
# Build operator CLI
cargo build -p pageseeds-cli

# Ship gate (Rust tests + task-store + CLI contract)
pnpm run test:cli

# Install local release binary to ~/.local/bin
pnpm run install:cli
# or: FROM_SOURCE=1 ./scripts/install-cli.sh
```

`pnpm test:all` is an alias of `test:cli` (no frontend/Vite/IPC gates). Run the ship gate locally before merge; there is no PR CI workflow. How to cut a CLI release: **[CLI Release](./docs/CLI_RELEASE.md)**.

---

## Business Objective

PageSeeds exists to answer one question: **"How do we grow organic traffic without hiring a 5-person SEO team?"**

The product treats SEO as a system — with inputs (keywords, content briefs), processes (research, creation, optimization), and feedback loops (GSC data, CTR, rankings). AI agents handle judgment-heavy decisions. Deterministic code handles repeatable operations. Operators stay in control via CLI review/selection tools and approval gates.
