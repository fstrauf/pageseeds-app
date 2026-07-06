import { useEffect, useState } from 'react'
import { RefreshCw, Upload, Download, Plus, Play, Trash2, X, AlertCircle } from 'lucide-react'
import { useRef } from 'react'
import { cn, formatDate } from '../../lib/utils'
import { listTasks, getTask, importFromRepo, exportToRepo, analyzeArticleDatePolicy, deleteTask } from '../../lib/tauri'
import type { Project, Task } from '../../lib/types'
import { canEnqueue } from '../../lib/taskCapabilities'
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
import { useQueue } from '../../lib/queue-context'
import { ListPlus } from 'lucide-react'
import { useErrorHandler } from '../../lib/toast-context'
import { useQuery, useMutation } from '../../hooks/useQuery'

const STATUS_TABS = ['all', 'todo', 'in_progress', 'review', 'done', 'failed'] as const
type StatusFilter = typeof STATUS_TABS[number]

const STATUS_LABELS: Record<StatusFilter, string> = {
  all: 'All',
  todo: 'To-do',
  in_progress: 'In Progress',
  review: 'Review',
  done: 'Done',
  failed: 'Failed',
}

// Helper to check if a task status matches the filter
function statusMatchesFilter(taskStatus: string, filter: StatusFilter): boolean {
  if (filter === 'all') return taskStatus !== 'cancelled'
  if (filter === 'todo') return taskStatus === 'todo' || taskStatus === 'queued'
  return taskStatus === filter
}

const STATUS_BADGE: Record<string, string> = {
  todo: 'bg-secondary text-secondary-foreground border-transparent',
  queued: 'bg-blue-100 text-blue-700 border-transparent',
  in_progress: 'bg-indigo-100 text-indigo-700 border-transparent',
  review: 'bg-amber-100 text-amber-700 border-transparent',
  done: 'bg-emerald-100 text-emerald-700 border-transparent',
  cancelled: 'bg-secondary text-muted-foreground border-transparent',
  failed: 'bg-red-100 text-red-700 border-transparent',
}

const PRIORITY_DOT: Record<string, string> = {
  high: 'bg-red-400',
  medium: 'bg-amber-400',
  low: 'bg-slate-400',
}

const PHASE_OPTIONS = ['all', 'collection', 'investigation', 'research', 'implementation', 'verification']

const PHASE_LABELS: Record<string, string> = {
  all: 'All phases',
  collection: 'Collection',
  investigation: 'Investigation',
  research: 'Research',
  implementation: 'Implementation',
  verification: 'Verification',
}

const EMPTY_TASKS: Task[] = []

