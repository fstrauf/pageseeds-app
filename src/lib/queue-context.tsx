import { createContext, useContext } from 'react'
import type { QueueItem } from './types'

export interface QueueContextValue {
  /** Append tasks to the end of the queue. Starts the runner if not already active. */
  enqueue: (items: QueueItem[]) => void
  /** Insert tasks at the front of the pending section (ahead of other queued items). */
  enqueueNext: (items: QueueItem[]) => void
  /** Whether the runner panel is currently visible. */
  isActive: boolean
}

export const QueueContext = createContext<QueueContextValue>({
  enqueue: () => {},
  enqueueNext: () => {},
  isActive: false,
})

export const useQueue = () => useContext(QueueContext)
