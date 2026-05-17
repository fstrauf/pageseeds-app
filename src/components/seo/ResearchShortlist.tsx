import { useEffect, useMemo, useState } from 'react'
import {
  Trash2,
  Plus,
  Search,
  Loader2,
  CheckSquare,
  Square,
  Sparkles,
  RotateCcw,
} from 'lucide-react'
import { useErrorHandler } from '../../lib/toast-context'
import {
  listResearchShortlist,
  addResearchShortlistEntry,
  deleteResearchShortlistEntry,
  resetResearchShortlistEntry,
  createTask,
} from '../../lib/tauri'
import { useQueue } from '../../lib/queue-context'
import type { ResearchShortlistEntry } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Input } from '@/components/ui/input'
import { cn } from '../../lib/utils'

interface Props {
  projectId: string
}

type StatusFilter = 'all' | 'pending' | 'researched' | 'covered' | 'saturated'

function statusBadgeClass(status: string): string {
  switch (status) {
    case 'pending':
      return 'bg-amber-50 text-amber-700 border-amber-200'
    case 'researched':
      return 'bg-blue-50 text-blue-700 border-blue-200'
    case 'covered':
      return 'bg-green-50 text-green-700 border-green-200'
    case 'saturated':
      return 'bg-gray-50 text-gray-700 border-gray-200'
    default:
      return 'bg-secondary text-secondary-foreground border-transparent'
  }
}

function priorityBadgeClass(priority: string): string {
  switch (priority) {
    case 'high':
      return 'bg-red-50 text-red-700 border-red-200'
    case 'medium':
      return 'bg-orange-50 text-orange-700 border-orange-200'
    default:
      return 'bg-gray-50 text-gray-700 border-gray-200'
  }
}