interface TaskBoardProps {
  projectId?: string
  projectName?: string
  project?: Project
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
  project,
  initialTaskId,
  onTaskOpened,
  onRunTasks,
  runCompletedTick = 0,
}: TaskBoardProps) {
  const { showError } = useErrorHandler()
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('todo')
  const [phaseFilter, setPhaseFilter] = useState('all')
  const [importExportMsg, setImportExportMsg] = useState<string | null>(null)
  const [selectedTask, setSelectedTask] = useState<Task | null>(null)
  const [checkedIds, setCheckedIds] = useState<Set<string>>(new Set())
  const [showCreate, setShowCreate] = useState(false)
  const [deletingSelected, setDeletingSelected] = useState(false)
  const queue = useQueue()

  const { data: fetchedTasks, error, isLoading: loading, refetch } = useQuery(
    `tasks-${projectId}-${statusFilter}-${phaseFilter}`,
    () => projectId ? listTasks(
      projectId,
      statusFilter !== 'all' ? statusFilter : undefined,
      phaseFilter !== 'all' ? phaseFilter : undefined,
    ) : Promise.resolve([]),
    { enabled: !!projectId, staleTime: 0 }
  )

  const tasks = fetchedTasks ?? EMPTY_TASKS

  useEffect(() => {
    if (error) {
      showError(error.message)
    }
  }, [error, showError])

  useEffect(() => {
    if (!projectId || runCompletedTick === 0) return
    refetch()
  }, [projectId, runCompletedTick, refetch])

  // Track the last initialTaskId we directly fetched so we don't loop.
  const fetchedTaskIdRef = useRef<string | null>(null)
  // Guard to ensure we only process each initialTaskId once, even if tasks
  // array refetches and causes the effect to re-run.
  const processedInitialTaskIdRef = useRef<string | null>(null)

  // Auto-open a specific task when navigated here from Overview or the task runner.
  useEffect(() => {
    if (!initialTaskId) return
    if (processedInitialTaskIdRef.current === initialTaskId) return
    processedInitialTaskIdRef.current = initialTaskId

    console.log('[TaskBoard] auto-open effect running, initialTaskId:', initialTaskId, 'tasks count:', tasks.length, 'statusFilter:', statusFilter)

    // Already in the current list? Open immediately.
    const target = tasks.find(t => t.id === initialTaskId)
    if (target) {
      console.log('[TaskBoard] found target in tasks list, opening:', target.id)
      setSelectedTask(target)
      onTaskOpened?.()
      fetchedTaskIdRef.current = null
      return
    }

    // Not in current filtered list. Widen to 'all' so the background list reloads,
    // and also fetch the task directly so the panel opens right away.
    if (statusFilter !== 'all') {
      console.log('[TaskBoard] widening statusFilter to all')
      setStatusFilter('all')
    }

    if (fetchedTaskIdRef.current !== initialTaskId) {
      console.log('[TaskBoard] fetching task directly:', initialTaskId)
      fetchedTaskIdRef.current = initialTaskId
      getTask(initialTaskId)
        .then(task => {
          console.log('[TaskBoard] getTask succeeded, opening:', task.id)
          setSelectedTask(task)
          onTaskOpened?.()
        })
        .catch((err) => {
          console.log('[TaskBoard] getTask failed:', err)
          fetchedTaskIdRef.current = null
          // Direct fetch failed; the background list reload may still pick it up.
        })
    }
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialTaskId, tasks, statusFilter])

  function handleTaskUpdated(updated: Task) {
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
    refetch()
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
    const toRun = tasks.filter(t => checkedIds.has(t.id) && (t.status === 'todo' || t.status === 'review' || t.status === 'failed') && canEnqueue(t))
    if (toRun.length === 0) return
    setCheckedIds(new Set())
    setSelectedTask(null)
    onRunTasks?.(toRun)
  }

  function handleAddToQueue() {
    const toQueue = tasks.filter(t => checkedIds.has(t.id) && (t.status === 'todo' || t.status === 'review' || t.status === 'failed') && canEnqueue(t))
    if (toQueue.length === 0) return
    
    queue.enqueue(
      toQueue.map(t => ({
        taskId: t.id,
        projectId: projectId!,
        projectName: projectName || 'Unknown',
        title: t.title || t.type,
        taskType: t.type,
        status: 'pending' as const,
      }))
    )
    
    setCheckedIds(new Set())
    setSelectedTask(null)
  }

  const deleteMutation = useMutation(
    async (ids: string[]) => {
      await Promise.all(ids.map(id => deleteTask(id)))
    },
    {
      invalidateQueries: 'tasks-',
      onSuccess: (_, ids) => {
        setImportExportMsg(`Deleted ${ids.length} task${ids.length !== 1 ? 's' : ''}.`)
      },
      onError: (error) => {
        showError(`Bulk delete failed: ${error.message}`)
        refetch()
      },
    }
  )

  async function handleDeleteSelected() {
    if (deletingSelected) return

    const selected = tasks.filter(t => checkedIds.has(t.id))
    const deletableSelected = selected.filter(t => t.status === 'todo' || t.status === 'review' || t.status === 'failed')
    const nonDeletableCount = selected.length - deletableSelected.length

    if (deletableSelected.length === 0) {
      showError('Only to-do, review, or failed tasks can be bulk deleted.')
      return
    }

    const confirmMsg =
      nonDeletableCount > 0
        ? `Delete ${deletableSelected.length} selected task${deletableSelected.length !== 1 ? 's' : ''}? (${nonDeletableCount} selected item${nonDeletableCount !== 1 ? 's are' : ' is'} not to-do/review/failed and will be kept.)`
        : `Delete ${deletableSelected.length} selected task${deletableSelected.length !== 1 ? 's' : ''}?`

    if (!window.confirm(confirmMsg)) return

    setDeletingSelected(true)
    setImportExportMsg(null)
    try {
      await deleteMutation.mutate(deletableSelected.map(t => t.id))
      setCheckedIds(prev => {
        const next = new Set(prev)
        for (const id of deletableSelected.map(t => t.id)) next.delete(id)
        return next
      })
      if (selectedTask && deletableSelected.some(t => t.id === selectedTask.id)) {
        setSelectedTask(null)
      }
    } finally {
      setDeletingSelected(false)
    }
  }

  function handleTaskDeleted(_id: string) {
    void _id
    setSelectedTask(null)
    refetch()
  }

  function handleTaskCreated(task: Task) {
    setShowCreate(false)
    setSelectedTask(task)
    refetch()
  }

  const importMutation = useMutation(
    async () => {
      if (!projectId) throw new Error('No project selected')
      return await importFromRepo(projectId)
    },
    {
      invalidateQueries: 'tasks-',
      onSuccess: (result) => {
        setImportExportMsg(`Imported: ${result.tasks_imported} tasks, ${result.articles_imported} articles`)
      },
      onError: (error) => {
        showError(`Import failed: ${error.message}`)
      },
    }
  )

  async function handleImport() {
    if (!projectId) return
    setImportExportMsg(null)
    await importMutation.mutate()
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
      showError(`Export failed: ${String(e)}`)
    }
  }

  function handleRunBatch() {
    const toRun = tasks.filter(t => (t.status === 'todo' || t.status === 'review' || t.status === 'failed'))
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
            <Button variant="ghost" size="icon-sm" onClick={refetch} disabled={loading} className="text-muted-foreground">
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
              variant="outline"
              onClick={handleAddToQueue}
              className="text-xs border-border"
            >
              <><ListPlus size={12} className="mr-1" />Add to queue</>
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
                <SelectItem key={p} value={p} className="text-xs">{PHASE_LABELS[p]}</SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

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
                  .filter(t => statusMatchesFilter(t.status, statusFilter))
                  .map(task => {
                  const isSelected = selectedTask?.id === task.id
                  const isChecked = checkedIds.has(task.id)
                  return (
                    <TableRow
                      key={task.id}
                      data-task-id={task.id}
                      className={cn(
                        'border-border cursor-pointer',
                        isChecked ? 'bg-primary/5' : isSelected ? 'bg-accent/40' : 'hover:bg-accent/20',
                      )}
                      onClick={() => {
                        if (checkedIds.size > 0) {
                          toggleCheck(task.id, { stopPropagation: () => {} } as React.MouseEvent)
                        } else {
                          // Always select the clicked task by ID to avoid stale closure issues
                          const clickedTask = tasks.find(t => t.id === task.id)
                          setSelectedTask(prev => prev?.id === task.id ? null : (clickedTask ?? task))
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
                      </TableCell>
                      <TableCell className="text-xs text-muted-foreground">
                        {task.phase ?? '—'}
                      </TableCell>
                      <TableCell>
                        <span className={cn('inline-block w-2 h-2 rounded-full', PRIORITY_DOT[task.priority ?? 'medium'])} />
                      </TableCell>
                      <TableCell>
                        <div className="flex items-center gap-1.5">
                          <Badge className={cn('text-xs', STATUS_BADGE[task.status])}>
                            {task.status.replace('_', ' ')}
                          </Badge>
                          {task.run.attempts > 0 && task.run.last_error && (
                            <span title="Last run failed">
                              <AlertCircle size={12} className="text-destructive shrink-0" />
                            </span>
                          )}
                        </div>
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
              refetch()
            }
            setSelectedTask(null)
            // Clear the pending task ID so subsequent "Open task" clicks work.
            onTaskOpened?.()
          }
        }}
      >
        <SheetContent side="right" className="w-[min(100vw,42rem)] max-w-[100vw] p-0 overflow-hidden [&>button:last-child]:hidden">
          {selectedTask && (
            <TaskDetail
              task={selectedTask}
              projectName={projectName}
              project={project}
              onClose={() => {
                setSelectedTask(null)
                onTaskOpened?.()
              }}
              onUpdated={handleTaskUpdated}
              onDeleted={handleTaskDeleted}
              onArticleTasksCreated={(newTasks) => {
                setStatusFilter('todo')
                setPhaseFilter('all')
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
