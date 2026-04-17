import { useCallback, useEffect, useRef, useState, useSyncExternalStore } from 'react'

// Lightweight query cache for tauri.ts calls

type QueryKey = string

interface CacheEntry<T> {
  data: T
  error: Error | null
  updatedAt: number
}

const cache = new Map<QueryKey, CacheEntry<unknown>>()
const subscribers = new Map<QueryKey, Set<() => void>>()
const refetchRegistry = new Map<QueryKey, () => void>()

function getCacheKey(queryKey: QueryKey): string {
  return queryKey
}

function notify(key: QueryKey) {
  const subs = subscribers.get(key)
  if (subs) {
    subs.forEach(cb => cb())
  }
}

export function invalidateQueries(pattern?: string | string[]) {
  const patterns = Array.isArray(pattern) ? pattern : pattern ? [pattern] : undefined
  refetchRegistry.forEach((refetch, key) => {
    if (!patterns || patterns.some(p => key.includes(p))) {
      const entry = cache.get(key)
      cache.set(key, { ...(entry || {}), updatedAt: 0 } as CacheEntry<unknown>)
      refetch()
    }
  })
}

export function useQuery<T>(
  queryKey: QueryKey,
  fetcher: () => Promise<T>,
  options?: {
    enabled?: boolean
    staleTime?: number // ms, default 0
    refetchInterval?: number // ms, optional
  }
) {
  const { enabled = true, staleTime = 0, refetchInterval } = options || {}
  const key = getCacheKey(queryKey)
  const fetcherRef = useRef(fetcher)
  fetcherRef.current = fetcher

  const [, forceRender] = useState(0)

  const fetchData = useCallback(async () => {
    try {
      const result = await fetcherRef.current()
      cache.set(key, { data: result as unknown, error: null, updatedAt: Date.now() })
    } catch (err) {
      cache.set(key, {
        data: cache.get(key)?.data,
        error: err instanceof Error ? err : new Error(String(err)),
        updatedAt: Date.now(),
      })
    }
    notify(key)
  }, [key])

  useEffect(() => {
    if (!enabled) return

    // Subscribe to cache updates
    if (!subscribers.has(key)) {
      subscribers.set(key, new Set())
    }
    const subs = subscribers.get(key)!
    const cb = () => forceRender(v => v + 1)
    subs.add(cb)

    // Register refetch for invalidation
    refetchRegistry.set(key, fetchData)

    const entry = cache.get(key)
    const isStale = !entry || Date.now() - entry.updatedAt > staleTime
    if (isStale) {
      fetchData()
    }

    let intervalId: number | undefined
    if (refetchInterval) {
      intervalId = window.setInterval(() => {
        fetchData()
      }, refetchInterval)
    }

    return () => {
      subs.delete(cb)
      refetchRegistry.delete(key)
      if (intervalId) clearInterval(intervalId)
    }
  }, [enabled, key, staleTime, refetchInterval, fetchData])

  const entry = cache.get(key) as CacheEntry<T> | undefined

  return {
    data: entry?.data,
    error: entry?.error,
    isLoading: enabled && entry === undefined,
    refetch: fetchData,
  }
}

export function useMutation<T, V = void>(
  mutationFn: (variables: V) => Promise<T>,
  options?: {
    invalidateQueries?: string | string[]
    onSuccess?: (data: T, variables: V) => void
    onError?: (error: Error, variables: V) => void
  }
) {
  const [isPending, setIsPending] = useState(false)

  const mutate = useCallback(
    async (variables: V) => {
      setIsPending(true)
      try {
        const result = await mutationFn(variables)
        if (options?.invalidateQueries) {
          invalidateQueries(options.invalidateQueries)
        }
        options?.onSuccess?.(result, variables)
        return result
      } catch (err) {
        const error = err instanceof Error ? err : new Error(String(err))
        options?.onError?.(error, variables)
        throw error
      } finally {
        setIsPending(false)
      }
    },
    [mutationFn, options]
  )

  return { mutate, isPending }
}
