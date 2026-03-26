# Documentation Guide

This directory contains consolidated documentation for the PageSeeds App. This README explains the structure and how to navigate it.

---

## Documentation Philosophy

**Old approach:** 15+ separate spec and process documents, many overlapping, some stale.

**New approach:** 4 core business process docs + consolidated references. Code is the truth — documentation explains **why** and **how it fits together**.

---

## Quick Reference

### I want to understand...

| What you want to know | Read this |
|-----------------------|-----------|
| What the app does | [Business Processes](./BUSINESS_PROCESSES.md) |
| How tasks are executed | [Workflow Engine](./WORKFLOW_ENGINE.md) |
| How the queue works | [Task Queue](./TASK_QUEUE.md) |
| Where data lives | [Data Persistence](./DATA_PERSISTENCE.md) |
| How AI agents work | [Agent Integration](./AGENT_INTEGRATION.md) |
| The critical rules | [CONTRACTS.md](../CONTRACTS.md) |
| How to add features | [AGENTS.md](../AGENTS.md) |
| Quick orientation | [AI_QUICK_START.md](../AI_QUICK_START.md) |

---

## Documentation Map

### Core Documentation (New)

```
docs/
├── README.md                       # This file
├── BUSINESS_PROCESSES.md           # What the app does
│   ├── Keyword Research
│   ├── Content Creation
│   ├── Content Review & Optimization
│   ├── Publishing
│   ├── GSC Collection & Investigation
│   ├── Reddit Opportunity
│   └── Fix Implementation
├── WORKFLOW_ENGINE.md              # How tasks are planned and executed
│   ├── Handlers
│   ├── Workflow Steps
│   ├── Executor
│   ├── Deterministic vs Agentic
│   └── Adding Workflows
├── TASK_QUEUE.md                   # Single execution path
│   ├── Queue Semantics
│   ├── Events
│   ├── Task Spawner
│   └── Debugging
├── DATA_PERSISTENCE.md             # Data architecture
│   ├── SQLite (runtime)
│   ├── JSON files (committed)
│   └── Data Flow
└── AGENT_INTEGRATION.md            # LLM integration
    ├── Agent Providers
    ├── Prompt Assembly
    ├── Normalizers
    └── Safety
```

### Root Documentation

```
├── AI_QUICK_START.md               # Entry point for AI agents
├── AGENTS.md                       # Comprehensive agent guide
├── CONTRACTS.md                    # Runtime invariants (critical)
├── STYLE_GUIDE.md                  # Design system
└── QUEUE_DEBUG.md                  # Debugging guide
```

### Historical Reference (Old)

These documents are kept for historical context but may be stale:

| Document | Purpose |
|----------|---------|
| `agent-dx-improvement-plan.md` | Agent experience improvements (mostly complete) |
| `gsc-collection-gap-analysis.md` | Gap analysis for GSC workflows |
| `keyword-research-fix-plan.md` | Keyword research fixes |
| `keyword-research-gap-analysis.md` | Keyword research gap analysis |
| `release-pipeline-spec.md` | Release/build pipeline specification |
| `task-queue-v2-spec.md` | Detailed task queue v2 specification |

---

## Key Principles

1. **Code is the truth** — Documentation explains intent and architecture, not every detail
2. **Single source of truth** — Each concept documented once, linked elsewhere
3. **Task-oriented** — Docs organized by what users/agents want to accomplish
4. **Living documents** — Update these when architecture changes

---

## When to Update Documentation

### Always Update
- [BUSINESS_PROCESSES.md](./BUSINESS_PROCESSES.md) — When adding/modifying user-facing workflows
- [WORKFLOW_ENGINE.md](./WORKFLOW_ENGINE.md) — When changing handler patterns or step kinds
- [TASK_QUEUE.md](./TASK_QUEUE.md) — When changing queue semantics or events
- [DATA_PERSISTENCE.md](./DATA_PERSISTENCE.md) — When adding SQLite tables or JSON formats
- [AGENT_INTEGRATION.md](./AGENT_INTEGRATION.md) — When changing agent invocation patterns
- [CONTRACTS.md](../CONTRACTS.md) — When adding status values, execution modes, or auto-spawned tasks

### Update If Relevant
- [AI_QUICK_START.md](../AI_QUICK_START.md) — When directory structure changes
- [AGENTS.md](../AGENTS.md) — When adding new patterns or changing conventions

---

## Contributing

When adding documentation:

1. **Identify the right doc** — Don't create new files, extend existing ones
2. **Link liberally** — Cross-reference related concepts
3. **Be concise** — Code samples > prose
4. **Include examples** — Real task types, real file paths
5. **Update the map** — Keep this README current
