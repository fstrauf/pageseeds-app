import { useState, useEffect, useCallback } from 'react'
import { Zap, RefreshCw, CheckCircle2, XCircle, AlertTriangle } from 'lucide-react'
import { getBatchSummary, runBatch } from '../../lib/tauri'
import type { BatchResult, BatchSummary } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Badge } from '@/components/ui/badge'
import { cn } from '../../lib/utils'

interface BatchPanelProps {
  projectId: string
}

export function BatchPanel({ projectId }: BatchPanelProps) {
  const [summary, setSummary] = useState<BatchSummary | null>(null)
  const [result, setResult] = useState<BatchResult | null>(null)
  const [running, setRunning] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [maxTasks, setMaxTasks] = useState(20)
  const [pauseOnError, setPauseOnError] = useState(true)

  const loadSummary = useCallback(async () => {
    try {
      const s = await getBatchSummary(projectId)
      setSummary(s)
    } catch (e: unknown) {
      setError(String(e))
    }
  }, [projectId])

  useEffect(() => {
    if (projectId) loadSummary()
  }, [projectId, loadSummary])

  async function startBatch() {
    setRunning(true)
    setResult(null)
    setError(null)
    try {
      const r = await runBatch(projectId, maxTasks, pauseOnError)
      setResult(r)
      await loadSummary()
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setRunning(false)
    }
  }

  const statusColor = (status: BatchResult['status']) =>
    status === 'complete' ? 'text-green-500' : status === 'error' ? 'text-destructive' : 'text-yellow-600'

  return (
    <ScrollArea className="h-full">
      <div className="p-6 flex flex-col gap-6">
        <div>
          <h2 className="text-base font-semibold text-foreground mb-1">Batch Mode</h2>
          <p className="text-xs text-muted-foreground">
            Run all ready autonomous tasks sequentially.
          </p>
        </div>

        {/* Summary card */}
        {summary && (
          <div className="grid grid-cols-3 gap-3">
            {[
              { label: 'Ready', value: summary.total_ready, accent: true },
              { label: 'Automatic', value: summary.automatic, accent: false },
              { label: 'Batchable', value: summary.batchable, accent: false },
            ].map(({ label, value, accent }) => (
              <div
                key={label}
                className={cn('rounded border p-3 text-center', accent ? 'border-primary/40 bg-primary/5' : 'border-border bg-card')}
              >
                <div className={cn('text-2xl font-bold', accent ? 'text-primary' : 'text-foreground')}>
                  {value}
                </div>
                <div className="text-xs text-muted-foreground mt-0.5">{label}</div>
              </div>
            ))}
          </div>
        )}

        {/* Config */}
        <div className="flex items-center gap-4 flex-wrap">
          <label className="flex items-center gap-2 text-sm text-foreground">
            Max tasks:
            <input
              type="number"
              min={1}
              max={100}
              value={maxTasks}
              onChange={e => setMaxTasks(Number(e.target.value))}
              className="w-16 rounded border border-border bg-input text-foreground text-sm px-2 py-0.5 text-center"
            />
          </label>
          <label className="flex items-center gap-2 text-sm text-foreground cursor-pointer">
            <input
              type="checkbox"
              checked={pauseOnError}
              onChange={e => setPauseOnError(e.target.checked)}
              className="rounded"
            />
            Pause on error
          </label>
          <button
            onClick={loadSummary}
            className="text-muted-foreground hover:text-foreground transition-colors"
            title="Refresh"
          >
            <RefreshCw size={14} />
          </button>
        </div>

        <div className="flex items-center gap-3">
          <Button
            size="sm"
            disabled={running || (summary?.total_ready === 0)}
            onClick={startBatch}
          >
            <Zap size={14} className="mr-1.5" />
            {running ? 'Running…' : 'Start Batch'}
          </Button>
          {running && (
            <span className="text-xs text-muted-foreground animate-pulse">Processing tasks…</span>
          )}
        </div>

        {error && (
          <div className="rounded border border-destructive/50 bg-destructive/5 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}

        {result && (
          <div className="flex flex-col gap-3">
            <div className="flex items-center gap-2">
              {result.status === 'complete' && <CheckCircle2 size={16} className="text-green-500" />}
              {result.status === 'error' && <XCircle size={16} className="text-destructive" />}
              {result.status === 'paused' && <AlertTriangle size={16} className="text-yellow-600" />}
              <span className={cn('text-sm font-medium', statusColor(result.status))}>
                {result.status.charAt(0).toUpperCase() + result.status.slice(1)}
              </span>
              <Badge variant="outline" className="text-xs">{result.processed} processed</Badge>
              <span className="ml-auto text-xs text-muted-foreground">
                {(result.duration_ms / 1000).toFixed(1)}s
              </span>
            </div>

            {result.results.length > 0 && (
              <div className="flex flex-col gap-1">
                <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">Succeeded</p>
                {result.results.map((r, i) => (
                  <div key={i} className="flex items-center gap-2 text-sm">
                    <CheckCircle2 size={12} className="text-green-500 shrink-0" />
                    <span className="text-foreground truncate">{r.title || r.task_id}</span>
                    <span className="text-xs text-muted-foreground ml-auto shrink-0">{r.task_type}</span>
                  </div>
                ))}
              </div>
            )}

            {result.errors.length > 0 && (
              <div className="flex flex-col gap-1">
                <p className="text-xs font-medium text-muted-foreground uppercase tracking-wide">Errors</p>
                {result.errors.map((r, i) => (
                  <div key={i} className="flex flex-col gap-0.5 text-sm">
                    <div className="flex items-center gap-2">
                      <XCircle size={12} className="text-destructive shrink-0" />
                      <span className="text-foreground truncate">{r.title || r.task_id}</span>
                    </div>
                    <p className="text-xs text-muted-foreground pl-5">{r.message}</p>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </ScrollArea>
  )
}
