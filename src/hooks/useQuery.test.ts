import { renderHook, act, waitFor } from '@testing-library/react'
import { vi } from 'vitest'
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
