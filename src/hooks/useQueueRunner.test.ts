import { describe, it, expect, vi, beforeEach } from 'vitest'
import { renderHook } from '@testing-library/react'
import { useQueueRunner } from './useQueueRunner'
import { useQueueStore } from '../stores/queueStore'
import type { QueueSnapshot } from '../lib/types'

// Mock Tauri event system
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}))

// Mock tauri commands used by queue store
vi.mock('@/lib/tauri', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri')>('@/lib/tauri')
  return {
    ...actual,
    getQueueSnapshot: vi.fn(() => Promise.resolve({ run: null, items: [] })),
    enqueueTasks: vi.fn(() => Promise.resolve({ run: null, items: [] })),
    removeQueueItem: vi.fn(() => Promise.resolve({ run: null, items: [] })),
    pauseQueue: vi.fn(() => Promise.resolve({ run: null, items: [] })),
    resumeQueue: vi.fn(() => Promise.resolve({ run: null, items: [] })),
    clearCompletedQueueItems: vi.fn(() => Promise.resolve({ run: null, items: [] })),
  }
})

function makeSnapshot(overrides: Partial<QueueSnapshot> = {}): QueueSnapshot {
  return {
    items: [],
    ...overrides,
  }
}

describe('useQueueRunner', () => {
  beforeEach(() => {
    // Reset store to a clean state
    useQueueStore.setState({
      snapshot: null,
      isVisible: false,
      unlisteners: [],
      expandedTaskIds: new Set(),
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
      snapshot: makeSnapshot({
        run: { id: 'run-1', status: 'finished', pause_on_error: true, created_at: '', updated_at: '' },
        items: [
          {
            run_id: 'run-1',
            position: 0,
            task_id: '1',
            project_id: 'p1',
            status: 'completed',
            title: 'Task 1',
            task_type: 'test',
            created_at: '',
            updated_at: '',
          },
        ],
      }),
    })

    renderHook(() => useQueueRunner(onCompleted))

    expect(onCompleted).toHaveBeenCalledTimes(1)
  })

  it('does not call onCompleted while queue is still running', () => {
    const onCompleted = vi.fn()

    useQueueStore.setState({
      snapshot: makeSnapshot({
        run: { id: 'run-1', status: 'running', pause_on_error: true, created_at: '', updated_at: '' },
        items: [
          {
            run_id: 'run-1',
            position: 0,
            task_id: '1',
            project_id: 'p1',
            status: 'completed',
            title: 'Task 1',
            task_type: 'test',
            created_at: '',
            updated_at: '',
          },
        ],
      }),
    })

    renderHook(() => useQueueRunner(onCompleted))

    expect(onCompleted).not.toHaveBeenCalled()
  })
})

it('does not cascade when parent re-renders with completed items in store', () => {
  const onCompleted = vi.fn()

  useQueueStore.setState({
    snapshot: makeSnapshot({
      run: { id: 'run-1', status: 'finished', pause_on_error: true, created_at: '', updated_at: '' },
      items: [{
        run_id: 'run-1',
        position: 0,
        task_id: '1',
        project_id: 'p1',
        status: 'completed',
        title: 'T1',
        task_type: 'test',
        created_at: '',
        updated_at: '',
      }],
    }),
  })

  const { rerender } = renderHook(
    () => useQueueRunner(onCompleted),
    { initialProps: { tick: 0 } }
  )

  expect(onCompleted).toHaveBeenCalledTimes(1)

  rerender({ tick: 1 })

  expect(onCompleted).toHaveBeenCalledTimes(1)
})
