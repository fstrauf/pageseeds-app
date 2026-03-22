import React from 'react'
import {
  RefreshCw, CheckCircle2, AlertCircle, Clock, ChevronDown, ChevronUp, ArrowRight,
} from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { cn } from '@/lib/utils'
import type { ActionState, ActionResultPayload } from '../../hooks/useActionRun'
import type { StepProgress } from '../../lib/types'

// ─── Step status helpers ──────────────────────────────────────────────────────

const STEP_TEXT: Record<string, string> = {
  todo: 'text-muted-foreground',
  in_progress: 'text-blue-600',
  review: 'text-amber-600',
  done: 'text-emerald-600',
  ok: 'text-emerald-600',
  failed: 'text-destructive',
  skipped: 'text-muted-foreground',
}

function StepIcon({ status }: { status: StepProgress['status'] }) {
  switch (status) {
    case 'ok': return <CheckCircle2 size={13} className="text-emerald-500 shrink-0" />
    case 'failed': return <AlertCircle size={13} className="text-destructive shrink-0" />
    case 'running': return <RefreshCw size={13} className="animate-spin text-blue-600 shrink-0" />
    case 'skipped': return <AlertCircle size={13} className="text-muted-foreground shrink-0" />
    default: return <Clock size={13} className="text-muted-foreground shrink-0" />
  }
}

// ─── Step row with expand ─────────────────────────────────────────────────────

function StepRow({ step }: { step: StepProgress }) {
  const [expanded, setExpanded] = React.useState(false)
  return (
    <div className={cn('py-1', STEP_TEXT[step.status] ?? 'text-muted-foreground')}>
      <div className="flex items-start gap-2.5">
        <StepIcon status={step.status} />
        <span className="text-xs font-mono text-foreground">{step.step_name}</span>
        {step.message && (
          <span className="ml-1 text-xs text-muted-foreground truncate flex-1">{step.message}</span>
        )}
        {step.output && (
          <button
            onClick={() => setExpanded(e => !e)}
            className="ml-auto shrink-0 text-muted-foreground hover:text-foreground"
          >
            {expanded ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
          </button>
        )}
      </div>
      {expanded && step.output && (
        <pre className="mt-1.5 ml-5 text-xs bg-muted rounded p-2 overflow-x-auto whitespace-pre-wrap text-muted-foreground max-h-40">
          {step.output}
        </pre>
      )}
    </div>
  )
}

// ─── Result body ─────────────────────────────────────────────────────────────

function ResultBody({ result }: { result: ActionResultPayload }) {
  if (result.kind === 'execution') {
    return (
      <div className="space-y-0.5">
        {result.data.steps.map((step) => (
          <StepRow key={step.step_name} step={step} />
        ))}
      </div>
    )
  }

  if (result.kind === 'summary') {
    return (
      <div className="flex flex-wrap gap-x-6 gap-y-1.5">
        {result.items.map((item) => (
          <div key={item.label} className="flex items-center gap-1.5 text-xs">
            <span className="text-muted-foreground">{item.label}</span>
            <span className="font-semibold text-foreground">{item.value}</span>
          </div>
        ))}
        {result.message && (
          <p className="w-full text-xs text-muted-foreground mt-0.5">{result.message}</p>
        )}
      </div>
    )
  }

  // kind === 'message'
  return (
    <p className="text-xs text-muted-foreground">{result.text}</p>
  )
}

// ─── Drawer ───────────────────────────────────────────────────────────────────

interface ActionDrawerProps {
  state: ActionState
  onDismiss: () => void
  /** Optional destination CTA shown on success. Must match your router's view type. */
  onNavigate?: (view: string, taskId?: string) => void
}

/**
 * Universal action feedback drawer.
 * Renders as a fixed panel at the bottom of the content area (right of sidebar).
 * Handles three result kinds: execution steps, summary key/values, plain message.
 */
export function ActionDrawer({ state, onDismiss, onNavigate }: ActionDrawerProps) {
  if (state.status === 'idle') return null

  const running = state.status === 'running'
  const isError = state.status === 'error'
  const isDone = state.status === 'done'
  const success = isDone && state.result
    ? (state.result.kind === 'execution'
        ? state.result.data.success
        : state.result.kind === 'summary'
          ? state.result.success
          : state.result.success)
    : false

  const hasDetail = isDone && state.result !== null
  const hasNextStep = success && state.nextStep && onNavigate

  return (
    <div className="fixed bottom-0 left-56 right-0 z-50 border-t border-border bg-card shadow-lg animate-in slide-in-from-bottom-2 duration-200">
      {/* Header bar */}
      <div className="flex items-center justify-between px-6 py-3 border-b border-border">
        <div className="flex items-center gap-2 min-w-0">
          {running && <RefreshCw size={14} className="animate-spin text-blue-600 shrink-0" />}
          {isDone && success && <CheckCircle2 size={14} className="text-emerald-500 shrink-0" />}
          {(isDone && !success) || isError
            ? <AlertCircle size={14} className="text-destructive shrink-0" />
            : null}

          <span className="text-sm font-medium text-foreground truncate">{state.label}</span>

          {running && (
            <span className="text-xs text-muted-foreground animate-pulse shrink-0">
              Running — this may take a moment…
            </span>
          )}

          {isDone && (
            <Badge
              variant="outline"
              className={cn(
                'border-transparent text-xs shrink-0',
                success ? 'bg-emerald-100 text-emerald-700' : 'bg-destructive/15 text-destructive',
              )}
            >
              {success ? 'complete' : 'failed'}
            </Badge>
          )}

          {isError && (
            <span className="text-xs text-destructive truncate">{state.errorMessage}</span>
          )}
        </div>

        <div className="flex items-center gap-2 ml-4 shrink-0">
          {hasNextStep && (
            <Button
              size="xs"
              onClick={() => {
                const taskId = state.result?.kind === 'execution' ? state.result.data.task_id : undefined
                onNavigate!(state.nextStep!.view, taskId)
                onDismiss()
              }}
              className="text-xs bg-primary/10 hover:bg-primary/20 text-primary border-primary/30 border"
              variant="ghost"
            >
              {state.nextStep!.label} <ArrowRight size={11} className="ml-1" />
            </Button>
          )}
          {!running && (
            <Button
              variant="ghost"
              size="xs"
              onClick={onDismiss}
              className="text-muted-foreground text-xs"
            >
              Dismiss
            </Button>
          )}
        </div>
      </div>

      {/* Detail body */}
      {hasDetail && state.result && (
        <div className="px-6 py-3 max-h-52 overflow-y-auto">
          <ResultBody result={state.result} />
        </div>
      )}
    </div>
  )
}
