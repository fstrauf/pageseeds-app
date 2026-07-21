import { useEffect, useMemo, useState } from 'react'
import { CheckSquare, Square, Loader2, Wrench } from 'lucide-react'
import { selectContentReviewFollowUps } from '../../lib/tauri'
import { useQueue } from '../../lib/queue-context'
import { useErrorHandler } from '../../lib/toast-context'
import type { ContentReviewProposal, ContentReviewSelectableArtifact, Task } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { cn } from '../../lib/utils'

interface ContentReviewPickerProps {
  task: Task
  onTasksCreated: (tasks: Task[]) => void
}

interface ProposalRow {
  proposal: ContentReviewProposal
  selected: boolean
}

function priorityColor(priority: string | null | undefined): string {
  switch (priority) {
    case 'high':
      return 'bg-red-100 text-red-700 border-transparent'
    case 'medium':
      return 'bg-amber-100 text-amber-700 border-transparent'
    case 'low':
      return 'bg-slate-100 text-slate-700 border-transparent'
    default:
      return 'bg-slate-100 text-slate-700 border-transparent'
  }
}

function truncate(str: string, maxLen: number): string {
  if (str.length <= maxLen) return str
  return str.slice(0, maxLen - 3) + '...'
}

export function ContentReviewPicker({ task, onTasksCreated }: ContentReviewPickerProps) {
  const queue = useQueue()
  const [rows, setRows] = useState<ProposalRow[]>([])
  const [summary, setSummary] = useState<string | null>(null)
  const [droppedCount, setDroppedCount] = useState(0)
  const [creating, setCreating] = useState(false)
  const { showError } = useErrorHandler()

  useEffect(() => {
    const artifact = task.artifacts.find(a => a.key === 'content_review_proposals')
    if (!artifact?.content) {
      // list_tasks returns light tasks without artifacts; TaskDetail hydrates
      // via getTask(). Don't error here — just show empty until hydrated.
      setRows([])
      setSummary(null)
      setDroppedCount(0)
      return
    }

    try {
      const data: ContentReviewSelectableArtifact = JSON.parse(artifact.content)
      setSummary(data.findings_summary ?? null)
      setDroppedCount(data.dropped?.length ?? 0)
      setRows(
        (data.proposals ?? []).map(proposal => ({
          proposal,
          selected: false,
        })),
      )
    } catch {
      showError('Failed to parse content review proposals. The data may be corrupted.')
    }
  }, [task, showError])

  const selectedCount = useMemo(() => rows.filter(r => r.selected).length, [rows])

  function toggleAll(selected: boolean) {
    setRows(rows.map(r => ({ ...r, selected })))
  }

  function toggleRow(id: string) {
    setRows(rows.map(r => (r.proposal.id === id ? { ...r, selected: !r.selected } : r)))
  }

  async function handleCreateTasks() {
    const selected = rows.filter(r => r.selected)
    if (selected.length === 0) return

    setCreating(true)
    try {
      const proposalIds = selected.map(r => r.proposal.id)
      const newTasks = await selectContentReviewFollowUps(task.id, proposalIds)

      if (newTasks.length > 0) {
        queue.enqueue(
          newTasks.map(t => ({
            taskId: t.id,
            projectId: t.project_id,
            title: t.title ?? t.type ?? 'Task',
            taskType: t.type ?? 'unknown',
            projectName: undefined,
          })),
        )
      }

      onTasksCreated(newTasks)
    } catch (e: unknown) {
      showError(String(e))
    } finally {
      setCreating(false)
    }
  }

  if (rows.length === 0) {
    return (
      <div className="text-xs text-muted-foreground py-4 space-y-1">
        <p>No fix proposals available. The review may not have found actionable recommendations.</p>
        {droppedCount > 0 && (
          <p className="text-muted-foreground/80">
            {droppedCount} proposal{droppedCount !== 1 ? 's' : ''}{' '}
            {droppedCount === 1 ? 'was' : 'were'} dropped during validation
            (duplicates, active tasks, or invalid params).
          </p>
        )}
      </div>
    )
  }

  return (
    <div className="space-y-3">
      {summary && (
        <p className="text-xs text-muted-foreground">{summary}</p>
      )}

      <div className="flex items-center justify-between text-xs">
        <div className="flex items-center gap-3 flex-wrap">
          <span className="text-muted-foreground">
            {rows.length} proposal{rows.length !== 1 ? 's' : ''}
          </span>
          {droppedCount > 0 && (
            <span className="text-muted-foreground/70">
              ({droppedCount} dropped)
            </span>
          )}
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <Button
            variant="ghost"
            size="xs"
            onClick={() => toggleAll(true)}
            className="text-[10px] h-6"
          >
            All
          </Button>
          <Button
            variant="ghost"
            size="xs"
            onClick={() => toggleAll(false)}
            className="text-[10px] h-6"
          >
            None
          </Button>
        </div>
      </div>

      <div className="space-y-2 max-h-96 overflow-y-auto pr-1">
        {rows.map(({ proposal, selected }) => (
          <div
            key={proposal.id}
            className={cn(
              'rounded-lg border p-3 space-y-1.5 transition-colors',
              selected
                ? 'border-primary bg-primary/5'
                : 'border-border bg-background hover:bg-secondary/40',
            )}
          >
            <div className="flex items-start gap-3">
              <button
                type="button"
                onClick={() => toggleRow(proposal.id)}
                className="mt-0.5 shrink-0 text-muted-foreground hover:text-foreground"
              >
                {selected ? <CheckSquare size={16} /> : <Square size={16} />}
              </button>

              <div className="flex-1 min-w-0 space-y-1">
                <div className="flex items-center gap-2 flex-wrap">
                  <h4 className="text-sm font-medium text-foreground leading-tight">
                    {truncate(proposal.title, 120)}
                  </h4>
                  <Badge className="text-[10px] border-transparent bg-blue-100 text-blue-700">
                    <span className="flex items-center gap-1">
                      <Wrench size={12} />
                      {proposal.task_type}
                    </span>
                  </Badge>
                  {proposal.priority && (
                    <Badge className={cn('text-[10px] border-transparent', priorityColor(proposal.priority))}>
                      {proposal.priority}
                    </Badge>
                  )}
                </div>

                {proposal.description && (
                  <p className="text-xs text-muted-foreground">
                    {truncate(proposal.description, 200)}
                  </p>
                )}
              </div>
            </div>
          </div>
        ))}
      </div>

      <div className="flex items-center justify-between pt-1">
        <span className="text-xs text-muted-foreground">
          {selectedCount} selected
        </span>
        <Button
          size="sm"
          disabled={selectedCount === 0 || creating}
          onClick={handleCreateTasks}
        >
          {creating ? (
            <>
              <Loader2 size={14} className="animate-spin mr-1.5" />
              Creating…
            </>
          ) : (
            `Create Tasks${selectedCount > 0 ? ` (${selectedCount})` : ''}`
          )}
        </Button>
      </div>
    </div>
  )
}
