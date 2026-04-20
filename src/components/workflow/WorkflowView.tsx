import { useState, useEffect, useCallback } from 'react'
import { Play, CheckCircle2 } from 'lucide-react'
import { listTasks } from '../../lib/tauri'
import { useQueue } from '../../lib/queue-context'
import { useErrorHandler } from '../../lib/toast-context'
import type { Task } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { ScrollArea } from '@/components/ui/scroll-area'
import { cn } from '../../lib/utils'

interface WorkflowViewProps {
  projectId: string
  projectName?: string}

export function WorkflowView({ projectId, projectName }: WorkflowViewProps) {
  const [tasks, setTasks] = useState<Task[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [running, setRunning] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const [queuedMsg, setQueuedMsg] = useState<string | null>(null)
  const queue = useQueue()
  const { showError } = useErrorHandler()
  
  const load = useCallback(async () => {
    try {
      const data = await listTasks(projectId, 'todo')
      setTasks(data)
      setLoaded(true)
    } catch (e: unknown) {
      showError(String(e))
    }
  }, [projectId, showError])

  // Listen for queue completion to refresh task list
  const [lastQueueActive, setLastQueueActive] = useState(queue.isActive)
  useEffect(() => {
    if (lastQueueActive && !queue.isActive) {
      // Queue was active and is now inactive - refresh the list
      load()
      setQueuedMsg(null)
    }
    setLastQueueActive(queue.isActive)
  }, [queue.isActive, lastQueueActive, load])

  async function run() {
    if (!selected) return
    const task = tasks.find(t => t.id === selected)
    if (!task) return
    
    setRunning(true)
    setQueuedMsg(null)
    try {
      // Add to queue instead of direct execution
      queue.enqueue([{
        taskId: task.id,
        projectId: task.project_id,
        projectName: projectName,
        title: task.title ?? task.type ?? 'Untitled',
        taskType: task.type ?? '',
        status: 'pending',
      }])
      setQueuedMsg(`Task added to queue. Check the TaskRunner panel for progress.`)
    } catch (e: unknown) {
      showError(String(e))
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

        {queuedMsg && (
          <div className="rounded border border-primary/30 bg-primary/5 px-3 py-2 text-sm text-foreground">
            <div className="flex items-center gap-2">
              <CheckCircle2 size={16} className="text-green-500" />
              <span>{queuedMsg}</span>
            </div>
          </div>
        )}
      </div>
    </ScrollArea>
  )
}
