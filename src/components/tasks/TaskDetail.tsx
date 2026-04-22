import { useState, useEffect } from 'react'
import { Trash2, AlertCircle, Ban, ArrowRight, Play, ChevronDown, X } from 'lucide-react'
import { updateTask, deleteTask, cancelTask, listTasks, getTask } from '../../lib/tauri'
import { useQueue } from '../../lib/queue-context'
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
import { Textarea } from '@/components/ui/textarea'
import {
  SheetHeader,
  SheetTitle,
  SheetDescription,
  SheetFooter,
  SheetClose,
} from '@/components/ui/sheet'
import { KeywordPicker } from './KeywordPicker'
import { RedditOpportunityPicker } from './RedditOpportunityPicker'
import { ErrorExplainer } from './error-explainer'

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
  projectName?: string
  /** Called when keywords are selected and write_article tasks have been created. */
  onArticleTasksCreated?: (tasks: Task[]) => void
}

export function TaskDetail({ task, onClose, onUpdated, onDeleted, onArticleTasksCreated, projectName }: TaskDetailProps) {
  const { enqueue } = useQueue()
  const [editTitle, setEditTitle] = useState(task.title ?? '')
  const [editDesc, setEditDesc] = useState(task.description ?? '')
  const [editPriority, setEditPriority] = useState(task.priority ?? 'medium')
  const [saving, setSaving] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [confirmDelete, setConfirmDelete] = useState(false)
  const [dismissing, setDismissing] = useState(false)
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

  function handleEnqueue() {
    enqueue([{
      taskId: task.id,
      projectId: task.project_id,
      title: task.title ?? task.type ?? 'Untitled',
      taskType: task.type ?? '',
      projectName,
    }])
  }

  return (
    <div className="h-full flex flex-col overflow-hidden overflow-x-hidden">
      {/* Header */}
      <SheetHeader className="shrink-0 flex-row items-center gap-2 px-5 py-4 border-b border-border min-w-0">
        <Badge variant="secondary" className="font-mono text-xs shrink-0">
          {task.type}
        </Badge>
        <SheetTitle className="text-xs text-muted-foreground truncate font-mono font-normal flex-1">
          {task.id}
        </SheetTitle>
        <SheetDescription className="sr-only">{task.type} task details</SheetDescription>
        <SheetClose asChild>
          <Button variant="ghost" size="icon-sm" className="text-muted-foreground shrink-0">
            <X size={14} />
          </Button>
        </SheetClose>
      </SheetHeader>

      <div className="flex-1 min-h-0 min-w-0 overflow-y-auto overflow-x-hidden">
        <div className="px-5 py-5 space-y-5 min-w-0">
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
              <Select value={editPriority} onValueChange={value => setEditPriority(value as 'high' | 'medium' | 'low')}>
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
            <Label className="text-xs text-muted-foreground">
              {(task.type === 'research_keywords' || task.type === 'custom_keyword_research') ? 'Keyword Themes' : 'Description'}
            </Label>
            <Textarea
              value={editDesc}
              onChange={e => setEditDesc(e.target.value)}
              placeholder={(task.type === 'research_keywords' || task.type === 'custom_keyword_research')
                ? 'Enter themes, one per line\nExample:\ncontent marketing\nSEO tools\nblog writing tips'
                : 'Notes or context…'
              }
              rows={4}
              className="bg-background border-border text-foreground text-sm resize-none"
            />
            {(task.type === 'research_keywords' || task.type === 'custom_keyword_research') && task.status === 'todo' && (
              <p className="text-[11px] text-muted-foreground leading-relaxed">
                Themes drive the keyword search. Enter 2–5 topics related to your site — one per line or comma-separated.
              </p>
            )}
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
                        {a.path && (
                          <div className="text-muted-foreground font-mono truncate max-w-48 sm:max-w-64">
                            {a.path}
                          </div>
                        )}
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
                    <div className="mt-2 space-y-2">
                      <ErrorExplainer
                        error={task.run.last_error}
                        taskType={task.type}
                        onRetry={task.status === 'todo' || task.status === 'in_progress' ? handleEnqueue : undefined}
                      />
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

          {/* Keyword picker — shown when keyword research task is in review status */}
          {(task.type === 'research_keywords' || task.type === 'custom_keyword_research' || task.type === 'research_landing_pages') && task.status === 'review' && (
            <>
              <Separator className="bg-border" />
              <div className="space-y-2">
                <div className="text-xs text-muted-foreground font-medium">
                  {task.type === 'research_landing_pages' ? 'Landing Page Keyword Results' : 'Keyword Results'}
                </div>
                <p className="text-xs text-muted-foreground">
                  {task.type === 'research_landing_pages' 
                    ? 'Select the keywords you want to create landing pages for, then click "Create Landing Page Tasks".'
                    : 'Select the keywords you want to write articles for, then click "Create Article Tasks".'}
                </p>
                <KeywordPicker
                  task={task}
                  onTasksCreated={newTasks => {
                    // Signal parent to switch to the todo tab (where new tasks will appear).
                    // Then close the panel. The async getTask below updates the task in the
                    // list to 'done' but won't reopen the panel (handleTaskUpdated guards this).
                    onArticleTasksCreated?.(newTasks)
                    onClose()
                    getTask(task.id).then(refreshed => onUpdated(refreshed)).catch(() => {})
                  }}
                />
              </div>
            </>
          )}

          {/* Reddit opportunity picker — shown when Reddit search is in review status */}
          {task.type === 'reddit_opportunity_search' && task.status === 'review' && (
            <>
              <Separator className="bg-border" />
              <div className="space-y-2">
                <div className="text-xs text-muted-foreground font-medium">Reddit Opportunities</div>
                <p className="text-xs text-muted-foreground">
                  Select the opportunities you want to reply to, then click "Create Reply Tasks".
                </p>
                <RedditOpportunityPicker
                  task={task}
                  onTasksCreated={newTasks => {
                    // Signal parent to switch to the todo tab (where new tasks will appear).
                    onArticleTasksCreated?.(newTasks)
                    onClose()
                    getTask(task.id).then(refreshed => onUpdated(refreshed)).catch(() => {})
                  }}
                />
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
      </div>

      {/* Footer actions */}
      <SheetFooter className="shrink-0 px-5 py-4 border-t border-border flex-col gap-3">
        {isDirty && (
          <Button size="sm" className="w-full" onClick={handleSave} disabled={saving}>
            {saving ? 'Saving…' : 'Save changes'}
          </Button>
        )}

        {/* Queue button for todo/batchable tasks, and re-run for stuck in_progress tasks */}
        {(task.status === 'todo' || task.status === 'in_progress') && task.execution_mode !== 'manual' && (
          <Button
            size="sm"
            className="w-full"
            onClick={handleEnqueue}
          >
            {task.status === 'in_progress' ? (
              <><Play size={13} className="mr-1.5" />Re-run</>
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
      </SheetFooter>
    </div>
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
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
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
