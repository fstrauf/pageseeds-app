import { useEffect, useState } from 'react'
import { RefreshCw } from 'lucide-react'
import { getRedditStatistics } from '../../lib/tauri'
import type { RedditStats } from '../../lib/types'
import { useQuery } from '../../hooks/useQuery'
import { useErrorHandler } from '../../lib/toast-context'

interface Props {
  projectId: string
}

export function RedditStats({ projectId }: Props) {
  const { showError } = useErrorHandler()
  const [stats, setStats] = useState<RedditStats | null>(null)

  const { data: fetchedStats, isLoading: loading, refetch, error: queryError } = useQuery(
    `reddit-stats-${projectId}`,
    () => getRedditStatistics(projectId),
    { enabled: !!projectId, staleTime: 0 }
  )

  useEffect(() => {
    setStats(fetchedStats || null)
  }, [fetchedStats])

  useEffect(() => {
    if (queryError) {
      showError(queryError.message)
    }
  }, [queryError, showError])

  const pending = stats?.by_status?.['pending'] ?? 0
  const posted  = stats?.by_status?.['posted']  ?? 0
  const skipped = stats?.by_status?.['skipped'] ?? 0
  const total   = stats?.total_opportunities ?? 0
  const skipRate = total > 0 ? ((skipped / total) * 100).toFixed(0) : '—'
  const postRate = total > 0 ? ((posted  / total) * 100).toFixed(0) : '—'

  return (
    <div className="p-4 space-y-5">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-semibold" style={{ color: 'var(--color-text)' }}>Statistics</h3>
        <button
          onClick={refetch}
          disabled={loading}
          className="p-1 rounded hover:bg-white/5 transition-colors"
        >
          <RefreshCw className={`h-3.5 w-3.5 ${loading ? 'animate-spin' : ''}`} style={{ color: 'var(--color-text-muted)' }} />
        </button>
      </div>

      {stats && (
        <>
          {/* summary cards */}
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <StatCard label="Total" value={total} />
            <StatCard label="Pending" value={pending} accent="amber" />
            <StatCard label="Posted" value={posted} accent="green" />
            <StatCard label="Skipped" value={skipped} />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <StatCard label="Post rate" value={`${postRate}%`} />
            <StatCard label="Skip rate" value={`${skipRate}%`} />
          </div>

          {/* scores */}
          {total > 0 && (
            <div className="grid grid-cols-2 gap-3">
              <StatCard label="Avg score (pending)" value={stats.average_score.toFixed(2)} />
              <StatCard label="Max score (pending)" value={stats.max_score.toFixed(2)} />
            </div>
          )}

          {/* by severity */}
          {Object.keys(stats.pending_by_severity).length > 0 && (
            <div>
              <p className="text-[10px] font-semibold uppercase tracking-wide mb-2" style={{ color: 'var(--color-text-muted)' }}>
                Pending by severity
              </p>
              <div className="space-y-1.5">
                {Object.entries(stats.pending_by_severity)
                  .sort(([, a], [, b]) => b - a)
                  .map(([sev, count]) => (
                    <SeverityBar key={sev} label={sev} count={count} max={pending} />
                  ))}
              </div>
            </div>
          )}
        </>
      )}

      {!stats && !loading && !queryError && (
        <div className="flex items-center justify-center h-24 text-sm" style={{ color: 'var(--color-text-muted)' }}>
          No data yet.
        </div>
      )}
    </div>
  )
}

function StatCard({
  label,
  value,
  accent,
}: {
  label: string
  value: string | number
  accent?: 'amber' | 'green'
}) {
  const valueColor = accent === 'amber'
    ? 'text-amber-600'
    : accent === 'green'
    ? 'text-emerald-600'
    : undefined

  return (
    <div
      className="rounded px-3 py-2.5"
      style={{ background: 'var(--color-background)', border: '1px solid var(--color-border)' }}
    >
      <p className="text-[10px] mb-1" style={{ color: 'var(--color-text-muted)' }}>{label}</p>
      <p className={`text-lg font-semibold leading-none ${valueColor ?? ''}`} style={valueColor ? undefined : { color: 'var(--color-text)' }}>
        {value}
      </p>
    </div>
  )
}

function SeverityBar({ label, count, max }: { label: string; count: number; max: number }) {
  const pct = max > 0 ? (count / max) * 100 : 0
  const barColor = label === 'high' ? 'bg-red-500' : label === 'medium' ? 'bg-amber-500' : 'bg-zinc-500'

  return (
    <div className="flex items-center gap-2">
      <span className="text-[10px] w-14" style={{ color: 'var(--color-text-muted)' }}>{label}</span>
      <div className="flex-1 h-1.5 rounded-full overflow-hidden" style={{ background: 'var(--color-border)' }}>
        <div className={`h-full rounded-full ${barColor}`} style={{ width: `${pct}%` }} />
      </div>
      <span className="text-[10px] w-6 text-right font-medium" style={{ color: 'var(--color-text)' }}>{count}</span>
    </div>
  )
}
