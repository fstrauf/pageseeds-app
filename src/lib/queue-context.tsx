import { createContext, useContext } from 'react'

export interface QueueContextTask {
  taskId: string
  projectId: string
  title?: string
  taskType?: string
  projectName?: string
  status?: string
}

export interface QueueContextValue {
  /** Append tasks to the end of the queue. Starts the runner if not already active. */
  enqueue: (items: QueueContextTask[]) => void
  /** Insert tasks at the front of the pending section (ahead of other queued items). */
  enqueueNext: (items: QueueContextTask[]) => void
  /** Whether the runner panel is currently visible. */
  isActive: boolean
}

export const QueueContext = createContext<QueueContextValue>({
  enqueue: () => {},
  enqueueNext: () => {},
  isActive: false,
})

export const useQueue = () => useContext(QueueContext)
