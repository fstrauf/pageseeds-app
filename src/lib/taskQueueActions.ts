import { useCallback } from 'react'
import type { Task } from './types'
import { canEnqueue } from './taskCapabilities'
import { useQueueStore } from '@/stores/queueStore'

import type { EnqueueItem } from './types'

function toEnqueueItem(task: Task, projectName?: string): EnqueueItem {
  return {
    task_id: task.id,
    project_id: task.project_id,
    title: task.title ?? task.type ?? 'Untitled',
    task_type: task.type ?? '',
    project_name: projectName ?? null,
  }
}

export function useTaskQueueActions() {
  const queue = useQueueStore()

  const enqueueTasks = useCallback(
    (tasks: Task[], projectName?: string) => {
      const runnable = tasks.filter(canEnqueue)
      if (runnable.length === 0) return
      queue.enqueue(runnable.map(t => toEnqueueItem(t, projectName)))
    },
    [queue]
  )

  const enqueueNext = useCallback(
    (tasks: Task[]) => {
      const runnable = tasks.filter(canEnqueue)
      if (runnable.length === 0) return
      queue.enqueueNext(runnable.map(t => toEnqueueItem(t)))
    },
    [queue]
  )

  return { enqueueTasks, enqueueNext }
}
