import { useState, useEffect, useCallback } from 'react'
import { RefreshCw, ChevronDown, ChevronUp, CheckCircle2, XCircle } from 'lucide-react'
import { useErrorHandler } from '../../lib/toast-context'
import { listLedgerRuns, getLedgerRunSummary, getLedgerRunEvents } from '../../lib/tauri'
import type { LedgerEvent, RunSummary } from '../../lib/types'
import { ScrollArea } from '@/components/ui/scroll-area'
import { cn } from '../../lib/utils'

interface RunHistoryProps {
  projectId: string
}

function EventLog({ events }: { events: LedgerEvent[] }) {
  if (events.length === 0) return <p className="text-xs text-muted-foreground px-2">No events.</p>
  return (
    <div className="max-h-48 overflow-y-auto rounded border border-border bg-muted/30 p-2 flex flex-col gap-0.5">
      {events.map((ev, i) => (
        <div key={i} className="flex gap-2 text-xs text-muted-foreground">
          <span className="shrink-0 tabular-nums">{new Date(ev.timestamp).toLocaleTimeString()}</span>
          <span className="text-foreground font-medium shrink-0">{ev.event_type}</span>
          <span className="truncate">{JSON.stringify(ev.payload)}</span>
        </div>
      ))}
    </div>
  )
}

function RunCard({ runId, projectId }: { runId: string; projectId: string }) {
  const { showError } = useErrorHandler()
  const [summary, setSummary] = useState<RunSummary | null>(null)
  const [events, setEvents] = useState<LedgerEvent[]>([])
  const [expanded, setExpanded] = useState(false)
  const [eventsLoaded, setEventsLoaded] = useState(false)

  useEffect(() => {
    getLedgerRunSummary(projectId, runId)
      .then(setSummary)
      .catch(e => showError(String(e)))
  }, [projectId, runId, showError])

  async function loadEvents() {
    if (eventsLoaded) return
    try {
      const evs = await getLedgerRunEvents(projectId, runId)
      setEvents(evs)
      setEventsLoaded(true)
    } catch (e: unknown) {
      showError(String(e))
    }
  }

  function toggle() {
    setExpanded(v => !v)
    if (!eventsLoaded) loadEvents()
  }

  return (
    <div className="border-b border-border last:border-b-0">
      <button
        className="w-full flex items-center gap-2 px-3 py-2 text-sm hover:bg-muted/40 transition-colors"
        onClick={toggle}
      >
        {summary ? (
          summary.tasks_failed === 0 ? (
            <CheckCircle2 size={13} className="shrink-0 text-green-500" />
          ) : (
            <XCircle size={13} className="shrink-0 text-destructive" />
          )
        ) : (
          <XCircle size={13} className="shrink-0 text-muted-foreground" />
        )}
        <span className="font-mono text-xs text-muted-foreground shrink-0">{runId}</span>
        {summary && (
          <>
            <span className="text-foreground ml-1">
              {summary.tasks_succeeded}/{summary.tasks_processed} tasks OK
            </span>
            <span className="ml-auto text-xs text-muted-foreground">
              {new Date(summary.started_at).toLocaleString()}
            </span>
          </>
        )}
        <span className={cn('ml-2 shrink-0 text-muted-foreground', !summary && 'ml-auto')}>
          {expanded ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
        </span>
      </button>

      {expanded && (
        <div className="px-3 pb-3 flex flex-col gap-2">
          {summary && summary.errors.length > 0 && (
            <div>
              <p className="text-xs font-medium text-destructive mb-0.5">Errors:</p>
              {summary.errors.map((e, i) => (
                <p key={i} className="text-xs text-muted-foreground">{e}</p>
              ))}
            </div>
          )}
          <EventLog events={events} />
        </div>
      )}
    </div>
  )
}

export function RunHistory({ projectId }: RunHistoryProps) {
  const { showError } = useErrorHandler()
  const [runs, setRuns] = useState<string[]>([])
  const [loading, setLoading] = useState(false)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const data = await listLedgerRuns(projectId)
      setRuns(data)
    } catch (e: unknown) {
      showError(String(e))
    } finally {
      setLoading(false)
    }
  }, [projectId, showError])

  useEffect(() => {
    if (projectId) load()
  }, [projectId, load])

  return (
    <ScrollArea className="h-full">
      <div className="p-6 flex flex-col gap-6">
        <div className="flex items-start justify-between">
          <div>
            <h2 className="text-base font-semibold text-foreground mb-1">Execution History</h2>
            <p className="text-xs text-muted-foreground">
              Orchestrator run ledger — JSONL events from{' '}
              <code className="font-mono text-xs">.github/automation/orchestrator_runs/</code>
            </p>
          </div>
          <button
            onClick={load}
            className="text-muted-foreground hover:text-foreground transition-colors shrink-0"
            title="Refresh"
          >
            <RefreshCw size={14} className={cn(loading && 'animate-spin')} />
          </button>
        </div>

        {runs.length === 0 && !loading && (
          <p className="text-sm text-muted-foreground">No runs recorded yet.</p>
        )}

        {runs.length > 0 && (
          <div className="rounded border border-border overflow-hidden">
            {runs.map(runId => (
              <RunCard key={runId} runId={runId} projectId={projectId} />
            ))}
          </div>
        )}
      </div>
    </ScrollArea>
  )
}
