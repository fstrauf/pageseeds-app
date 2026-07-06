# Documentation Guide

This directory contains consolidated documentation for the PageSeeds App. This README explains the structure and how to navigate it.

---

## Documentation Philosophy

**Old approach:** 15+ separate spec and process documents, many overlapping, some stale.

**New approach:** Core business process docs + consolidated references. Code is the truth — documentation explains **why** and **how it fits together**.

---

## Quick Reference

### I want to understand...

| What you want to know | Read this |
|-----------------------|-----------|
| What the app does (business perspective) | [Business Processes](./BUSINESS_PROCESSES.md) |
| What the app does (technical overview) | [README.md](../README.md) |
| How tasks are executed | [Workflow Engine](./WORKFLOW_ENGINE.md) |
| Where data lives | [Data Persistence](./DATA_PERSISTENCE.md) |
| How AI agents work | [Agent Integration](./AGENT_INTEGRATION.md) |
| How to build features | [Agent Development Playbook](./AGENT_DEVELOPMENT_PLAYBOOK.md) |
| The critical rules | [CONTRACTS.md](../CONTRACTS.md) |
| Agent rules and orientation | [AGENTS.md](../AGENTS.md) |
| Quick orientation | [AI_QUICK_START.md](../AI_QUICK_START.md) |

---

## Documentation Map

### Core Documentation

```
docs/
├── README.md                       # This file
├── BUSINESS_PROCESSES.md           # What the app does — 14 business workflows
│   ├── Keyword Research
│   ├── Content Creation
│   ├── Content Review & Optimization
│   ├── Publishing
│   ├── GSC Collection & Investigation
│   ├── CTR Optimization
│   ├── Cannibalization Detection
│   ├── Internal Linking & Clustering
│   ├── Reddit Opportunity
│   ├── Social Media Marketing
│   ├── Agentic Investigation
│   ├── Fix Implementation
│   ├── Territory Research
│   └── Calculator Rollout
├── WORKFLOW_ENGINE.md              # How tasks are planned and executed
│   ├── Handlers
│   ├── Workflow Steps
│   ├── Executor
│   ├── Deterministic vs Agentic
│   └── Adding Workflows
├── DATA_PERSISTENCE.md             # Data architecture
│   ├── SQLite (runtime)
│   ├── JSON files (committed)
│   └── Data Flow
├── AGENT_INTEGRATION.md            # LLM integration
│   ├── Agent Providers
│   ├── Prompt Assembly
│   ├── Normalizers
│   └── Safety
├── AGENT_DEVELOPMENT_PLAYBOOK.md   # Scenario-based development guide
│   ├── Changing a Skill
│   ├── Adding Content-Writing Behavior
│   ├── Attaching Tasks to Queue
│   ├── Building Per-Article Fix Pipelines
│   └── Adding Frontend UI
└── AGENTIC_INVESTIGATION_SPEC.md   # Investigation feature specification
```

### Root Documentation

```
├── README.md                       # App overview, business objective, quick start
├── AI_QUICK_START.md               # Entry point for AI agents
├── AGENTS.md                       # Agent orientation, core rules, DRY catalog, pre-change checklist
├── CONTRACTS.md                    # Runtime invariants (critical)
├── STYLE_GUIDE.md                  # Design system
└── QUEUE_DEBUG.md                  # Debugging guide
```

### Other Reference

| Document | Purpose |
|----------|---------|
| `docs/seo_action_plan.md` | SEO action plan (project-specific) |

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
- [DATA_PERSISTENCE.md](./DATA_PERSISTENCE.md) — When adding SQLite tables or JSON formats
- [AGENT_INTEGRATION.md](./AGENT_INTEGRATION.md) — When changing agent invocation patterns
- [CONTRACTS.md](../CONTRACTS.md) — When adding status values, execution modes, or auto-spawned tasks

### Update If Relevant
- [README.md](../README.md) — When app capabilities or business positioning changes
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
