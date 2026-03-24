# Development Process

How to avoid sinking hours into broken features in this repo.

## What went wrong with content review

We ported infrastructure (GSC sync, audit, task runner UI, batch execution, multi-select) for weeks without ever testing a single end-to-end run. Each piece worked in isolation, but the chain was broken at the most important link: the agent prompt.

Specific failures:
- **Wrong CLI flag** (`--dangerously-skip-permissions` doesn't exist in Copilot CLI) — would have been caught by one test run.
- **Empty agent prompts** — optimize_article tasks had no file content, no instructions. Would have been obvious from reading the prompt once.
- **27 tasks instead of 5** — no selection logic ported. Would have been caught by running the workflow and counting the output.
- **SKILL.md used as a prompt** — it's a human doc, not an agent instruction. Would have been caught by reading what the CLI actually sends to its agent.

All of these are "try it once" bugs, not design problems.

## Rules going forward

Date policy baseline for all new work: use the shared date-policy path so published and ready_to_publish articles never have future dates or duplicate publish dates.

### 1. Port behavior, not architecture

When porting from the CLI, start by identifying the **inputs and outputs** of the feature, not the class hierarchy. Content review takes articles.json → produces recommendations.json. That's it. Code the shortest path to that outcome.

Don't build handler registries, step planners, or batch infrastructure until the happy path works end-to-end.

### 2. Test the agent prompt before writing the executor

Before writing any Rust code around an agent call:
1. Copy the prompt the CLI sends (from its runner code, not SKILL.md)
2. Paste it into the Copilot CLI manually: `copilot -p "..." --allow-all-tools --deny-tool='shell(git:*)'`
3. Verify the output matches expectations
4. Then wrap it in Rust

If the prompt doesn't work manually, no amount of infrastructure will fix it.

### 3. One end-to-end run before any UI work

The content review pipeline should have been: write GSC sync → write audit → write article selection → write prompt → run it once → see recommendations.json appear → then build the UI.

Instead we built: GSC sync → audit → TaskRunner overlay → multi-select checkboxes → batch runner → then discovered the agent gets an empty prompt.

Rule: **no UI work until the backend produces correct output from one manual trigger.**

### 4. Read the reference implementation first

The CLI has working, tested implementations of every workflow. Before writing a Rust port:
1. Read the CLI's runner class for that workflow (e.g., `content_review.py`)
2. List every function it calls and what data flows between them
3. Identify which parts are deterministic (port to Rust) vs. agentic (port the prompt)
4. Write the Rust version following the same data flow

We skipped step 3 for content review — we ported the deterministic parts but replaced the agent prompt with a generic "execute this task" stub.

### 5. Spec before code for multi-step features

Any feature that touches more than 2 files gets a spec in `docs/` first. The spec must include:
- **Inputs**: what data exists before the feature runs
- **Outputs**: what files/state exist after
- **Data flow**: what transforms happen, in order
- **Agent prompts**: the actual text sent to the agent (not "we'll use SKILL.md")
- **Acceptance criteria**: how to verify it works (not "it compiles")

This document is the contract between planning and implementation. If the spec doesn't exist, the feature isn't ready to build.

### 6. Keep the feature list short

We were simultaneously working on: content review, task spawning, TaskRunner UI, multi-select, batch runner, Run All button, permission flags, prompt building, and article selection. That's 9 things.

Ship one thing. Verify it works. Then start the next.

### 7. One execution surface

All task/workflow execution UX must use the shared bottom runner drawer (`TaskRunner`) and never introduce separate modal/popup runners.
When a workflow creates downstream work, return those follow-up tasks in the execution result and surface direct actions (`Run now` or `Open task`) in the runner so users never have to hunt for the next step.

### 8. The global task queue is the only execution path

All task execution goes through the global queue (`useQueueRunner` in `App.tsx`, exposed via `QueueContext`). Do not call `executeTask` directly from components.

- **To run a task from anywhere:** call `useQueue().enqueue([{ taskId, projectId, title, taskType, projectName }])`.
- **`QueueContext`** is provided at the root — any component can consume it without prop drilling.
- **`useQueueRunner`** owns all queue state (ordered list, running/paused/done, live step events). Do not add parallel state for this in components.
- **Tauri event `task_step_progress`** is emitted by the Rust executor after each step and consumed by the hook to drive the live step display. If you add new step types, emit the same event — no other wiring needed.
- **Follow-up tasks** are auto-queued from `ExecutionResult.follow_up_tasks`. Return them from the Rust command and they will flow through without any frontend changes.
