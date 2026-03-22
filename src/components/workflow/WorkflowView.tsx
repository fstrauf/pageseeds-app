import { useState } from 'react'
import { Play, CheckCircle2, XCircle, Clock, AlertCircle, ChevronDown, ChevronUp } from 'lucide-react'
import { executeTask, listTasks } from '../../lib/tauri'
import type { ExecutionResult, StepProgress, Task } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { ScrollArea } from '@/components/ui/scroll-area'
import { cn } from '../../lib/utils'

interface WorkflowViewProps {
  projectId: string
}

const statusIcon = (status: StepProgress['status']) => {
  switch (status) {
    case 'ok': return <CheckCircle2 size={14} className="text-green-500 shrink-0" />
    case 'failed': return <XCircle size={14} className="text-destructive shrink-0" />
    case 'running': return <Clock size={14} className="text-blue-600 animate-pulse shrink-0" />
    case 'skipped': return <AlertCircle size={14} className="text-muted-foreground shrink-0" />
    default: return <div className="w-3.5 h-3.5 rounded-full border border-border shrink-0" />
  }
}

function StepCard({ step }: { step: StepProgress }) {
  const [expanded, setExpanded] = useState(false)
  return (
    <div className={cn('rounded border px-3 py-2 text-sm',
      step.status === 'ok' && 'border-green-500/30 bg-green-500/5',
      step.status === 'failed' && 'border-destructive/30 bg-destructive/5',
      step.status === 'running' && 'border-blue-400/30 bg-blue-400/5',
      step.status === 'skipped' && 'border-border',
      step.status === 'pending' && 'border-border opacity-50',
    )}>
      <div className="flex items-center gap-2">
        {statusIcon(step.status)}
        <span className="font-medium text-foreground">{step.step_name}</span>
        <Badge variant="outline" className="text-xs py-0 h-4">{step.kind}</Badge>
        <span className="ml-auto text-muted-foreground text-xs truncate max-w-xs">{step.message}</span>
        {step.output && (
          <button
            onClick={() => setExpanded(!expanded)}
            className="ml-1 text-muted-foreground hover:text-foreground"
          >
            {expanded ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
          </button>
        )}
      </div>
      {expanded && step.output && (
        <pre className="mt-2 text-xs bg-muted rounded p-2 overflow-x-auto whitespace-pre-wrap text-muted-foreground max-h-40">
          {step.output}
        </pre>
      )}
    </div>
  )
}

export function WorkflowView({ projectId }: WorkflowViewProps) {
  const [tasks, setTasks] = useState<Task[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [result, setResult] = useState<ExecutionResult | null>(null)
  const [running, setRunning] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loaded, setLoaded] = useState(false)

  async function load() {
    setError(null)
    try {
      const data = await listTasks(projectId, 'todo')
      setTasks(data)
      setLoaded(true)
    } catch (e: unknown) {
      setError(String(e))
    }
  }

  async function run() {
    if (!selected) return
    setRunning(true)
    setResult(null)
    setError(null)
    try {
      const r = await executeTask(selected)
      setResult(r)
      await load()
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setRunning(false)
    }
  }

  if (!loaded) {
    return (
      <div className="p-6 flex flex-col gap-4">
        <div>
          <h2 className="text-base font-semibold text-foreground mb-1">Workflow Execution</h2>
          <p className="text-xs text-muted-foreground">Select a task and run it through its workflow handler.</p>
        </div>
        <Button size="sm" variant="outline" onClick={load}>Load Tasks</Button>
      </div>
    )
  }

  return (
    <ScrollArea className="h-full">
      <div className="p-6 flex flex-col gap-6">
        <div>
          <h2 className="text-base font-semibold text-foreground mb-1">Workflow Execution</h2>
          <p className="text-xs text-muted-foreground">Select a task and execute it step-by-step.</p>
        </div>

        <div className="flex flex-col gap-2">
          <label className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
            Ready Tasks ({tasks.length})
          </label>
          {tasks.length === 0 ? (
            <p className="text-sm text-muted-foreground">No todo tasks.</p>
          ) : (
            <div className="max-h-56 overflow-y-auto rounded border border-border divide-y divide-border">
              {tasks.map(t => (
                <button
                  key={t.id}
                  className={cn(
                    'w-full text-left px-3 py-2 text-sm transition-colors',
                    selected === t.id
                      ? 'bg-primary/10 text-primary'
                      : 'hover:bg-muted text-foreground',
                  )}
                  onClick={() => setSelected(t.id)}
                >
                  <span className="font-medium">{t.title ?? t.id}</span>
                  <span className="ml-2 text-xs text-muted-foreground">{t.task_type ?? (t as Task & { type?: string }).type}</span>
                </button>
              ))}
            </div>
          )}
        </div>

        <Button
          size="sm"
          disabled={!selected || running}
          onClick={run}
          className="self-start"
        >
          <Play size={14} className="mr-1.5" />
          {running ? 'Running…' : 'Execute Task'}
        </Button>

        {error && (
          <div className="rounded border border-destructive/50 bg-destructive/5 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}

        {result && (
          <div className="flex flex-col gap-2">
            <div className="flex items-center gap-2">
              {result.success ? (
                <CheckCircle2 size={16} className="text-green-500" />
              ) : (
                <XCircle size={16} className="text-destructive" />
              )}
              <span className="text-sm font-medium text-foreground">
                {result.success ? 'Completed' : 'Failed'}: {result.message}
              </span>
              <span className="ml-auto text-xs text-muted-foreground">
                {new Date(result.started_at).toLocaleTimeString()} — {new Date(result.finished_at).toLocaleTimeString()}
              </span>
            </div>
            <div className="flex flex-col gap-1">
              {result.steps.map((step, i) => (
                <StepCard key={i} step={step} />
              ))}
            </div>
          </div>
        )}
      </div>
    </ScrollArea>
  )
}
