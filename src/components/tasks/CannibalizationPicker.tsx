import { useEffect, useMemo, useState } from 'react'
import { CheckSquare, Square, Loader2, GitMerge, Landmark, Map, Calculator } from 'lucide-react'
import { createCannibalizationTasksFromSelection } from '../../lib/tauri'
import { useQueue } from '../../lib/queue-context'
import { useErrorHandler } from '../../lib/toast-context'
import type { Task, CannibalizationStrategy } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'
import { cn } from '../../lib/utils'

interface CannibalizationPickerProps {
  task: Task
  onTasksCreated: (tasks: Task[]) => void
}

interface RecRow {
  recommendation_type: string
  recommendation_id: string
  title: string
  subtitle: string
  detail: string
  selected: boolean
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function typeLabel(type: string): string {
  switch (type) {
    case 'merge': return 'Merge'
    case 'hub': return 'Hub Page'
    case 'territory': return 'Territory'
    case 'calculator': return 'Calculator'
    default: return type
  }
}

function typeIcon(type: string) {
  switch (type) {
    case 'merge': return <GitMerge size={14} />
    case 'hub': return <Landmark size={14} />
    case 'territory': return <Map size={14} />
    case 'calculator': return <Calculator size={14} />
    default: return null
  }
}

function typeColor(type: string): string {
  switch (type) {
    case 'merge': return 'bg-blue-100 text-blue-700 border-transparent'
    case 'hub': return 'bg-purple-100 text-purple-700 border-transparent'
    case 'territory': return 'bg-emerald-100 text-emerald-700 border-transparent'
    case 'calculator': return 'bg-amber-100 text-amber-700 border-transparent'
    default: return 'bg-slate-100 text-slate-700 border-transparent'
  }
}

function truncate(str: string, maxLen: number): string {
  if (str.length <= maxLen) return str
  return str.slice(0, maxLen - 3) + '...'
}

// ─── Component ───────────────────────────────────────────────────────────────

export function CannibalizationPicker({ task, onTasksCreated }: CannibalizationPickerProps) {
  const queue = useQueue()
  const [rows, setRows] = useState<RecRow[]>([])
  const [creating, setCreating] = useState(false)
  const { showError } = useErrorHandler()

  // Parse strategy from task artifact
  useEffect(() => {
    const artifact = task.artifacts.find(a => a.key === 'cannibalization_strategy')
    if (!artifact?.content) {
      // list_tasks returns light tasks without artifacts; TaskDetail hydrates
      // via getTask(). Don't error here — just show empty until hydrated.
      setRows([])
      return
    }

    try {
      const strategy: CannibalizationStrategy = JSON.parse(artifact.content)
      const newRows: RecRow[] = []

      for (const rec of strategy.merge_recommendations ?? []) {
        newRows.push({
          recommendation_type: 'merge',
          recommendation_id: rec.cluster_id,
          title: `Merge: ${rec.cluster_id}`,
          subtitle: `Keep ${rec.keep_url} → redirect ${rec.redirect_urls?.length ?? 0} page(s)`,
          detail: rec.reason ?? '',
          selected: false,
        })
      }

      for (const rec of strategy.hub_recommendations ?? []) {
        newRows.push({
          recommendation_type: 'hub',
          recommendation_id: rec.topic,
          title: rec.suggested_title || `Hub: ${rec.topic}`,
          subtitle: rec.suggested_url || rec.topic,
          detail: `Intent: ${rec.intent || '—'} · Spokes: ${rec.spoke_pages?.length ?? 0}`,
          selected: false,
        })
      }

      for (const rec of strategy.territory_recommendations ?? []) {
        newRows.push({
          recommendation_type: 'territory',
          recommendation_id: rec.theme,
          title: `Territory: ${rec.theme}`,
          subtitle: `Priority: ${rec.priority || '—'}`,
          detail: rec.suggested_tasks?.join(', ') || '',
          selected: false,
        })
      }

      for (const rec of strategy.calculator_recommendations ?? []) {
        newRows.push({
          recommendation_type: 'calculator',
          recommendation_id: rec.strategy,
          title: `Calculator: ${rec.strategy}`,
          subtitle: `Universe: ${rec.ticker_universe || '—'}`,
          detail: rec.reason ?? '',
          selected: false,
        })
      }

      setRows(newRows)
    } catch {
      showError('Failed to parse cannibalization strategy. The data may be corrupted.')
    }
  }, [task, showError])

  const selectedCount = useMemo(() => rows.filter(r => r.selected).length, [rows])

  const countsByType = useMemo(() => {
    const map: Record<string, number> = {}
    for (const row of rows) {
      map[row.recommendation_type] = (map[row.recommendation_type] || 0) + 1
    }
    return map
  }, [rows])

  function toggleAll(selected: boolean) {
    setRows(rows.map(r => ({ ...r, selected })))
  }

  function toggleRow(recommendation_type: string, recommendation_id: string) {
    setRows(rows.map(r =>
      r.recommendation_type === recommendation_type && r.recommendation_id === recommendation_id
        ? { ...r, selected: !r.selected }
        : r
    ))
  }

  async function handleCreateTasks() {
    const selected = rows.filter(r => r.selected)
    if (selected.length === 0) return

    setCreating(true)
    try {
      const selections = selected.map(r => ({
        recommendation_type: r.recommendation_type,
        recommendation_id: r.recommendation_id,
      }))
      const newTasks = await createCannibalizationTasksFromSelection(task.id, selections)

      // Auto-add created tasks to the queue (shopping cart pattern)
      if (newTasks.length > 0) {
        queue.enqueue(
          newTasks.map(t => ({
            taskId: t.id,
            projectId: t.project_id,
            title: t.title ?? t.type ?? 'Task',
            taskType: t.type ?? 'unknown',
            projectName: undefined,
          }))
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
      <div className="text-xs text-muted-foreground py-4">
        No recommendations found. The audit may have returned an empty strategy.
      </div>
    )
  }

  return (
    <div className="space-y-3">
      {/* Summary */}
      <div className="flex items-center justify-between text-xs">
        <div className="flex items-center gap-3 flex-wrap">
          <span className="text-muted-foreground">
            {rows.length} recommendations
          </span>
          {Object.entries(countsByType).map(([type, count]) => (
            <Badge key={type} className={cn('text-[10px] border-transparent', typeColor(type))}>
              {typeLabel(type)} {count}
            </Badge>
          ))}
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

      {/* Recommendations list */}
      <div className="space-y-2 max-h-96 overflow-y-auto pr-1">
        {rows.map((row) => (
          <div
            key={`${row.recommendation_type}:${row.recommendation_id}`}
            className={cn(
              'rounded-lg border p-3 space-y-1.5 transition-colors',
              row.selected
                ? 'border-primary bg-primary/5'
                : 'border-border bg-background hover:bg-secondary/40'
            )}
          >
            <div className="flex items-start gap-3">
              <button
                onClick={() => toggleRow(row.recommendation_type, row.recommendation_id)}
                className="mt-0.5 shrink-0 text-muted-foreground hover:text-foreground"
              >
                {row.selected ? <CheckSquare size={16} /> : <Square size={16} />}
              </button>

              <div className="flex-1 min-w-0 space-y-1">
                <div className="flex items-center gap-2 flex-wrap">
                  <h4 className="text-sm font-medium text-foreground leading-tight">
                    {truncate(row.title, 120)}
                  </h4>
                  <Badge className={cn('text-[10px] border-transparent', typeColor(row.recommendation_type))}>
                    <span className="flex items-center gap-1">
                      {typeIcon(row.recommendation_type)}
                      {typeLabel(row.recommendation_type)}
                    </span>
                  </Badge>
                </div>

                <p className="text-xs text-muted-foreground">
                  {row.subtitle}
                </p>

                {row.detail && (
                  <p className="text-[11px] text-muted-foreground leading-relaxed">
                    {truncate(row.detail, 200)}
                  </p>
                )}
              </div>
            </div>
          </div>
        ))}
      </div>

      <Separator className="bg-border" />

      {/* Footer actions */}
      <div className="flex items-center justify-between">
        <div className="text-xs text-muted-foreground">
          {selectedCount === 0
            ? 'Select recommendations to create tasks'
            : `${selectedCount} selected`}
        </div>

        <Button
          size="sm"
          onClick={handleCreateTasks}
          disabled={selectedCount === 0 || creating}
        >
          {creating ? (
            <>
              <Loader2 size={14} className="mr-1.5 animate-spin" />
              Creating...
            </>
          ) : (
            <>Create Tasks</>
          )}
        </Button>
      </div>
    </div>
  )
}
