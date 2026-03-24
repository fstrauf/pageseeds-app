import { useState, useRef, useEffect } from 'react'
import { listen } from '@tauri-apps/api/event'
import { executeTask } from '../lib/tauri'
import type { QueueItem, RunnerItem, TaskStepEvent, StepProgress } from '../lib/types'

/**
 * Manages all global queue state and the sequential execution loop.
 *
 * Design notes:
 * - Items state is updated via a functional updater that also writes itemsRef.current
 *   synchronously, so the async loop always sees the latest items without waiting
 *   for a React re-render.
 * - Pause is implemented via a Promise that resolves when resume() is called.
 * - Follow-up tasks with execution_mode !== 'manual' are auto-appended after
 *   their parent task completes.
 * - Tauri 'task_step_progress' events update liveSteps during task execution
 *   for live per-step feedback in the runner UI.
 */
export function useQueueRunner(onCompleted: () => void) {
  const [items, setItems] = useState<RunnerItem[]>([])
  const [isRunning, setIsRunning] = useState(false)
  const [isPaused, setIsPaused] = useState(false)
  const [isVisible, setIsVisible] = useState(false)

  const itemsRef = useRef<RunnerItem[]>([])
  const loopRunningRef = useRef(false)
  const isPausedRef = useRef(false)
  // Resolved when resume() is called while the loop is waiting between tasks.
  const resumeCallbackRef = useRef<(() => void) | null>(null)

  // Keep isPausedRef in sync with state so the async loop can read it without
  // stale closure issues.
  useEffect(() => {
    isPausedRef.current = isPaused
  }, [isPaused])

  /**
   * Update items state AND itemsRef atomically inside the functional updater.
   * This ensures itemsRef.current is always current even before React re-renders,
   * which is critical for the async execution loop.
   */
  function updateItems(updater: (prev: RunnerItem[]) => RunnerItem[]) {
    setItems(prev => {
      const next = updater(prev)
      itemsRef.current = next
      return next
    })
  }

  // Subscribe to per-step Tauri events emitted by the Rust executor.
  useEffect(() => {
    let unlisten: (() => void) | null = null
    listen<TaskStepEvent>('task_step_progress', event => {
      const { task_id, step_name, status, message } = event.payload
      updateItems(prev =>
        prev.map(it => {
          if (it.task.id !== task_id) return it
          const liveSteps: StepProgress[] = [...(it.liveSteps ?? [])]
          const idx = liveSteps.findIndex(s => s.step_name === step_name)
          const updated: StepProgress = {
            step_name,
            kind: '',
            status: status as StepProgress['status'],
            message,
          }
          if (idx >= 0) {
            liveSteps[idx] = updated
          } else {
            liveSteps.push(updated)
          }
          return { ...it, liveSteps }
        }),
      )
    }).then(fn => {
      unlisten = fn
    })
    return () => {
      unlisten?.()
    }
  }, [])

  async function runLoop() {
    if (loopRunningRef.current) return
    loopRunningRef.current = true
    setIsRunning(true)

    while (true) {
      // Pause between tasks — wait until resume() resolves the callback.
      if (isPausedRef.current) {
        await new Promise<void>(resolve => {
          resumeCallbackRef.current = resolve
        })
        resumeCallbackRef.current = null
      }

      const nextTask = itemsRef.current.find(it => it.status === 'queued')
      if (!nextTask) break

      const taskId = nextTask.task.id
      updateItems(prev =>
        prev.map(it =>
          it.task.id === taskId ? { ...it, status: 'running', liveSteps: [] } : it,
        ),
      )

      try {
        const result = await executeTask(taskId)

        updateItems(prev =>
          prev.map(it =>
            it.task.id === taskId
              ? { ...it, status: result.success ? 'done' : 'failed', result }
              : it,
          ),
        )

        // Auto-queue runnable follow-ups so multi-step workflows continue
        // without manual intervention.
        if (result.follow_up_tasks?.length) {
          const autoRun = result.follow_up_tasks.filter(
            fu => fu.status === 'todo' && fu.execution_mode !== 'manual',
          )
          if (autoRun.length > 0) {
            const followUpItems: RunnerItem[] = autoRun.map(fu => ({
              task: { id: fu.id, title: fu.title, type: fu.task_type },
              status: 'queued' as const,
            }))
            updateItems(prev => {
              const deduped = followUpItems.filter(
                fi => !prev.some(p => p.task.id === fi.task.id),
              )
              return [...prev, ...deduped]
            })
          }
        }
      } catch (e) {
        updateItems(prev =>
          prev.map(it =>
            it.task.id === taskId
              ? { ...it, status: 'failed', error: String(e) }
              : it,
          ),
        )
      }
    }

    loopRunningRef.current = false
    setIsRunning(false)
    onCompleted()
  }

  function enqueue(newItems: QueueItem[]) {
    if (newItems.length === 0) return
    const toAdd: RunnerItem[] = newItems.map(qi => ({
      task: {
        id: qi.taskId,
        title: qi.title,
        type: qi.taskType,
        projectId: qi.projectId,
        projectName: qi.projectName,
      },
      status: 'queued' as const,
    }))
    updateItems(prev => {
      const deduped = toAdd.filter(na => !prev.some(p => p.task.id === na.task.id))
      return [...prev, ...deduped]
    })
    setIsVisible(true)
    if (!loopRunningRef.current) void runLoop()
  }

  function enqueueNext(newItems: QueueItem[]) {
    if (newItems.length === 0) return
    const toAdd: RunnerItem[] = newItems.map(qi => ({
      task: {
        id: qi.taskId,
        title: qi.title,
        type: qi.taskType,
        projectId: qi.projectId,
        projectName: qi.projectName,
      },
      status: 'queued' as const,
    }))
    updateItems(prev => {
      const deduped = toAdd.filter(na => !prev.some(p => p.task.id === na.task.id))
      const firstQueued = prev.findIndex(it => it.status === 'queued')
      const insertAt = firstQueued >= 0 ? firstQueued : prev.length
      return [...prev.slice(0, insertAt), ...deduped, ...prev.slice(insertAt)]
    })
    setIsVisible(true)
    if (!loopRunningRef.current) void runLoop()
  }

  function removeItem(taskId: string) {
    updateItems(prev =>
      prev.filter(it => !(it.task.id === taskId && it.status === 'queued')),
    )
  }

  function pause() {
    // Update ref immediately so the loop sees it after the current await.
    isPausedRef.current = true
    setIsPaused(true)
  }

  function resume() {
    isPausedRef.current = false
    setIsPaused(false)
    resumeCallbackRef.current?.()
  }

  function close() {
    if (loopRunningRef.current) return
    updateItems(() => [])
    setIsVisible(false)
    setIsPaused(false)
    isPausedRef.current = false
  }

  return {
    items,
    isRunning,
    isPaused,
    isVisible,
    enqueue,
    enqueueNext,
    removeItem,
    pause,
    resume,
    close,
  }
}
