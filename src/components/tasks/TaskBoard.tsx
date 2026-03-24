import { useEffect, useState, useCallback } from 'react'
import { RefreshCw, Upload, Download, Plus, Play, Trash2, X } from 'lucide-react'
import { cn, formatDate } from '../../lib/utils'
import { listTasks, importFromRepo, exportToRepo, analyzeArticleDatePolicy, deleteTask } from '../../lib/tauri'
import type { Task } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Tabs, TabsList, TabsTrigger } from '@/components/ui/tabs'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import {
  Table,
  TableBody,
  TableCell,
  TableFooter,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'
import { TaskDetail } from './TaskDetail'
import { TaskCreate } from './TaskCreate'
import { Sheet, SheetContent } from '@/components/ui/sheet'

const STATUS_TABS = ['all', 'todo', 'in_progress', 'review', 'done'] as const
type StatusFilter = typeof STATUS_TABS[number]

const STATUS_LABELS: Record<StatusFilter, string> = {
  all: 'All',
  todo: 'To-do',
  in_progress: 'In Progress',
  review: 'Review',
  done: 'Done',
}

const STATUS_BADGE: Record<string, string> = {
  todo: 'bg-secondary text-secondary-foreground border-transparent',
  in_progress: 'bg-indigo-100 text-indigo-700 border-transparent',
  review: 'bg-amber-100 text-amber-700 border-transparent',
  done: 'bg-emerald-100 text-emerald-700 border-transparent',
  cancelled: 'bg-secondary text-muted-foreground border-transparent',
}

const PRIORITY_DOT: Record<string, string> = {
  high: 'bg-red-400',
  medium: 'bg-amber-400',
  low: 'bg-slate-400',
}

const PHASE_OPTIONS = ['all', '1-foundation', '2-research', '3-creation', '4-publish', '5-promote']

interface TaskBoardProps {
  projectId?: string
  projectName?: string
  /** If set, auto-open this task as soon as the task list loads. */
  initialTaskId?: string
  /** Called once the task has been opened so the caller can clear the pending id. */
  onTaskOpened?: () => void
  /** Trigger the global task runner drawer with one or more tasks. */
  onRunTasks?: (tasks: Task[]) => void
  /** Changes when a global task queue completes so this board can reload. */
  runCompletedTick?: number
}

export function TaskBoard({
  projectId,
  projectName,
  initialTaskId,
  onTaskOpened,
  onRunTasks,
  runCompletedTick = 0,
}: TaskBoardProps) {
  const [tasks, setTasks] = useState<Task[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('todo')
  const [phaseFilter, setPhaseFilter] = useState('all')
  const [importExportMsg, setImportExportMsg] = useState<string | null>(null)
  const [selectedTask, setSelectedTask] = useState<Task | null>(null)
  const [checkedIds, setCheckedIds] = useState<Set<string>>(new Set())
  const [showCreate, setShowCreate] = useState(false)
  const [deletingSelected, setDeletingSelected] = useState(false)

  const load = useCallback(async () => {
    if (!projectId) return
    setLoading(true)
    setError(null)
    try {
      const data = await listTasks(
        projectId,
        statusFilter !== 'all' ? statusFilter : undefined,
        phaseFilter !== 'all' ? phaseFilter : undefined,
      )
      setTasks(data)
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [projectId, statusFilter, phaseFilter])

  useEffect(() => { load() }, [load])

  useEffect(() => {
    if (!projectId || runCompletedTick === 0) return
    load()
  }, [projectId, runCompletedTick, load])

  // Auto-open a specific task when navigated here from Overview (e.g. after running a workflow).
  useEffect(() => {
    if (!initialTaskId) return

    // If the current filtered list is empty and we're not already on 'all',
    // widen the filter first so the next load can find the task.
    if (tasks.length === 0 && statusFilter !== 'all') {
      setStatusFilter('all')
      return
    }
    if (tasks.length === 0) return

    // First try exact match (e.g. a task navigated to directly)
    const target = tasks.find(t => t.id === initialTaskId)
    if (target) {
      setSelectedTask(target)
      onTaskOpened?.()
      return
    }
    // The target task may be in a different status than the current filter (e.g. 'review'
    // while we're showing 'todo'). Widen to 'all' and reload so we can find it.
    if (statusFilter !== 'all') {
      setStatusFilter('all')
      return
    }
    // Fallback: the initial task may be done (e.g. content_review spawns apply task).
    // Auto-open the first content_review_apply task in the current list instead.
    const applyTask = tasks.find(t => t.type === 'content_review_apply')
    if (applyTask) {
      setSelectedTask(applyTask)
      onTaskOpened?.()
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialTaskId, tasks])

  function handleTaskUpdated(updated: Task) {
    setTasks(prev => prev.map(t => t.id === updated.id ? updated : t))
    // Only update selectedTask / switch tabs if this task's panel is currently open.
    // This prevents a stale async getTask from re-opening a panel the user already closed.
    if (selectedTask?.id === updated.id) {
      setSelectedTask(updated)
      // Auto-switch to the task's new status tab so it stays visible in the list.
      if (
        statusFilter !== 'all' &&
        updated.status !== statusFilter &&
        STATUS_TABS.includes(updated.status as StatusFilter)
      ) {
        setStatusFilter(updated.status as StatusFilter)
      }
    }
  }

  function toggleCheck(id: string, e: React.MouseEvent) {
    e.stopPropagation()
    setCheckedIds(prev => {
      const next = new Set(prev)
      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }
      return next
    })
    // Close detail pane when multi-selecting
    setSelectedTask(null)
  }

  function toggleCheckAll(visible: Task[]) {
    const allChecked = visible.every(t => checkedIds.has(t.id))
    if (allChecked) {
      setCheckedIds(new Set())
    } else {
      setCheckedIds(new Set(visible.map(t => t.id)))
    }
  }

  function handleRunSelected() {
    const toRun = tasks.filter(t => checkedIds.has(t.id) && t.status === 'todo')
    if (toRun.length === 0) return
    setCheckedIds(new Set())
    setSelectedTask(null)
    onRunTasks?.(toRun)
  }

  async function handleDeleteSelected() {
    if (deletingSelected) return

    const selected = tasks.filter(t => checkedIds.has(t.id))
    const deletableSelected = selected.filter(t => t.status === 'todo' || t.status === 'review')
    const nonDeletableCount = selected.length - deletableSelected.length

    if (deletableSelected.length === 0) {
      setImportExportMsg('Only to-do or review tasks can be bulk deleted.')
      return
    }

    const confirmMsg =
      nonDeletableCount > 0
        ? `Delete ${deletableSelected.length} selected to-do/review task${deletableSelected.length !== 1 ? 's' : ''}? (${nonDeletableCount} selected item${nonDeletableCount !== 1 ? 's are' : ' is'} not to-do/review and will be kept.)`
        : `Delete ${deletableSelected.length} selected to-do/review task${deletableSelected.length !== 1 ? 's' : ''}?`

    if (!window.confirm(confirmMsg)) return

    setDeletingSelected(true)
    setImportExportMsg(null)
    try {
      await Promise.all(deletableSelected.map(t => deleteTask(t.id)))
      const deletedIds = new Set(deletableSelected.map(t => t.id))
      setTasks(prev => prev.filter(t => !deletedIds.has(t.id)))
      setCheckedIds(prev => {
        const next = new Set(prev)
        for (const id of deletedIds) next.delete(id)
        return next
      })
      if (selectedTask && deletedIds.has(selectedTask.id)) {
        setSelectedTask(null)
      }
      setImportExportMsg(
        `Deleted ${deletableSelected.length} to-do/review task${deletableSelected.length !== 1 ? 's' : ''}.`
      )
    } catch (e: unknown) {
      setImportExportMsg(`Bulk delete failed: ${String(e)}`)
      await load()
    } finally {
      setDeletingSelected(false)
    }
  }

  function handleTaskDeleted(id: string) {
    setTasks(prev => prev.filter(t => t.id !== id))
    setSelectedTask(null)
  }

  function handleTaskCreated(task: Task) {
    setTasks(prev => [task, ...prev])
    setShowCreate(false)
    setSelectedTask(task)
  }

  async function handleImport() {
    if (!projectId) return
    setImportExportMsg(null)
    try {
      const result = await importFromRepo(projectId)
      setImportExportMsg(`Imported: ${result.tasks_imported} tasks, ${result.articles_imported} articles`)
      await load()
    } catch (e: unknown) {
      setImportExportMsg(`Import failed: ${String(e)}`)
    }
  }

  async function handleExport() {
    if (!projectId) return
    setImportExportMsg(null)
    try {
      const report = await analyzeArticleDatePolicy(projectId, ['published', 'ready_to_publish'], 0)
      if (report.issues.length > 0) {
        const first = report.issues[0]
        setImportExportMsg(
          `Export blocked by date policy: ${report.issues.length} issue(s). First: article ${first.article_id} ${first.issue_type.replace(/_/g, ' ')}.`
        )
        return
      }
      await exportToRepo(projectId)
      setImportExportMsg('Exported successfully')
    } catch (e: unknown) {
      setImportExportMsg(`Export failed: ${String(e)}`)
    }
  }

  function handleRunBatch() {
    const toRun = tasks.filter(t => t.status === 'todo' && t.execution_mode !== 'manual')
    if (toRun.length === 0) return
    setCheckedIds(new Set())
    setSelectedTask(null)
    onRunTasks?.(toRun)
  }

  if (!projectId) {
    return (
      <div className="flex items-center justify-center h-64 text-sm text-muted-foreground">
        Select a project to view tasks.
      </div>
    )
  }

  return (
    <div className="flex h-full">
      {/* Main list pane */}
      <div className="flex-1 min-w-0 p-6 overflow-y-auto">
        {/* Header */}
        <div className="flex items-center justify-between mb-6">
          <h1 className="text-lg font-semibold text-foreground">Tasks</h1>
          <div className="flex items-center gap-2">
            <Button variant="outline" size="sm" onClick={handleImport} className="border-border text-muted-foreground hover:text-foreground">
              <Download size={14} />
              Import JSON
            </Button>
            <Button variant="outline" size="sm" onClick={handleExport} className="border-border text-muted-foreground hover:text-foreground">
              <Upload size={14} />
              Export JSON
            </Button>
            <Button variant="ghost" size="icon-sm" onClick={load} disabled={loading} className="text-muted-foreground">
              <RefreshCw size={14} className={loading ? 'animate-spin' : ''} />
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={handleRunBatch}
              className="border-border text-muted-foreground hover:text-foreground"
            >
              <Play size={14} />
              Run All
            </Button>
            <Button size="sm" onClick={() => setShowCreate(true)}>
              <Plus size={14} />
              New Task
            </Button>
          </div>
        </div>

        {importExportMsg && (
          <div className="mb-4 px-3 py-2 rounded-md text-sm border border-border text-muted-foreground bg-card">
            {importExportMsg}
          </div>
        )}

        {/* Selection action bar */}
        {checkedIds.size > 0 && (
          <div className="mb-4 flex items-center gap-3 px-3 py-2 rounded-md bg-primary/5 border border-primary/20 text-sm">
            <span className="text-foreground font-medium">{checkedIds.size} selected</span>
            <Button
              size="xs"
              onClick={handleRunSelected}
              className="bg-primary text-primary-foreground hover:bg-primary/90 text-xs"
            >
              <><Play size={12} className="mr-1" />Run {checkedIds.size} task{checkedIds.size !== 1 ? 's' : ''}</>
            </Button>
            <Button
              size="xs"
              variant="destructive"
              onClick={handleDeleteSelected}
              disabled={deletingSelected}
              className="text-xs"
            >
              <><Trash2 size={12} className="mr-1" />{deletingSelected ? 'Deleting...' : 'Delete selected'}</>
            </Button>
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => setCheckedIds(new Set())}
              className="ml-auto text-muted-foreground"
            >
              <X size={14} />
            </Button>
          </div>
        )}

        {/* Filters */}
        <div className="flex items-center gap-4 mb-4">
          <Tabs value={statusFilter} onValueChange={v => setStatusFilter(v as StatusFilter)}>
            <TabsList className="bg-card border border-border">
              {STATUS_TABS.map(s => (
                <TabsTrigger key={s} value={s} className="text-xs data-[state=active]:bg-primary data-[state=active]:text-primary-foreground">
                  {STATUS_LABELS[s]}
                </TabsTrigger>
              ))}
            </TabsList>
          </Tabs>

          <Select value={phaseFilter} onValueChange={setPhaseFilter}>
            <SelectTrigger className="w-40 h-8 text-xs bg-card border-border text-muted-foreground">
              <SelectValue />
            </SelectTrigger>
            <SelectContent className="bg-popover border-border text-popover-foreground">
              {PHASE_OPTIONS.map(p => (
                <SelectItem key={p} value={p} className="text-xs">{p === 'all' ? 'All phases' : p}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {error && (
          <div className="mb-4 px-3 py-2 rounded-md text-sm bg-destructive/15 text-destructive">
            {error}
          </div>
        )}

        {/* Task table */}
        <div className="rounded-lg border border-border overflow-hidden">
          <Table>
            <TableHeader>
              <TableRow className="bg-card hover:bg-card border-border">
                <TableHead className="w-8 pl-3">
                  {(() => {
                    const visible = tasks.filter(t => statusFilter !== 'all' || t.status !== 'cancelled')
                    const allChecked = visible.length > 0 && visible.every(t => checkedIds.has(t.id))
                    const someChecked = visible.some(t => checkedIds.has(t.id))
                    return (
                      <input
                        type="checkbox"
                        checked={allChecked}
                        ref={el => { if (el) el.indeterminate = someChecked && !allChecked }}
                        onChange={() => toggleCheckAll(visible)}
                        className="w-3.5 h-3.5 cursor-pointer accent-primary"
                      />
                    )
                  })()}
                </TableHead>
                <TableHead className="text-xs text-muted-foreground w-32">Type</TableHead>
                <TableHead className="text-xs text-muted-foreground">Title</TableHead>
                <TableHead className="text-xs text-muted-foreground w-36">Phase</TableHead>
                <TableHead className="text-xs text-muted-foreground w-20">Priority</TableHead>
                <TableHead className="text-xs text-muted-foreground w-28">Status</TableHead>
                <TableHead className="text-xs text-muted-foreground w-28">Updated</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {loading && tasks.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={8} className="py-10 text-center text-xs text-muted-foreground">
                    Loading…
                  </TableCell>
                </TableRow>
              ) : tasks.length === 0 ? (
                <TableRow>
                  <TableCell colSpan={8} className="py-12">
                    {statusFilter !== 'all' || phaseFilter !== 'all' ? (
                      <div className="flex flex-col items-center gap-2 text-center">
                        <p className="text-sm text-muted-foreground">No tasks match the current filters.</p>
                      </div>
                    ) : (
                      <div className="flex flex-col items-center gap-3 text-center">
                        <p className="text-sm font-medium text-foreground">No tasks yet</p>
                        <p className="text-xs text-muted-foreground max-w-xs">
                          Import tasks from your repository's{' '}
                          <code className="font-mono">.github/automation/task_list.json</code>
                        </p>
                        <Button size="sm" onClick={handleImport}>
                          <Download size={14} />
                          Import from repository
                        </Button>
                      </div>
                    )}
                  </TableCell>
                </TableRow>
              ) : (
                tasks
                  .filter(t => statusFilter !== 'all' || t.status !== 'cancelled')
                  .map(task => {
                  const isSelected = selectedTask?.id === task.id
                  const isChecked = checkedIds.has(task.id)
                  return (
                    <TableRow
                      key={task.id}
                      className={cn(
                        'border-border cursor-pointer',
                        isChecked ? 'bg-primary/5' : isSelected ? 'bg-accent/40' : 'hover:bg-accent/20',
                      )}
                      onClick={() => {
                        if (checkedIds.size > 0) {
                          toggleCheck(task.id, { stopPropagation: () => {} } as React.MouseEvent)
                        } else {
                          setSelectedTask(isSelected ? null : task)
                        }
                      }}
                    >
                      <TableCell className="pl-3" onClick={e => toggleCheck(task.id, e)}>
                        <input
                          type="checkbox"
                          checked={isChecked}
                          onChange={() => {}}
                          className="w-3.5 h-3.5 cursor-pointer accent-primary"
                        />
                      </TableCell>
                      <TableCell>
                        <Badge variant="secondary" className="font-mono text-xs">
                          {task.type}
                        </Badge>
                      </TableCell>
                      <TableCell>
                        <div className="font-medium text-sm text-foreground max-w-xs truncate">
                          {task.title}
                        </div>
                        {task.article_slug && (
                          <div className="text-xs mt-0.5 text-muted-foreground truncate">
                            {task.article_slug}
                          </div>
                        )}
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {task.phase ?? '—'}
                      </TableCell>
                      <TableCell>
                        <span className={cn('inline-block w-2 h-2 rounded-full', PRIORITY_DOT[task.priority ?? 'medium'])} />
                      </TableCell>
                      <TableCell>
                        <Badge className={cn('text-xs', STATUS_BADGE[task.status])}>
                          {task.status.replace('_', ' ')}
                        </Badge>
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {formatDate(task.updated_at)}
                      </TableCell>
                    </TableRow>
                  )
                })
              )}
            </TableBody>
            {tasks.length > 0 && (
              <TableFooter className="bg-card border-border">
                <TableRow>
                  <TableCell colSpan={8} className="py-2.5 text-xs text-muted-foreground">
                    {tasks.length} task{tasks.length !== 1 ? 's' : ''}
                    {checkedIds.size > 0 && ` · ${checkedIds.size} selected`}
                  </TableCell>
                </TableRow>
              </TableFooter>
            )}
          </Table>
        </div>
      </div>

      {/* Detail sheet */}
      <Sheet
        open={!!selectedTask}
        onOpenChange={(open) => {
          if (!open) {
            // Reload when closing a completed content_review so spawned tasks appear.
            if (selectedTask?.type === 'content_review' && selectedTask?.status === 'done') {
              load()
            }
            setSelectedTask(null)
          }
        }}
      >
        <SheetContent side="right" className="w-[min(100vw,42rem)] max-w-[100vw] p-0 overflow-hidden [&>button:last-child]:hidden">
          {selectedTask && (
            <TaskDetail
              task={selectedTask}
              projectName={projectName}
              onClose={() => setSelectedTask(null)}
              onUpdated={handleTaskUpdated}
              onDeleted={handleTaskDeleted}
              onArticleTasksCreated={(newTasks) => {
                setStatusFilter('todo')
                setPhaseFilter('all')

                // Ensure newly created write_article tasks are visible immediately,
                // then pre-select them so the user can run them right away.
                setTasks(prev => {
                  const byId = new Map(prev.map(t => [t.id, t]))
                  for (const t of newTasks) byId.set(t.id, t)
                  return Array.from(byId.values())
                })
                setCheckedIds(new Set(newTasks.map(t => t.id)))
              }}
            />
          )}
        </SheetContent>
      </Sheet>

      {/* Create modal */}
      {showCreate && projectId && (
        <TaskCreate
          projectId={projectId}
          onClose={() => setShowCreate(false)}
          onCreated={handleTaskCreated}
        />
      )}
    </div>
  )
}
