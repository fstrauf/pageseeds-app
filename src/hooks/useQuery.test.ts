import { renderHook, act, waitFor } from '@testing-library/react'
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { useQuery, useMutation, invalidateQueries } from './useQuery'

describe('useQuery', () => {
  beforeEach(() => {
    // Clear static caches by invalidating everything
    invalidateQueries()
  })

  it('fetches data on mount', async () => {
    const fetcher = vi.fn().mockResolvedValue('hello')
    const { result } = renderHook(() => useQuery('test-key', fetcher))

    expect(result.current.isLoading).toBe(true)
    await waitFor(() => expect(result.current.data).toBe('hello'))
    expect(fetcher).toHaveBeenCalledTimes(1)
  })

  it('returns cached data without re-fetching when fresh', async () => {
    const fetcher = vi.fn().mockResolvedValue('cached')
    const { result } = renderHook(() => useQuery('fresh-key', fetcher, { staleTime: 10000 }))

    await waitFor(() => expect(result.current.data).toBe('cached'))
    expect(fetcher).toHaveBeenCalledTimes(1)

    // Mount a second hook with the same key
    const { result: result2 } = renderHook(() => useQuery('fresh-key', fetcher, { staleTime: 10000 }))
    expect(result2.current.data).toBe('cached')
    expect(fetcher).toHaveBeenCalledTimes(1)
  })

  it('re-fetches after invalidation', async () => {
    let value = 'first'
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(value))

    const { result } = renderHook(() => useQuery('invalidate-key', fetcher))
    await waitFor(() => expect(result.current.data).toBe('first'))
    expect(fetcher).toHaveBeenCalledTimes(1)

    value = 'second'
    act(() => {
      invalidateQueries('invalidate-key')
    })

    await waitFor(() => expect(result.current.data).toBe('second'))
    expect(fetcher).toHaveBeenCalledTimes(2)
  })

  it('does not fetch when disabled', () => {
    const fetcher = vi.fn().mockResolvedValue('data')
    const { result } = renderHook(() => useQuery('disabled-key', fetcher, { enabled: false }))

    expect(result.current.isLoading).toBe(false)
    expect(result.current.data).toBeUndefined()
    expect(fetcher).not.toHaveBeenCalled()
  })

  it('refetch() returns latest data even when cache is not stale', async () => {
    let value = 'initial'
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(value))

    const { result } = renderHook(() =>
      useQuery('refetch-fresh-key', fetcher, { staleTime: 30_000 })
    )
    await waitFor(() => expect(result.current.data).toBe('initial'))
    expect(fetcher).toHaveBeenCalledTimes(1)

    // Simulate new data available from backend (like after audit completes)
    value = 'updated'
    // refetch() should bypass staleTime and fetch fresh data
    act(() => {
      result.current.refetch()
    })

    await waitFor(() => expect(result.current.data).toBe('updated'))
    expect(fetcher).toHaveBeenCalledTimes(2)
  })

  it('refetch() updates data across two hooks sharing the same key', async () => {
    let value = 'v1'
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(value))

    // Hook A — initial fetch
    const { result: resultA } = renderHook(() =>
      useQuery('shared-refetch-key', fetcher, { staleTime: 30_000 })
    )
    await waitFor(() => expect(resultA.current.data).toBe('v1'))
    expect(fetcher).toHaveBeenCalledTimes(1)

    // Hook B — reads from cache (no second fetch because not stale)
    const { result: resultB } = renderHook(() =>
      useQuery('shared-refetch-key', fetcher, { staleTime: 30_000 })
    )
    expect(resultB.current.data).toBe('v1')
    expect(fetcher).toHaveBeenCalledTimes(1)

    // Backend data changes
    value = 'v2'
    // Hook B calls refetch (like the Overview effect does)
    act(() => {
      resultB.current.refetch()
    })

    await waitFor(() => expect(resultB.current.data).toBe('v2'))
    // Hook A should also see the update because they share the same cache key
    await waitFor(() => expect(resultA.current.data).toBe('v2'))
    expect(fetcher).toHaveBeenCalledTimes(2)
  })
})

describe('useMutation', () => {
  it('mutates and calls onSuccess', async () => {
    const mutationFn = vi.fn().mockResolvedValue('result')
    const onSuccess = vi.fn()

    const { result } = renderHook(() =>
      useMutation(mutationFn, { onSuccess })
    )

    await act(async () => {
      await result.current.mutate('input')
    })

    expect(mutationFn).toHaveBeenCalledWith('input')
    expect(onSuccess).toHaveBeenCalledWith('result', 'input')
    expect(result.current.isPending).toBe(false)
  })

  it('mutate reference is stable across renders', async () => {
    const mutationFn = vi.fn().mockResolvedValue('ok')
    const onSuccess = vi.fn()

    const { result, rerender } = renderHook(() =>
      useMutation(mutationFn, { onSuccess })
    )

    const firstMutate = result.current.mutate
    rerender()
    expect(result.current.mutate).toBe(firstMutate)
  })

  it('invalidates queries on success', async () => {
    const fetcher = vi.fn().mockResolvedValue('data')
    renderHook(() => useQuery('inv-key', fetcher))
    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1))

    const mutationFn = vi.fn().mockResolvedValue('done')
    const { result } = renderHook(() =>
      useMutation(mutationFn, { invalidateQueries: 'inv-key' })
    )

    await act(async () => {
      await result.current.mutate('x')
    })

    await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2))
  })
})
