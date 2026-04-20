import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook } from '@testing-library/react'
import { useQueueRunner } from './useQueueRunner'
import { useQueueStore } from '../stores/queueStore'

// Mock Tauri event system
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

// Mock tauri commands used by queue store
vi.mock('@/lib/tauri', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri')>('@/lib/tauri')
  return {
    ...actual,
    executeQueue: vi.fn(() => Promise.resolve()),
    markTasksQueued: vi.fn(() => Promise.resolve()),
    markTasksTodo: vi.fn(() => Promise.resolve()),
    pauseQueue: vi.fn(() => Promise.resolve()),
    resumeQueue: vi.fn(() => Promise.resolve()),
    clearCompletedQueueItems: vi.fn(() => Promise.resolve()),
  }
})

describe('useQueueRunner', () => {
  beforeEach(() => {
    // Reset store to a clean state
    useQueueStore.setState({
      items: [],
      isRunning: false,
      isPaused: false,
      isVisible: false,
    })
  })

  it('returns a stable items reference between renders when store is unchanged', () => {
    const { result, rerender } = renderHook(() => useQueueRunner())

    const firstItemsRef = result.current.items
    rerender()
    const secondItemsRef = result.current.items

    // Same reference means no unnecessary re-renders downstream
    expect(secondItemsRef).toBe(firstItemsRef)
  })

  it('calls onCompleted when all items are done and queue is not running', () => {
    const onCompleted = vi.fn()

    useQueueStore.setState({
      items: [
        {
          taskId: '1',
          projectId: 'p1',
          title: 'Task 1',
          taskType: 'test',
          status: 'completed',
        },
      ],
      isRunning: false,
    })

    renderHook(() => useQueueRunner(onCompleted))

    expect(onCompleted).toHaveBeenCalledTimes(1)
  })

  it('does not call onCompleted while queue is still running', () => {
    const onCompleted = vi.fn()

    useQueueStore.setState({
      items: [
        {
          taskId: '1',
          projectId: 'p1',
          title: 'Task 1',
          taskType: 'test',
          status: 'completed',
        },
      ],
      isRunning: true,
    })

    renderHook(() => useQueueRunner(onCompleted))

    expect(onCompleted).not.toHaveBeenCalled()
  })
})

it('does not cascade when parent re-renders with completed items in store', () => {
  const onCompleted = vi.fn()

  useQueueStore.setState({
    items: [{
      taskId: '1',
      projectId: 'p1',
      title: 'T1',
      taskType: 'test',
      status: 'completed',
    }],
    isRunning: false,
  })

  const { rerender } = renderHook(
    () => useQueueRunner(onCompleted),
    { initialProps: { tick: 0 } }
  )

  expect(onCompleted).toHaveBeenCalledTimes(1)

  rerender({ tick: 1 })

  expect(onCompleted).toHaveBeenCalledTimes(1)
})
