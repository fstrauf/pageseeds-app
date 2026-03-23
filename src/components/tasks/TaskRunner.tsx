import { useEffect, useRef, useState } from 'react'
import { CheckCircle2, XCircle, Clock, Loader2, ChevronDown, ChevronRight, ChevronUp } from 'lucide-react'
import { executeTask } from '../../lib/tauri'
import type { Task, ExecutionResult, StepProgress } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { cn } from '../../lib/utils'

interface RunnerItem {
  task: Task
  status: 'queued' | 'running' | 'done' | 'failed'
  result?: ExecutionResult
  error?: string
}

interface Props {
  tasks: Task[]
  /** Called (with no args) when all tasks have finished, so the parent can reload. */
  onDone: () => void
  /** Called when the user dismisses the panel. */
  onClose: () => void
  /** Notifies parent when run state changes (for disabling duplicate runs, etc.). */
  onRunningChange?: (running: boolean) => void
}

export function TaskRunner({ tasks, onDone, onClose, onRunningChange }: Props) {
  const [items, setItems] = useState<RunnerItem[]>(() =>
    tasks.map(t => ({ task: t, status: 'queued' as const })),
  )
  const [expanded, setExpanded] = useState<Set<string>>(new Set())
  const [isPanelExpanded, setIsPanelExpanded] = useState(true)
  const [isRunning, setIsRunning] = useState(false)
  const [isDone, setIsDone] = useState(false)
  const hasStarted = useRef(false)

  useEffect(() => {
    if (hasStarted.current) return
    hasStarted.current = true
    runAll()
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  async function runAll() {
    setIsRunning(true)
    onRunningChange?.(true)
    for (const task of tasks) {
      setItems(prev => prev.map(it =>
        it.task.id === task.id ? { ...it, status: 'running' } : it,
      ))
      try {
        const result = await executeTask(task.id)
        setItems(prev => prev.map(it =>
          it.task.id === task.id
            ? { ...it, status: result.success ? 'done' : 'failed', result }
            : it,
        ))
        if (!result.success) {
          setExpanded(prev => new Set(prev).add(task.id))
        }
      } catch (e) {
        setItems(prev => prev.map(it =>
          it.task.id === task.id
            ? { ...it, status: 'failed', error: String(e) }
            : it,
        ))
        setExpanded(prev => new Set(prev).add(task.id))
      }
    }
    setIsRunning(false)
    onRunningChange?.(false)
    setIsDone(true)
    onDone()
  }

  function toggleExpand(id: string) {
    setExpanded(prev => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  const succeeded = items.filter(it => it.status === 'done').length
  const failed = items.filter(it => it.status === 'failed').length
  const completed = succeeded + failed
  const progress = tasks.length > 0 ? (completed / tasks.length) * 100 : 0

  const headerLabel = isRunning
    ? 'Running task queue'
    : isDone
      ? 'Task queue finished'
      : 'Task queue pending'

  const summary = isDone
    ? [
        succeeded > 0 ? `${succeeded} succeeded` : null,
        failed > 0 ? `${failed} failed` : null,
      ].filter(Boolean).join(' · ') || 'Done'
    : `${completed} / ${tasks.length} complete`

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
}

function ItemRow({ item, expanded, onToggle }: ItemRowProps) {
  const { task, status, result, error } = item
  const hasDetails = !!(result?.steps?.length || error)

  const durationMs =
    result
      ? new Date(result.finished_at).getTime() - new Date(result.started_at).getTime()
      : undefined

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
          <div className="font-medium text-foreground truncate">{task.title}</div>
          <div className="text-xs text-muted-foreground font-mono mt-0.5">{task.type}</div>
        </div>

        {/* Right-side metadata */}
        <div className="flex items-center gap-2 text-xs text-muted-foreground flex-shrink-0">
          {status === 'queued'  && <span>queued</span>}
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

          {/* Result message */}
          {result?.message && (
            <div className={cn(
              'text-xs font-medium mb-2',
              result.success ? 'text-emerald-700' : 'text-red-600',
            )}>
              {result.message}
            </div>
          )}

          {/* Steps */}
          {result?.steps?.map((step, i) => (
            <StepRow key={i} step={step} />
          ))}
        </div>
      )}
    </div>
  )
}

// ---------------------------------------------------------------------------

function StepRow({ step }: { step: StepProgress }) {
  const [showOutput, setShowOutput] = useState(false)

  const icon =
    step.status === 'ok'      ? '✓' :
    step.status === 'failed'  ? '✗' :
    step.status === 'skipped' ? '–' :
    step.status === 'running' ? '⟳' : '○'

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
        <div className="flex items-baseline gap-2">
          <span className="font-mono text-foreground">{step.step_name}</span>
          {step.message && (
            <span className="text-muted-foreground truncate">{step.message}</span>
          )}
          {step.output && (
            <button
              onClick={() => setShowOutput(v => !v)}
              className="text-muted-foreground underline underline-offset-2 hover:text-foreground flex-shrink-0"
            >
              {showOutput ? 'hide output' : 'show output'}
            </button>
          )}
        </div>
        {step.output && showOutput && (
          <pre className="mt-1 text-muted-foreground font-mono whitespace-pre-wrap break-all bg-secondary/60 rounded px-2 py-1.5 text-[10px] max-h-48 overflow-y-auto">
            {step.output}
          </pre>
        )}
      </div>
    </div>
  )
}
