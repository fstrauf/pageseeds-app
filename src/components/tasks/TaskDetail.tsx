import { useState, useEffect } from 'react'
import { Trash2, AlertCircle, Ban, ArrowRight, Play, ChevronDown } from 'lucide-react'
import { updateTask, deleteTask, cancelTask, listTasks, executeTask, getTask } from '../../lib/tauri'
import type { Task } from '../../lib/types'
import { cn, formatDate } from '../../lib/utils'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Separator } from '@/components/ui/separator'
import { Label } from '@/components/ui/label'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Sheet, SheetContent, SheetTitle } from '@/components/ui/sheet'

const STATUS_BADGE: Record<string, string> = {
  todo: 'bg-secondary text-secondary-foreground border-transparent',
  in_progress: 'bg-indigo-100 text-indigo-700 border-transparent',
  review: 'bg-amber-100 text-amber-700 border-transparent',
  done: 'bg-emerald-100 text-emerald-700 border-transparent',
  cancelled: 'bg-secondary text-muted-foreground border-transparent',
}

interface TaskDetailProps {
  task: Task
  onClose: () => void
  onUpdated: (task: Task) => void
  onDeleted: (id: string) => void
}

export function TaskDetail({ task, onClose, onUpdated, onDeleted }: TaskDetailProps) {
  const [editTitle, setEditTitle] = useState(task.title ?? '')
  const [editDesc, setEditDesc] = useState(task.description ?? '')
  const [editPriority, setEditPriority] = useState(task.priority ?? 'medium')
  const [saving, setSaving] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [dismissing, setDismissing] = useState(false)
  const [executing, setExecuting] = useState(false)
  const [execMsg, setExecMsg] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [spawnedTasks, setSpawnedTasks] = useState<import('../../lib/types').Task[]>([])

  // When a content_review task is done, load the content_review_apply task it spawned
  useEffect(() => {
    if (task.type !== 'content_review' || task.status !== 'done') return
    listTasks(task.project_id, 'todo')
      .then(all => setSpawnedTasks(all.filter(t => t.type === 'content_review_apply')))
      .catch(() => setSpawnedTasks([]))
  }, [task.id, task.type, task.status, task.project_id])

  const isDirty =
    editTitle !== (task.title ?? '') ||
    editDesc !== (task.description ?? '') ||
    editPriority !== (task.priority ?? 'medium')

  async function handleSave() {
    setSaving(true)
    setError(null)
    try {
      const updated = await updateTask(
        task.id,
        editTitle || undefined,
        editDesc || undefined,
        editPriority,
      )
      onUpdated(updated)
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setSaving(false)
    }
  }

  async function handleDelete() {
    setDeleting(true)
    setError(null)
    try {
      await deleteTask(task.id)
      onDeleted(task.id)
    } catch (e: unknown) {
      setError(String(e))
      setDeleting(false)
      setConfirmDelete(false)
    }
  }

  async function handleDismiss() {
    setDismissing(true)
    setError(null)
    try {
      const updated = await cancelTask(task.id)
      onUpdated(updated)
      onClose()
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setDismissing(false)
    }
  }

  async function handleExecute() {
    setExecuting(true)
    setExecMsg(null)
    setError(null)
    try {
      const result = await executeTask(task.id)
      // Fetch the refreshed task (status will have changed)
      const refreshed = await getTask(task.id)
      onUpdated(refreshed)
      setExecMsg(result.success ? result.message : null)
      if (!result.success) setError(result.message)
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setExecuting(false)
    }
  }

  return (
    <Sheet open modal={false} onOpenChange={(o) => { if (!o) onClose() }}>
      <SheetContent
        className="w-150 sm:max-w-150 flex flex-col gap-0 p-0"
        aria-describedby={undefined}
      >
        <SheetTitle className="sr-only">{task.title ?? task.type}</SheetTitle>

        {/* Header */}
        <div className="flex items-center gap-2 min-w-0 px-5 py-4 border-b border-border pr-12">
          <Badge variant="secondary" className="font-mono text-xs shrink-0">
            {task.type}
          </Badge>
          <span className="text-xs text-muted-foreground truncate font-mono">{task.id}</span>
        </div>

      <ScrollArea className="flex-1 min-h-0">
        <div className="px-5 py-5 space-y-5">
          {error && (
            <div className="flex items-start gap-2 px-3 py-2.5 rounded-md text-sm bg-destructive/15 text-destructive">
              <AlertCircle size={14} className="mt-0.5 shrink-0" />
              {error}
            </div>
          )}

          {/* Title */}
          <div className="space-y-1.5">
            <Label className="text-xs text-muted-foreground">Title</Label>
            <Input
              value={editTitle}
              onChange={e => setEditTitle(e.target.value)}
              placeholder="Task title…"
              className="bg-background border-border text-foreground text-sm"
            />
          </div>

          {/* Status + Priority row */}
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">Status</Label>
              <Badge className={cn('text-xs', STATUS_BADGE[task.status])}>
                {task.status.replace('_', ' ')}
              </Badge>
            </div>
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">Priority</Label>
              <Select value={editPriority} onValueChange={setEditPriority}>
                <SelectTrigger className="h-7 text-xs bg-background border-border text-foreground">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent className="bg-popover border-border text-popover-foreground">
                  {['high', 'medium', 'low'].map(p => (
                    <SelectItem key={p} value={p} className="text-xs">{p}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          {/* Phase + Execution mode */}
          <div className="grid grid-cols-2 gap-4 text-sm">
            <div className="space-y-1">
              <div className="text-xs text-muted-foreground">Phase</div>
              <div className="text-foreground">{task.phase ?? '—'}</div>
            </div>
            <div className="space-y-1">
              <div className="text-xs text-muted-foreground">Execution</div>
              <div className="text-foreground">{task.execution_mode}</div>
            </div>
          </div>

          {/* Description */}
          <div className="space-y-1.5">
            <Label className="text-xs text-muted-foreground">Description</Label>
            <textarea
              value={editDesc}
              onChange={e => setEditDesc(e.target.value)}
              placeholder="Notes or context…"
              rows={4}
              className="w-full rounded-md border border-border bg-background px-3 py-2 text-sm text-foreground placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring resize-none"
            />
          </div>

          {/* Depends on */}
          {task.depends_on.length > 0 && (
            <>
              <Separator className="bg-border" />
              <div className="space-y-2">
                <div className="text-xs text-muted-foreground font-medium">Depends on</div>
                <div className="flex flex-wrap gap-1.5">
                  {task.depends_on.map(dep => (
                    <Badge key={dep} variant="outline" className="font-mono text-xs border-border text-muted-foreground">
                      {dep}
                    </Badge>
                  ))}
                </div>
              </div>
            </>
          )}

          {/* Artifacts */}
          {task.artifacts.length > 0 && (
            <>
              <Separator className="bg-border" />
              <div className="space-y-2">
                <div className="text-xs text-muted-foreground font-medium">Artifacts</div>
                <div className="space-y-1">
                  {task.artifacts.map((a, i) => (
                    <div key={i} className="flex items-start justify-between gap-2 text-xs">
                      <span className="font-mono text-foreground">{a.key}</span>
                      <div className="text-right space-y-0.5">
                        {a.path && <div className="text-muted-foreground font-mono truncate max-w-50">{a.path}</div>}
                        {a.source && <Badge variant="outline" className="text-[10px] border-border text-muted-foreground">{a.source}</Badge>}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            </>
          )}

          {/* Run info */}
          {(task.run.attempts > 0 || task.run.last_error) && (
            <>
              <Separator className="bg-border" />
              <div className="space-y-2">
                <div className="text-xs text-muted-foreground font-medium">Run</div>
                <div className="text-xs space-y-1">
                  <div className="flex justify-between">
                    <span className="text-muted-foreground">Attempts</span>
                    <span className="text-foreground">{task.run.attempts}</span>
                  </div>
                  {task.run.provider && (
                    <div className="flex justify-between">
                      <span className="text-muted-foreground">Provider</span>
                      <span className="text-foreground">{task.run.provider}</span>
                    </div>
                  )}
                  {task.run.last_error && (
                    <div className="mt-1 space-y-1.5">
                      <div className="px-2 py-1.5 rounded bg-destructive/10 text-destructive text-xs font-mono">
                        {task.run.last_error}
                      </div>
                      {task.status === 'todo' && (
                        <Button
                          variant="ghost"
                          size="xs"
                          onClick={handleDismiss}
                          disabled={dismissing}
                          className="text-muted-foreground hover:text-foreground h-6 px-2 text-xs"
                        >
                          <Ban size={11} className="mr-1" />
                          {dismissing ? 'Dismissing…' : 'Dismiss'}
                        </Button>
                      )}
                    </div>
                  )}
                </div>
              </div>
            </>
          )}

          {/* Next steps for completed content review */}
          {task.type === 'content_review' && task.status === 'done' && (
            <>
              <Separator className="bg-border" />
              <div className="space-y-2">
                <div className="text-xs text-muted-foreground font-medium">Next Steps</div>
                {spawnedTasks.length > 0 ? (
                  <div className="space-y-1.5">
                    <div className="text-xs text-muted-foreground">
                      Apply recommendations task ready:
                    </div>
                    {spawnedTasks.map(t => (
                      <div
                        key={t.id}
                        className="flex items-center gap-2 px-2.5 py-2 rounded-md bg-secondary/60 text-xs"
                      >
                        <span className="w-1.5 h-1.5 rounded-full shrink-0 bg-red-400" />
                        <span className="text-foreground truncate">{t.title ?? t.id}</span>
                        <Badge variant="outline" className="ml-auto shrink-0 text-[10px] border-border text-muted-foreground">
                          {t.priority}
                        </Badge>
                      </div>
                    ))}
                    <div className="flex items-center gap-1 text-xs text-muted-foreground pt-0.5">
                      <ArrowRight size={11} />
                      Close this panel, then run the apply task to edit your files
                    </div>
                  </div>
                ) : (
                  <div className="text-xs text-muted-foreground">
                    All articles are in good health — no optimization tasks needed.
                  </div>
                )}
              </div>
            </>
          )}

          {/* Recommendations preview for content_review_apply tasks */}
          {task.type === 'content_review_apply' && (() => {
            const recArtifact = task.artifacts.find(a => a.key === 'recommendations')
            if (!recArtifact?.content) return null
            let articles: Array<{
              article_id: number
              article_title: string
              article_file: string
              failed_checks: Array<{ check_id: string; label: string }>
              suggestions: Array<{ category: string; current: string; proposed: string; reason: string }>
            }> = []
            try { articles = JSON.parse(recArtifact.content).articles ?? [] } catch { return null }
            if (articles.length === 0) return null
            return <RecommendationsPreview articles={articles} />
          })()}

          {/* Timestamps */}
          <Separator className="bg-border" />
          <div className="text-xs space-y-1 text-muted-foreground">
            <div className="flex justify-between">
              <span>Created</span>
              <span>{formatDate(task.created_at)}</span>
            </div>
            <div className="flex justify-between">
              <span>Updated</span>
              <span>{formatDate(task.updated_at)}</span>
            </div>
          </div>
        </div>
      </ScrollArea>

      {/* Footer actions */}
      <div className="px-5 py-4 border-t border-border space-y-3">
        {isDirty && (
          <Button size="sm" className="w-full" onClick={handleSave} disabled={saving}>
            {saving ? 'Saving…' : 'Save changes'}
          </Button>
        )}

        {execMsg && (
          <div className="flex items-center gap-2 px-3 py-2 rounded-md text-xs bg-emerald-50 text-emerald-700">
            <ArrowRight size={12} />{execMsg}
          </div>
        )}

        {/* Run button for todo/batchable tasks */}
        {task.status === 'todo' && task.execution_mode !== 'manual' && (
          <Button
            size="sm"
            className="w-full"
            onClick={handleExecute}
            disabled={executing}
          >
            {executing ? (
              <><span className="animate-spin mr-1.5">⌛</span>Running…</>
            ) : (
              <><Play size={13} className="mr-1.5" />Run</>
            )}
          </Button>
        )}

        <div className="flex items-center gap-2">
          {!confirmDelete ? (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setConfirmDelete(true)}
              className="text-muted-foreground hover:text-destructive ml-auto"
            >
              <Trash2 size={14} />
            </Button>
          ) : (
            <div className="flex items-center gap-1.5 ml-auto">
              <span className="text-xs text-muted-foreground">Delete?</span>
              <Button
                variant="destructive"
                size="xs"
                onClick={handleDelete}
                disabled={deleting}
              >
                {deleting ? '…' : 'Yes'}
              </Button>
              <Button
                variant="ghost"
                size="xs"
                onClick={() => setConfirmDelete(false)}
                className="text-muted-foreground"
              >
                No
              </Button>
            </div>
          )}
        </div>
      </div>
      </SheetContent>
    </Sheet>
  )
}

// ── RecommendationsPreview ────────────────────────────────────────────────────

interface RecArticle {
  article_id: number
  article_title: string
  article_file: string
  failed_checks: Array<{ check_id: string; label: string }>
  suggestions: Array<{ category: string; current: string; proposed: string; reason: string }>
}

function RecommendationsPreview({ articles }: { articles: RecArticle[] }) {
  const [open, setOpen] = useState<Set<number>>(new Set([articles[0]?.article_id]))

  function toggle(id: number) {
    setOpen(prev => {
      const next = new Set(prev)
      next.has(id) ? next.delete(id) : next.add(id)
      return next
    })
  }

  return (
    <>
      <Separator className="bg-border" />
      <div className="space-y-2">
        <div className="text-xs text-muted-foreground font-medium">
          Recommendations — {articles.length} article{articles.length !== 1 ? 's' : ''}
        </div>
        <div className="space-y-1.5">
          {articles.map(a => (
            <div key={a.article_id} className="rounded-md border border-border overflow-hidden">
              {/* Article header row */}
              <button
                onClick={() => toggle(a.article_id)}
                className="w-full flex items-center justify-between gap-2 px-2.5 py-2 bg-secondary/40 hover:bg-secondary/70 text-left transition-colors"
              >
                <span className="text-xs font-medium text-foreground truncate">{a.article_title}</span>
                <div className="flex items-center gap-1.5 shrink-0">
                  <Badge variant="outline" className="text-[10px] border-border text-muted-foreground">
                    {a.suggestions.length} fix{a.suggestions.length !== 1 ? 'es' : ''}
                  </Badge>
                  <ChevronDown
                    size={12}
                    className={cn('text-muted-foreground transition-transform', open.has(a.article_id) && 'rotate-180')}
                  />
                </div>
              </button>

              {/* Suggestions */}
              {open.has(a.article_id) && (
                <div className="divide-y divide-border">
                  {a.suggestions.map((s, i) => (
                    <div key={i} className="px-2.5 py-2 space-y-1 text-xs">
                      <div className="flex items-center gap-1.5">
                        <Badge variant="outline" className="text-[9px] font-mono border-border text-muted-foreground shrink-0">
                          {s.category}
                        </Badge>
                      </div>
                      {s.current && s.current !== '(missing)' && (
                        <div className="text-muted-foreground line-through opacity-60 leading-relaxed">{s.current}</div>
                      )}
                      <div className="text-foreground leading-relaxed">{s.proposed}</div>
                      <div className="text-muted-foreground italic leading-relaxed opacity-80">{s.reason}</div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
      </div>
    </>
  )
}