export function ResearchShortlist({ projectId }: Props) {
  const [entries, setEntries] = useState<ResearchShortlistEntry[]>([])
  const [loading, setLoading] = useState(true)
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('pending')
  const [selected, setSelected] = useState<Set<bigint>>(new Set())
  const [creating, setCreating] = useState(false)
  const [showAddForm, setShowAddForm] = useState(false)
  const [newTheme, setNewTheme] = useState('')
  const [newSeeds, setNewSeeds] = useState('')
  const [newPriority, setNewPriority] = useState('medium')

  const { showError } = useErrorHandler()
  const queue = useQueue()

  const filteredEntries = useMemo(() => {
    if (statusFilter === 'all') return entries
    return entries.filter((e) => e.status === statusFilter)
  }, [entries, statusFilter])

  useEffect(() => {
    loadEntries()
  }, [projectId, statusFilter])

  async function loadEntries() {
    setLoading(true)
    try {
      const data = await listResearchShortlist(
        projectId,
        statusFilter === 'all' ? undefined : statusFilter,
      )
      setEntries(data)
      setSelected(new Set())
    } catch (e) {
      showError(String(e))
    } finally {
      setLoading(false)
    }
  }

  function toggleSelect(id: bigint) {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
  }

  function selectAll() {
    setSelected(new Set(filteredEntries.map((e) => e.id).filter(Boolean) as bigint[]))
  }

  function selectNone() {
    setSelected(new Set())
  }

  async function handleDelete(id: bigint) {
    try {
      await deleteResearchShortlistEntry(id)
      await loadEntries()
    } catch (e) {
      showError(String(e))
    }
  }

  async function handleReset(id: bigint) {
    try {
      await resetResearchShortlistEntry(id)
      await loadEntries()
    } catch (e) {
      showError(String(e))
    }
  }

  async function handleAdd() {
    if (!newTheme.trim()) return
    const seeds = newSeeds
      .split(/[,\n]/)
      .map((s) => s.trim())
      .filter((s) => s.length > 0)
    try {
      await addResearchShortlistEntry(
        projectId,
        newTheme.trim(),
        seeds.length > 0 ? seeds : [newTheme.trim()],
        newPriority,
      )
      setNewTheme('')
      setNewSeeds('')
      setShowAddForm(false)
      await loadEntries()
    } catch (e) {
      showError(String(e))
    }
  }

  async function handleResearchSelected() {
    const selectedEntries = entries.filter((e) => e.id !== null && selected.has(e.id))
    if (selectedEntries.length === 0) return

    setCreating(true)
    try {
      // Build description: one theme per line
      const themes = selectedEntries.map((e) => e.theme)
      const description = themes.join('\n')

      const task = await createTask(
        projectId,
        'custom_keyword_research',
        `Research: ${themes.slice(0, 3).join(', ')}${themes.length > 3 ? ` +${themes.length - 3} more` : ''}`,
        description,
        'high',
        true,
      )

      // Auto-enqueue the task
      queue.enqueue([
        {
          taskId: task.id,
          projectId,
          title: task.title ?? 'Custom keyword research',
          taskType: task.type ?? 'custom_keyword_research',
        },
      ])

      // Mark selected shortlist entries as researched
      for (const entry of selectedEntries) {
        if (entry.id !== null) {
          // Note: we don't have a direct mark_researched command for single entries,
          // but the task pipeline will mark them when it runs. For now we just refresh.
          // The backend pipeline marks shortlist entries as researched after completion.
        }
      }

      setSelected(new Set())
      await loadEntries()
    } catch (e) {
      showError(String(e))
    } finally {
      setCreating(false)
    }
  }

  const counts = useMemo(() => {
    const c: Record<string, number> = { all: entries.length }
    for (const e of entries) {
      c[e.status] = (c[e.status] || 0) + 1
    }
    return c
  }, [entries])

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Toolbar */}
      <div className="px-4 py-3 border-b shrink-0 space-y-3">
        <div className="flex items-center justify-between gap-2">
          <div className="flex items-center gap-1.5">
            {(['all', 'pending', 'researched', 'covered', 'saturated'] as StatusFilter[]).map(
              (status) => (
                <Button
                  key={status}
                  variant={statusFilter === status ? 'default' : 'outline'}
                  size="xs"
                  onClick={() => setStatusFilter(status)}
                  className="text-[11px] h-7 capitalize"
                >
                  {status}
                  {counts[status] !== undefined && (
                    <span className="ml-1 text-[10px] opacity-70">({counts[status]})</span>
                  )}
                </Button>
              ),
            )}
          </div>
          <Button
            variant="outline"
            size="sm"
            onClick={() => setShowAddForm((s) => !s)}
            className="h-7 text-xs"
          >
            <Plus size={13} className="mr-1" />
            Add Idea
          </Button>
        </div>

        {showAddForm && (
          <div className="space-y-2 p-3 bg-secondary/30 rounded-md border border-border">
            <div className="flex gap-2">
              <Input
                placeholder="Theme (e.g. 'project management software')"
                value={newTheme}
                onChange={(e) => setNewTheme(e.target.value)}
                className="h-8 text-xs flex-1"
              />
              <select
                value={newPriority}
                onChange={(e) => setNewPriority(e.target.value)}
                className="h-8 text-xs border border-border rounded px-2 bg-background"
              >
                <option value="high">High</option>
                <option value="medium">Medium</option>
                <option value="low">Low</option>
              </select>
            </div>
            <Input
              placeholder="Seeds (optional, comma or line separated)"
              value={newSeeds}
              onChange={(e) => setNewSeeds(e.target.value)}
              className="h-8 text-xs"
            />
            <div className="flex gap-2">
              <Button size="xs" onClick={handleAdd} disabled={!newTheme.trim()} className="h-7 text-xs">
                Save
              </Button>
              <Button
                variant="ghost"
                size="xs"
                onClick={() => setShowAddForm(false)}
                className="h-7 text-xs"
              >
                Cancel
              </Button>
            </div>
          </div>
        )}

        {selected.size > 0 && (
          <div className="flex items-center justify-between gap-2">
            <span className="text-xs text-muted-foreground">
              {selected.size} selected
            </span>
            <div className="flex gap-1.5">
              <Button
                variant="ghost"
                size="xs"
                onClick={selectAll}
                className="h-7 text-xs"
              >
                All
              </Button>
              <Button
                variant="ghost"
                size="xs"
                onClick={selectNone}
                className="h-7 text-xs"
              >
                None
              </Button>
              <Button
                size="sm"
                onClick={handleResearchSelected}
                disabled={creating}
                className="h-7 text-xs"
              >
                {creating ? (
                  <Loader2 size={12} className="mr-1 animate-spin" />
                ) : (
                  <Search size={12} className="mr-1" />
                )}
                Research Selected
              </Button>
            </div>
          </div>
        )}
      </div>

      {/* List */}
      <div className="flex-1 overflow-y-auto p-4">
        {loading ? (
          <div className="flex items-center justify-center py-12">
            <Loader2 size={16} className="animate-spin text-muted-foreground" />
          </div>
        ) : filteredEntries.length === 0 ? (
          <div className="text-sm text-muted-foreground text-center py-12">
            No {statusFilter === 'all' ? '' : statusFilter} entries in the shortlist.
            {statusFilter === 'pending' && (
              <p className="text-xs mt-1">
                Run "Update Research Shortlist" from Tasks, or add your own ideas above.
              </p>
            )}
          </div>
        ) : (
          <div className="space-y-1.5">
            {filteredEntries.map((entry) => {
              const isSelected = entry.id !== null && selected.has(entry.id)
              return (
                <div
                  key={entry.id?.toString() ?? entry.theme}
                  className={cn(
                    'flex items-start gap-2.5 px-3 py-2.5 rounded-md border text-xs transition-colors',
                    isSelected
                      ? 'bg-primary/5 border-primary/20'
                      : 'bg-secondary/20 border-transparent hover:bg-secondary/40',
                  )}
                >
                  <button
                    onClick={() => entry.id !== null && toggleSelect(entry.id)}
                    className="mt-0.5 shrink-0"
                  >
                    {isSelected ? (
                      <CheckSquare size={14} className="text-primary" />
                    ) : (
                      <Square size={14} className="text-muted-foreground" />
                    )}
                  </button>

                  <div className="flex-1 min-w-0 space-y-1">
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="font-medium text-foreground truncate">
                        {entry.theme}
                      </span>
                      <Badge
                        variant="outline"
                        className={cn('text-[10px] px-1.5 py-0 h-4', statusBadgeClass(entry.status))}
                      >
                        {entry.status}
                      </Badge>
                      <Badge
                        variant="outline"
                        className={cn('text-[10px] px-1.5 py-0 h-4', priorityBadgeClass(entry.priority))}
                      >
                        {entry.priority}
                      </Badge>
                    </div>

                    {entry.seeds.length > 0 && (
                      <div className="flex flex-wrap gap-1">
                        {entry.seeds.map((seed) => (
                          <span
                            key={seed}
                            className="text-[10px] px-1.5 py-0.5 bg-background border border-border rounded text-muted-foreground"
                          >
                            {seed}
                          </span>
                        ))}
                      </div>
                    )}

                    <div className="flex items-center gap-3 text-[10px] text-muted-foreground">
                      <span>Source: {entry.source}</span>
                      {entry.article_count !== null && entry.article_count > 0 && (
                        <span>{entry.article_count} articles</span>
                      )}
                      {entry.total_impressions !== null && entry.total_impressions > 0 && (
                        <span>{Math.round(entry.total_impressions).toLocaleString()} impressions</span>
                      )}
                      <span>{new Date(entry.added_at).toLocaleDateString()}</span>
                    </div>
                  </div>

                  <div className="flex items-center gap-1 shrink-0 mt-0.5">
                    {(entry.status === 'researched' || entry.status === 'covered') && (
                      <button
                        onClick={() => entry.id !== null && handleReset(entry.id)}
                        className="text-muted-foreground hover:text-primary transition-colors"
                        title="Reset to pending"
                      >
                        <RotateCcw size={13} />
                      </button>
                    )}
                    <button
                      onClick={() => entry.id !== null && handleDelete(entry.id)}
                      className="text-muted-foreground hover:text-destructive transition-colors"
                      title="Delete"
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>
                </div>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}
