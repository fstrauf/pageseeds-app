import { useEffect, useMemo, useRef, useState } from 'react'
import { CheckCircle2, XCircle, Clock, Loader2, ChevronDown, ChevronRight, ChevronUp, Pause, Play, X, AlertTriangle } from 'lucide-react'
import type { FollowUpTask, RunnerItem, StepProgress } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Dialog, DialogContent, DialogHeader, DialogTitle } from '@/components/ui/dialog'
import { ScrollArea } from '@/components/ui/scroll-area'
import { cn } from '../../lib/utils'
import { useQueue } from '../../lib/queue-context'

import { createLogger, LogTarget } from '../../lib/logging';
const logger = createLogger(LogTarget.UI);

// ─── Props ──────────────────────────────────────────────────────────────────

interface Props {
  /** Items managed by useQueueRunner */
  items: RunnerItem[]
  isRunning: boolean
  isPaused: boolean
  onPause: () => void
  onResume: () => void
  onRemove: (taskId: string) => void
  onClose: () => void
  onOpenTask?: (taskId: string) => void
}

export function TaskRunner({
  items,
  isRunning,
  isPaused,
  onPause,
  onResume,
  onRemove,
  onClose,
  onOpenTask,
}: Props) {
  logger.entry('TaskRunner', { itemCount: items.length, isRunning, isPaused });
  
  const [userExpanded, setUserExpanded] = useState<Set<string>>(new Set())
  const [isPanelExpanded, setIsPanelExpanded] = useState(true)
  const [prevStatusMap, setPrevStatusMap] = useState<Map<string, string>>(new Map())
  const queue = useQueue()
  const autoOpenedRef = useRef<Set<string>>(new Set())

  // Derive auto-expand ids from status transitions using previous state map
  const autoExpandIds = useMemo(() => {
    const ids: string[] = []
    for (const item of items) {
      const was = prevStatusMap.get(item.task.id)
      const now = item.status
      if (was && was !== now && (now === 'done' || now === 'failed')) {
        logger.stateChange(`task ${item.task.id}`, was, now)
        const hasFollowUps = (item.result?.follow_up_tasks?.length ?? 0) > 0
        if (now === 'failed' || hasFollowUps) {
          ids.push(item.task.id)
        }
      }
    }
    return ids
  }, [items, prevStatusMap])

  // Combine user-expanded + auto-expanded
  const expanded = useMemo(() => {
    const next = new Set(userExpanded)
    for (const id of autoExpandIds) next.add(id)
    return next
  }, [userExpanded, autoExpandIds])

  // Update prev status map after render so next render can detect transitions.
  // Idempotent: only sets state if the content actually changed, avoiding extra re-renders.
  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect
    setPrevStatusMap(prev => {
      if (prev.size !== items.length) {
        logger.stateChange('items', prev.size, items.length)
        return new Map(items.map(it => [it.task.id, it.status]))
      }
      for (const item of items) {
        if (prev.get(item.task.id) !== item.status) {
          return new Map(items.map(it => [it.task.id, it.status]))
        }
      }
      return prev
    })
  }, [items])

  // Auto-open review tasks (bounded side effect — only fires on real status changes)
  useEffect(() => {
    for (const item of items) {
      const was = prevStatusMap.get(item.task.id)
      const now = item.status
      if (was && was !== now && now === 'done') {
        const isReviewTask = item.result?.follow_up_tasks?.some(
          f => f.id === item.task.id && f.status === 'review'
        )
        if (isReviewTask && onOpenTask && !autoOpenedRef.current.has(item.task.id)) {
          logger.debug('auto-opening review task', { id: item.task.id })
          autoOpenedRef.current.add(item.task.id)
          onOpenTask(item.task.id)
        }
      }
    }
  }, [items, onOpenTask, prevStatusMap])

  const succeeded = items.filter(it => it.status === 'done').length
  const failed = items.filter(it => it.status === 'failed').length
  const completed = succeeded + failed
  const total = items.length
  const progress = total > 0 ? (completed / total) * 100 : 0
  const isDone = !isRunning && completed === total && total > 0

  function toggleExpand(id: string) {
    setUserExpanded(prev => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const headerLabel = isRunning
    ? isPaused
      ? 'Task queue paused'
      : 'Running task queue'
    : isDone
      ? 'Task queue finished'
      : 'Task queue pending'

  const summary = isDone
    ? [
        succeeded > 0 ? `${succeeded} succeeded` : null,
        failed > 0 ? `${failed} failed` : null,
      ].filter(Boolean).join(' · ') || 'Done'
    : `${completed} / ${total} complete`

  return (
    <div className="fixed bottom-0 left-56 right-0 z-50 border-t border-border bg-card shadow-lg animate-in slide-in-from-bottom-2 duration-200">
      <div className="px-6 py-3 border-b border-border">
        <div className="flex items-center justify-between gap-3">
          <div className="min-w-0 flex items-center gap-2">
            {isRunning && <Loader2 size={14} className="animate-spin text-blue-600 shrink-0" />}
            {isDone && failed === 0 && <CheckCircle2 size={14} className="text-emerald-500 shrink-0" />}
            {isDone && failed > 0 && <XCircle size={14} className="text-red-500 shrink-0" />}
            {!isRunning && !isDone && <Clock size={14} className="text-muted-foreground shrink-0" />}

            <span className="text-sm font-medium text-foreground truncate">{headerLabel}</span>
            <Badge variant="outline" className="text-xs border-border text-muted-foreground shrink-0">
              {summary}
            </Badge>
          </div>

          <div className="flex items-center gap-1.5 shrink-0">
            {isRunning && (
              <Button
                variant="ghost"
                size="xs"
                onClick={isPaused ? onResume : onPause}
                className="text-xs text-muted-foreground"
              >
                {isPaused ? (
                  <><Play size={12} className="mr-1" />Resume</>
                ) : (
                  <><Pause size={12} className="mr-1" />Pause</>
                )}
              </Button>
            )}
            <Button
              variant="ghost"
              size="xs"
              onClick={() => setIsPanelExpanded(v => !v)}
              className="text-xs text-muted-foreground"
            >
              {isPanelExpanded ? (
                <><ChevronDown size={12} className="mr-1" />Collapse</>
              ) : (
                <><ChevronUp size={12} className="mr-1" />Expand</>
              )}
            </Button>
            {!isRunning && (
              <Button
                variant="ghost"
                size="xs"
                onClick={onClose}
                className="text-xs text-muted-foreground"
              >
                Dismiss
              </Button>
            )}
          </div>
        </div>

        <div className="mt-2.5 h-1.5 w-full rounded-full bg-secondary overflow-hidden">
          <div
            className={cn(
              'h-full rounded-full transition-all duration-500 ease-out',
              isDone
                ? failed > 0 ? 'bg-amber-500' : 'bg-emerald-500'
                : 'bg-primary',
            )}
            style={{ width: `${progress}%` }}
          />
        </div>
      </div>

      {isPanelExpanded && (
        <>
          <div className="max-h-56 overflow-y-auto p-4 space-y-2 min-h-0">
            {items.map(item => (
              <ItemRow
                key={item.task.id}
                item={item}
                expanded={expanded.has(item.task.id)}
                onToggle={() => toggleExpand(item.task.id)}
                onRunNow={(taskId) => {
                  queue.enqueueNext([{
                    taskId,
                    projectId: item.task.projectId ?? '',
                    title: `Follow-up task`,
                    taskType: 'follow_up',
                    projectName: item.task.projectName,
                  }])
                }}
                onRemove={onRemove}
                onOpenTask={onOpenTask}
              />
            ))}
          </div>

          <div className="px-6 py-3 border-t border-border flex items-center justify-between">
            {isRunning ? (
              <span className="text-xs text-muted-foreground flex items-center gap-2">
                <Loader2 size={12} className="animate-spin" />
                Running in background while you continue using the app
              </span>
            ) : (
              <span className="text-xs text-muted-foreground">Queue complete</span>
            )}
            {!isRunning && (
              <Button
                variant="outline"
                size="sm"
                onClick={onClose}
              >
                Close
              </Button>
            )}
          </div>
        </>
      )}
    </div>
  )
}

// ---------------------------------------------------------------------------

interface ItemRowProps {
  item: RunnerItem
  expanded: boolean
  onToggle: () => void
  onRunNow: (taskId: string) => void
  onRemove: (taskId: string) => void
  onOpenTask?: (taskId: string) => void
}

function ItemRow({ item, expanded, onToggle, onRunNow, onRemove, onOpenTask }: ItemRowProps) {
  const { task, status, result, error, liveSteps } = item
  const hasDetails = !!(result?.steps?.length || result?.follow_up_tasks?.length || error || (liveSteps && liveSteps.length > 0))

  const durationMs =
    result
      ? new Date(result.finished_at).getTime() - new Date(result.started_at).getTime()
      : undefined

  // Detect missing files from ctr_build_context step output
  const missingFileInfo = useMemo(() => {
    const buildStep = result?.steps?.find((s: StepProgress) => s.step_name === 'ctr_build_context')
    if (!buildStep?.output) return null
    try {
      const parsed = JSON.parse(buildStep.output)
      const cleaned = parsed.cleaned_stale_entries ?? 0
      const files: string[] = parsed.cleaned_files ?? []
      if (cleaned > 0) return { count: cleaned, files }
      // Also check if any article has file_not_found issue
      const articles = parsed.top_20_by_clicks_lost ?? parsed.articles ?? []
      const missing = articles.filter((a: Record<string, unknown>) => (a.issues_detected as Record<string, boolean> | undefined)?.file_not_found)
      if (missing.length > 0) return { count: missing.length, files: missing.map((a: Record<string, unknown>) => a.file as string) }
      return null
    } catch {
      return null
    }
  }, [result])

  const hasMissingFiles = !!missingFileInfo

  return (
    <div
      className={cn(
        'rounded-lg border text-sm transition-colors',
        status === 'running' && 'border-blue-300 bg-blue-50/40 dark:bg-blue-950/20',
        status === 'done'    && 'border-emerald-200 bg-emerald-50/30 dark:bg-emerald-950/10',
        status === 'failed'  && 'border-red-200 bg-red-50/30 dark:bg-red-950/10',
        status === 'queued'  && 'border-border bg-card',
      )}
    >
      {/* Row header */}
      <div
        className={cn(
          'flex items-center gap-3 px-4 py-3',
          hasDetails && 'cursor-pointer select-none',
        )}
        onClick={hasDetails ? onToggle : undefined}
      >
        {/* Status icon */}
        <div className="flex-shrink-0 w-5">
          {status === 'queued'  && <Clock     size={15} className="text-muted-foreground" />}
          {status === 'running' && <Loader2   size={15} className="text-blue-600 animate-spin" />}
          {status === 'done'    && <CheckCircle2 size={15} className="text-emerald-600" />}
          {status === 'failed'  && <XCircle   size={15} className="text-red-500" />}
        </div>

        {/* Task info */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <div className="font-medium text-foreground truncate">{task.title}</div>
            {hasMissingFiles && (
              <Badge variant="outline" className="text-[10px] h-4 px-1 border-amber-300 text-amber-700 bg-amber-50 shrink-0">
                <AlertTriangle size={10} className="mr-0.5" />
                {missingFileInfo.count} missing
              </Badge>
            )}
          </div>
          <div className="text-xs text-muted-foreground font-mono mt-0.5">{task.type}</div>
        </div>

        {/* Right-side metadata */}
        <div className="flex items-center gap-2 text-xs text-muted-foreground flex-shrink-0">
          {status === 'queued'  && (
            <>
              <span>queued</span>
              <Button
                variant="ghost"
                size="xs"
                onClick={(e) => { e.stopPropagation(); onRemove(task.id) }}
                className="text-xs text-muted-foreground h-5 w-5 p-0"
              >
                <X size={12} />
              </Button>
            </>
          )}
          {status === 'running' && <span className="text-blue-600 font-medium">running…</span>}
          {status === 'done'    && durationMs != null && <span>{(durationMs / 1000).toFixed(1)}s</span>}
          {status === 'failed'  && <span className="text-red-500 font-medium">failed</span>}
          {hasDetails && (
            expanded
              ? <ChevronDown size={14} />
              : <ChevronRight size={14} />
          )}
        </div>
      </div>

      {/* Expanded detail */}
      {expanded && hasDetails && (
        <div className="px-4 pb-3 pt-2 border-t border-inherit space-y-1.5">
          {/* Raw error (no result) */}
          {error && (
            <div className="text-xs text-red-600 font-mono bg-red-50 dark:bg-red-950/30 rounded px-2 py-1.5">
              {error}
            </div>
          )}

          {/* Missing files warning */}
          {hasMissingFiles && (
            <div className="rounded-md border border-amber-200 bg-amber-50 dark:bg-amber-950/20 px-3 py-2 mb-2">
              <div className="flex items-center gap-1.5 text-xs font-medium text-amber-800">
                <AlertTriangle size={12} />
                {missingFileInfo.count} file{missingFileInfo.count !== 1 ? 's' : ''} referenced in articles.json not found on disk
              </div>
              <div className="mt-1 text-[10px] text-amber-700 font-mono space-y-0.5 max-h-24 overflow-y-auto">
                {missingFileInfo.files.map((f: string, i: number) => (
                  <div key={i} className="truncate">{f}</div>
                ))}
              </div>
            </div>
          )}

          {/* Result message */}
          {result?.message && (
            <div className={cn(
              'text-xs font-medium mb-2',
              result.success ? 'text-emerald-700' : 'text-red-600',
            )}>
              {result.message}
            </div>
          )}

          {/* Live steps (while running, before result arrives) */}
          {!result && liveSteps && liveSteps.length > 0 && liveSteps.map((step, i) => (
            <StepRow key={i} step={step} />
          ))}

          {/* Steps from finished result */}
          {result?.steps?.map((step, i) => (
            <StepRow key={i} step={step} />
          ))}

          {result?.follow_up_tasks && result.follow_up_tasks.length > 0 && (
            <FollowUpList
              followUps={result.follow_up_tasks}
              onRunNow={onRunNow}
              onOpenTask={onOpenTask}
            />
          )}
        </div>
      )}
    </div>
  )
}

interface FollowUpListProps {
  followUps: FollowUpTask[]
  onRunNow: (taskId: string) => void
  onOpenTask?: (taskId: string) => void
}

function FollowUpList({ followUps, onRunNow, onOpenTask }: FollowUpListProps) {
  if (followUps.length === 0) return null

  return (
    <div className="mt-2.5 pt-2 border-t border-inherit space-y-1.5">
      <div className="text-[11px] font-medium text-muted-foreground">
        Next task{followUps.length !== 1 ? 's' : ''}
      </div>
      {followUps.map(task => {
        const canRun = task.status === 'todo' && task.execution_mode !== 'manual'
        const isReview = task.status === 'review'
        const reviewLabel =
          task.task_type === 'research_keywords' || task.task_type === 'custom_keyword_research'
            ? 'Select keywords'
            : 'Review results'
        return (
          <div
            key={task.id}
            className="rounded-md border border-border/70 bg-background/60 px-2.5 py-2 flex items-center gap-2"
          >
            <div className="min-w-0 flex-1">
              <div className="text-xs text-foreground truncate">{task.title}</div>
              <div className="text-[10px] text-muted-foreground font-mono mt-0.5 truncate">
                {task.task_type} · {task.status}
              </div>
            </div>
            {isReview ? (
              <Button
                size="xs"
                onClick={() => onOpenTask?.(task.id)}
                className="text-[11px]"
              >
                {reviewLabel}
              </Button>
            ) : canRun ? (
              <Button
                size="xs"
                onClick={() => onRunNow(task.id)}
                className="text-[11px]"
              >
                Run now
              </Button>
            ) : (
              <Button
                size="xs"
                variant="outline"
                onClick={() => onOpenTask?.(task.id)}
                className="text-[11px]"
              >
                Open task
              </Button>
            )}
          </div>
        )
      })}
    </div>
  )
}

// ---------------------------------------------------------------------------

function StepRow({ step }: { step: StepProgress }) {
  const [dialogOpen, setDialogOpen] = useState(false)
  const [expanded, setExpanded] = useState(false)

  const icon =
    step.status === 'ok'      ? '✓' :
    step.status === 'failed'  ? '✗' :
    step.status === 'skipped' ? '–' :
    step.status === 'running' ? '⟳' : '○'

  const isFailed = step.status === 'failed'
  const isImplementation = step.step_name === 'implementation_agent_stage'

  // For implementation steps, show a preview inline so users can see what changed
  // without clicking through a dialog.
  const outputPreview = useMemo(() => {
    if (!step.output || !isImplementation) return null
    // Extract first ~400 chars or first paragraph
    const text = step.output.trim()
    const firstPara = text.split('\n\n')[0] ?? text
    return firstPara.length > 400 ? firstPara.slice(0, 400) + '…' : firstPara
  }, [step.output, isImplementation])

  return (
    <div className="flex items-start gap-2 text-xs">
      <span className={cn(
        'mt-0.5 flex-shrink-0 font-mono w-3 text-center',
        step.status === 'ok'      && 'text-emerald-600',
        step.status === 'failed'  && 'text-red-500',
        step.status === 'skipped' && 'text-muted-foreground',
        step.status === 'running' && 'text-blue-600',
        step.status === 'pending' && 'text-muted-foreground',
      )}>
        {icon}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2 flex-wrap">
          <span className="font-mono text-foreground">{step.step_name}</span>
          {step.message && (
            <span className={cn(
              isFailed ? 'text-red-600 font-medium' : 'text-muted-foreground truncate'
            )}>
              {step.message}
            </span>
          )}
          {step.output && (
            <button
              onClick={() => setDialogOpen(true)}
              className="text-muted-foreground underline underline-offset-2 hover:text-foreground flex-shrink-0"
            >
              {isFailed ? 'show details' : 'show output'}
            </button>
          )}
        </div>

        {/* Inline preview for implementation agent output */}
        {outputPreview && (
          <div className="mt-1.5">
            <button
              onClick={() => setExpanded(v => !v)}
              className="text-[10px] text-muted-foreground hover:text-foreground underline underline-offset-2"
            >
              {expanded ? 'Hide preview' : 'Show what changed'}
            </button>
            {expanded && (
              <div className="mt-1 rounded border bg-secondary/30 px-2.5 py-2 text-[11px] text-muted-foreground whitespace-pre-wrap break-words max-h-64 overflow-y-auto">
                {outputPreview}
              </div>
            )}
          </div>
        )}

        <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
          <DialogContent className={cn('max-w-2xl', isFailed && 'border-red-200')}>
            <DialogHeader>
              <DialogTitle className={cn('font-mono text-sm', isFailed && 'text-red-600')}>
                {step.step_name}{isFailed ? ' — failed' : ''}
              </DialogTitle>
            </DialogHeader>
            {isFailed && step.message && (
              <div className="text-xs text-red-600 font-medium mb-2">
                {step.message}
              </div>
            )}
            <ScrollArea className="h-[60vh] w-full rounded border">
              <pre className="p-3 text-[11px] font-mono whitespace-pre-wrap break-words text-muted-foreground">
                {step.output}
              </pre>
            </ScrollArea>
          </DialogContent>
        </Dialog>
      </div>
    </div>
  )
}
