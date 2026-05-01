# Feature Spec: Cannibalization Review Task Picker

## Problem

Cannibalization review currently behaves differently from keyword research and Reddit search.

Today, `cannibalization_audit` produces strategy output, but follow-up work is managed from the standalone Cannibalization page. Users approve recommendations there, then bulk-create tasks from approved items. This makes the flow feel disconnected from the task system.

The desired model is the same as keyword research and Reddit search:

1. Run a master task.
2. Master task completes into `review`.
3. Open the task drawer.
4. Select follow-up work in a review picker.
5. Create child tasks.
6. Mark the master task `done`.
7. Run child tasks independently.

## Goals

- Make `cannibalization_audit` a task-level review workflow.
- Show cannibalization recommendations inside the task drawer.
- Allow users to select recommendations and create child tasks from that drawer.
- Mark the parent `cannibalization_audit` task as `done` after selected child tasks are created.
- Align cannibalization behavior with `KeywordPicker` and `RedditOpportunityPicker`.
- Keep child tasks independently runnable through the normal task queue.

## Non-Goals

- Remove the existing Cannibalization page entirely.
- Redesign the cannibalization strategy generation pipeline.
- Automatically run all generated child tasks without user selection.
- Change the actual recommendation types produced by the audit.

## User Flow

1. User starts `cannibalization_audit` from Overview or Tasks.
2. The queue runs the audit.
3. On success, the task status becomes `review`.
4. The Task Runner shows a review action.
5. User opens the task drawer.
6. Drawer shows a `CannibalizationPicker`.
7. User selects recommendations:
   - merge/consolidation tasks
   - hub/article tasks
   - territory research tasks
   - calculator rollout tasks
8. User clicks `Create Tasks`.
9. Backend validates selections against the parent task artifact.
10. Backend creates child tasks through `TaskSpawner`.
11. Backend marks the parent task `done`.
12. Frontend queues or reveals the created child tasks.

## Backend Changes

### 1. Add Review Surface

Add a new `TaskReviewSurface` value:

```rust
CannibalizationPicker
```

Serialized value:

```text
cannibalization_picker
```

### 2. Update Task Definition

Change `cannibalization_audit` task metadata:

```rust
TaskDefinition {
    task_type: "cannibalization_audit",
    phase: "investigation",
    run_policy: TaskRunPolicy::AutoEnqueue,
    review_surface: TaskReviewSurface::CannibalizationPicker,
    follow_up_policy: FollowUpPolicy::UserSelection,
    handler_family: HandlerFamily::CannibalizationAudit,
}
```

### 3. Add Selection Command

Add a command shaped like:

```rust
create_cannibalization_tasks_from_selection(
    parent_task_id: String,
    selections: Vec<CannibalizationSelection>,
) -> Result<Vec<Task>, String>
```

Selection shape:

```rust
struct CannibalizationSelection {
    recommendation_type: String, // merge | hub | territory | calculator
    recommendation_id: String,
}
```

### 4. Validate Against Parent Artifact

The command must:

- load the parent task by `parent_task_id`,
- verify `parent_task.type == "cannibalization_audit"`,
- read the cannibalization strategy artifact from the parent task,
- validate selected recommendation IDs exist in that artifact,
- create tasks only for valid selections,
- use `TaskSpawner` for all child task creation,
- preserve idempotency per recommendation,
- mark the parent task `done` after tasks are created.

### 5. Return Full Tasks

Return `Vec<Task>`, not task IDs, so the frontend can enqueue and display child tasks consistently.

## Frontend Changes

### 1. Add `CannibalizationPicker`

Create a task-drawer picker component similar to:

- `KeywordPicker`
- `RedditOpportunityPicker`

Responsibilities:

- parse recommendations from the parent task artifact,
- group recommendations by type,
- allow selecting/deselecting rows,
- show counts and basic recommendation details,
- call `createCannibalizationTasksFromSelection`,
- pass created tasks to `onTasksCreated`.

### 2. Wire Into `TaskDetail`

Render picker when:

```tsx
task.type === 'cannibalization_audit' &&
task.status === 'review'
```

After task creation:

- call `onArticleTasksCreated?.(newTasks)` or a more generic callback,
- close the drawer,
- refresh the parent task,
- let the created tasks appear in the task list/queue.

### 3. Update Task Runner Review Labels

Task Runner should use `review_surface` metadata rather than hard-coded task-type labels.

Expected label:

```text
Select recommendations
```

## Existing Cannibalization Page

Keep the standalone Cannibalization page, but change its role.

Recommended role:

- strategy overview,
- historical/latest strategy browser,
- optional advanced review dashboard,
- task status visibility.

It should no longer be the primary place where the workflow completes.

## Data and Compatibility

- Existing strategy review rows can remain for now.
- New task-drawer flow should not require approval rows.
- Child-task idempotency should continue to prevent duplicate tasks.
- Existing `create_tasks_from_approved_recommendations` can remain temporarily for backward compatibility, but should be treated as legacy.

## Child Task Mapping

| Recommendation Type | Child Task Type |
|---|---|
| `merge` | `consolidate_cluster` |
| `hub` | `write_article` |
| `territory` | `territory_research` |
| `calculator` | `calculator_rollout` |

## Acceptance Criteria

- `cannibalization_audit` completes into `review`.
- Opening a reviewed cannibalization task shows a drawer picker.
- User can select one or more recommendations.
- Creating tasks creates the expected child task types.
- Parent `cannibalization_audit` is marked `done`.
- Created child tasks can be queued and run independently.
- Re-running task creation does not duplicate existing child tasks.
- Keyword and Reddit picker flows remain unchanged.

## Tests

### Backend

- `cannibalization_audit` success maps to `TaskStatus::Review`.
- selection command rejects invalid parent task IDs.
- selection command rejects IDs not present in the parent artifact.
- selection command creates expected child task types.
- selection command marks parent task `done`.
- repeated selection command is idempotent.

### Frontend

- `TaskDetail` renders `CannibalizationPicker` for reviewed cannibalization tasks.
- picker submits selected recommendation IDs.
- picker handles empty selection.
- picker calls parent refresh/close behavior after task creation.
- Task Runner shows the correct review action label for `cannibalization_picker`.

## Open Questions

- Should `cannibalization_audit` remain `AutoEnqueue`, or should it become `UserEnqueue`?
- Should created child tasks auto-insert next in queue, like keyword/Reddit, or just appear in the task board?
- Should the old project-level approval UI remain editable, or become read-only once drawer flow exists?
