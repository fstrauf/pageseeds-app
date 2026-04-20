import { useEffect, useState, useCallback } from 'react'
import { ChevronUp, ChevronDown, RefreshCw } from 'lucide-react'
import { listRedditOpportunities, markRedditSkipped, postToReddit } from '../../lib/tauri'
import type { RedditOpportunity } from '../../lib/types'
import { useQuery } from '../../hooks/useQuery'
import { useErrorHandler } from '../../lib/toast-context'

interface Props {
  projectId: string
  selectedId?: string
  onSelect: (opp: RedditOpportunity) => void
  onStatusChange?: () => void
}

type SortKey = 'final_score' | 'upvotes' | 'subreddit' | 'reply_status' | 'created_at'

function SortIcon({ col, sortKey, sortAsc }: { col: SortKey; sortKey: SortKey; sortAsc: boolean }) {
  if (sortKey !== col) return null
  return sortAsc ? <ChevronUp className="inline h-3 w-3 ml-1" /> : <ChevronDown className="inline h-3 w-3 ml-1" />
}

function relativeDate(iso: string | null | undefined): string {
  if (!iso) return '—'
  const ms = Date.now() - new Date(iso).getTime()
  const days = Math.floor(ms / 86_400_000)
  if (days === 0) return 'today'
  if (days === 1) return '1d ago'
  if (days < 30) return `${days}d ago`
  const months = Math.floor(days / 30)
  return `${months}mo ago`
}

const STATUS_COLORS: Record<string, string> = {
  pending: 'text-amber-700 bg-amber-100',
  posted: 'text-emerald-700 bg-emerald-100',
  skipped: 'text-zinc-600 bg-zinc-100',
}

const SEVERITY_COLORS: Record<string, string> = {
  high: 'text-red-600',
  medium: 'text-amber-600',
  low: 'text-zinc-500',
}

export function OpportunityFeed({ projectId, selectedId, onSelect, onStatusChange }: Props) {
  const { showError } = useErrorHandler()

  const [statusFilter, setStatusFilter] = useState<string>('pending')
  const [sortKey, setSortKey] = useState<SortKey>('final_score')
  const [sortAsc, setSortAsc] = useState(false)
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set())
  const [bulkWorking, setBulkWorking] = useState(false)
  const [bulkMsg, setBulkMsg] = useState<string | null>(null)

  const { data: fetchedOpps = [], isLoading: loading, refetch, error: queryError } = useQuery(
    `reddit-opps-${projectId}-${statusFilter}`,
    () => listRedditOpportunities(projectId, statusFilter || undefined),
    { enabled: !!projectId, staleTime: 0 }
  )



  useEffect(() => {
    if (queryError) {
      showError(queryError.message)
    }
  }, [queryError, showError])

  function toggleSort(key: SortKey) {
    if (sortKey === key) setSortAsc(a => !a)
    else { setSortKey(key); setSortAsc(false) }
  }

  const sorted = [...fetchedOpps].sort((a, b) => {
    const av = a[sortKey] ?? (sortKey === 'final_score' || sortKey === 'upvotes' ? -Infinity : '')
    const bv = b[sortKey] ?? (sortKey === 'final_score' || sortKey === 'upvotes' ? -Infinity : '')
    const cmp = av < bv ? -1 : av > bv ? 1 : 0
    return sortAsc ? cmp : -cmp
  })

  const pendingIds = sorted
    .filter(o => o.reply_status === 'pending')
    .map(o => o.post_id)

  const allPendingSelected =
    pendingIds.length > 0 && pendingIds.every(id => selectedIds.has(id))

  function toggleSelectAll() {
    if (allPendingSelected) {
      setSelectedIds(new Set())
    } else {
      setSelectedIds(new Set(pendingIds))
    }
  }

  function toggleRow(id: string, e: React.MouseEvent) {
    // Only toggle checkbox; row click still opens detail
    e.stopPropagation()
    setSelectedIds(prev => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const handleBulkSkip = useCallback(async () => {
    if (selectedIds.size === 0 || bulkWorking) return
    setBulkWorking(true)
    setBulkMsg(null)
    let done = 0
    for (const id of selectedIds) {
      try { await markRedditSkipped(id); done++ } catch { /* continue */ }
    }
    setBulkMsg(`Skipped ${done} of ${selectedIds.size}`)
    setSelectedIds(new Set())
    await refetch()
    onStatusChange?.()
    setBulkWorking(false)
  }, [selectedIds, bulkWorking, refetch, onStatusChange])

  const handleBulkPostToReddit = useCallback(async () => {
    if (selectedIds.size === 0 || bulkWorking) return
    const withReply = sorted.filter(o => selectedIds.has(o.post_id) && o.reply_text?.trim())
    const missing = selectedIds.size - withReply.length
    if (missing > 0) {
      setBulkMsg(`${missing} selected item${missing > 1 ? 's have' : ' has'} no draft reply — run enrichment first.`)
      return
    }
    setBulkWorking(true)
    setBulkMsg(null)
    let done = 0
    const errors: string[] = []
    for (let i = 0; i < withReply.length; i++) {
      const opp = withReply[i]
      setBulkMsg(`Posting ${i + 1}/${withReply.length}…`)
      try {
        await postToReddit(projectId, opp.post_id, opp.reply_text ?? '')
        done++
      } catch (e) {
        errors.push(`${opp.post_id}: ${String(e)}`)
      }
    }
    if (errors.length > 0) {
      setBulkMsg(`Posted ${done}/${withReply.length}. Failed: ${errors.join(', ')}`)
    } else {
      setBulkMsg(`Posted ${done} of ${withReply.length} to Reddit`)
    }
    setSelectedIds(new Set())
    await refetch()
    onStatusChange?.()
    setBulkWorking(false)
  }, [selectedIds, sorted, bulkWorking, projectId, refetch, onStatusChange])

  const thClass = 'px-3 py-2 text-left text-xs font-medium cursor-pointer select-none'
    + ' hover:text-white transition-colors'
  const thStyle = { color: 'var(--color-text-muted)' }

  return (
    <div className="flex flex-col h-full">
      {/* toolbar */}
      <div className="flex items-center gap-2 px-4 py-2 border-b shrink-0" style={{ borderColor: 'var(--color-border)' }}>
        <div className="flex gap-1">
          {(['', 'pending', 'posted', 'skipped'] as const).map(s => (
            <button
              key={s}
              onClick={() => { setStatusFilter(s); setSelectedIds(new Set()) }}
              className={`px-2 py-0.5 rounded text-xs transition-colors ${
                statusFilter === s
                  ? 'bg-primary text-primary-foreground'
                  : 'text-muted-foreground hover:text-foreground'
              }`}
            >
              {s === '' ? 'All' : s.charAt(0).toUpperCase() + s.slice(1)}
            </button>
          ))}
        </div>
        <div className="flex-1" />
        <span className="text-xs" style={{ color: 'var(--color-text-muted)' }}>
          {fetchedOpps.length} result{fetchedOpps.length !== 1 ? 's' : ''}
        </span>
        <button
          onClick={refetch}
          disabled={loading}
          className="p-1 rounded hover:bg-white/5 transition-colors"
          title="Refresh"
        >
          <RefreshCw className={`h-3.5 w-3.5 ${loading ? 'animate-spin' : ''}`} style={{ color: 'var(--color-text-muted)' }} />
        </button>
      </div>

      {/* bulk action bar — only visible when items are selected */}
      {selectedIds.size > 0 && (
        <div
          className="flex items-center gap-2 px-4 py-2 border-b text-xs shrink-0"
          style={{ borderColor: 'var(--color-border)', background: 'var(--color-background)' }}
        >
          <span style={{ color: 'var(--color-text-muted)' }}>
            {selectedIds.size} selected
          </span>
          <div className="flex-1" />
          <button
            onClick={handleBulkSkip}
            disabled={bulkWorking}
            className="px-3 py-1 rounded border text-xs transition-colors hover:bg-white/5 disabled:opacity-40"
            style={{ borderColor: 'var(--color-border)', color: 'var(--color-text-muted)' }}
          >
            {bulkWorking ? 'Working…' : `Skip ${selectedIds.size}`}
          </button>
          <button
            onClick={handleBulkPostToReddit}
            disabled={bulkWorking}
            className="px-3 py-1 rounded text-xs font-medium transition-colors disabled:opacity-40"
            style={{ background: 'var(--color-primary)', color: 'var(--color-primary-foreground)' }}
          >
            {bulkWorking ? 'Working…' : `Post ${selectedIds.size} to Reddit`}
          </button>
          <button
            onClick={() => setSelectedIds(new Set())}
            className="px-2 py-1 rounded text-xs hover:bg-white/5 transition-colors"
            style={{ color: 'var(--color-text-muted)' }}
          >
            Clear
          </button>
        </div>
      )}

      {bulkMsg && (
        <div className="mx-4 my-1 px-3 py-1.5 rounded text-xs bg-blue-100 text-blue-700">{bulkMsg}</div>
      )}

      {/* table */}
      <div className="flex-1 overflow-y-auto">
        {fetchedOpps.length === 0 && !loading ? (
          <div className="flex items-center justify-center h-32 text-sm" style={{ color: 'var(--color-text-muted)' }}>
            {statusFilter === 'pending' ? 'No pending opportunities.' : 'No opportunities found.'}
          </div>
        ) : (
          <table className="w-full text-xs">
            <thead className="sticky top-0" style={{ background: 'var(--color-surface)', borderBottom: '1px solid var(--color-border)' }}>
              <tr>
                <th className="px-3 py-2 w-8">
                  {pendingIds.length > 0 && (
                    <input
                      type="checkbox"
                      checked={allPendingSelected}
                      onChange={toggleSelectAll}
                      className="cursor-pointer accent-primary"
                      title="Select all pending"
                    />
                  )}
                </th>
                <th className={thClass} style={thStyle} onClick={() => toggleSort('final_score')}>Score<SortIcon col="final_score" sortKey={sortKey} sortAsc={sortAsc} /></th>
                <th className={thClass} style={{ ...thStyle, minWidth: 200 }}>Title</th>
                <th className={thClass} style={thStyle} onClick={() => toggleSort('subreddit')}>Subreddit<SortIcon col="subreddit" sortKey={sortKey} sortAsc={sortAsc} /></th>
                <th className={thClass} style={thStyle} onClick={() => toggleSort('upvotes')}>↑<SortIcon col="upvotes" sortKey={sortKey} sortAsc={sortAsc} /></th>
                <th className={thClass} style={{ ...thStyle, minWidth: 240 }}>Reply</th>
                <th className={thClass} style={thStyle} onClick={() => toggleSort('created_at')}>Posted<SortIcon col="created_at" sortKey={sortKey} sortAsc={sortAsc} /></th>
                <th className={thClass} style={thStyle} onClick={() => toggleSort('reply_status')}>Status<SortIcon col="reply_status" sortKey={sortKey} sortAsc={sortAsc} /></th>
              </tr>
            </thead>
            <tbody>
              {sorted.map(opp => {
                const isPending = opp.reply_status === 'pending'
                const isChecked = selectedIds.has(opp.post_id)
                return (
                  <tr
                    key={opp.post_id}
                    onClick={() => onSelect(opp)}
                    className={`cursor-pointer border-b transition-colors hover:bg-white/5 ${
                      selectedId === opp.post_id ? 'bg-primary/10' : ''
                    }`}
                    style={{ borderColor: 'var(--color-border)' }}
                  >
                    <td className="px-3 py-2 w-8" onClick={e => isPending && toggleRow(opp.post_id, e)}>
                      {isPending && (
                        <input
                          type="checkbox"
                          checked={isChecked}
                          onChange={() => {/* controlled via onClick */}}
                          className="cursor-pointer accent-primary pointer-events-none"
                        />
                      )}
                    </td>
                    <td className="px-3 py-2 font-mono font-medium" style={{ color: 'var(--color-text)' }}>
                      {opp.final_score != null ? opp.final_score.toFixed(1) : '—'}
                      {opp.severity && (
                        <span className={`ml-1.5 text-[10px] font-semibold ${SEVERITY_COLORS[opp.severity] ?? ''}`}>
                          {opp.severity.toUpperCase()}
                        </span>
                      )}
                    </td>
                    <td className="px-3 py-2 max-w-xs truncate" style={{ color: 'var(--color-text)', maxWidth: 260 }}>
                      {opp.title ?? '—'}
                    </td>
                    <td className="px-3 py-2" style={{ color: 'var(--color-text-muted)' }}>
                      {opp.subreddit ? `r/${opp.subreddit}` : '—'}
                    </td>
                    <td className="px-3 py-2 text-right" style={{ color: 'var(--color-text-muted)' }}>
                      {opp.upvotes ?? '—'}
                    </td>
                    <td className="px-3 py-2" style={{ color: 'var(--color-text-muted)', maxWidth: 280 }}>
                      {opp.reply_text
                        ? <span className="line-clamp-2 leading-relaxed" title={opp.reply_text}>{opp.reply_text}</span>
                        : <span className="italic opacity-40">no draft</span>
                      }
                    </td>
                    <td className="px-3 py-2 whitespace-nowrap" style={{ color: 'var(--color-text-muted)' }}>
                      {relativeDate(opp.created_at)}
                    </td>
                    <td className="px-3 py-2">
                      <span className={`px-1.5 py-0.5 rounded text-[10px] font-medium ${STATUS_COLORS[opp.reply_status] ?? 'text-zinc-600 bg-zinc-100'}`}>
                        {opp.reply_status}
                      </span>
                    </td>
                  </tr>
                )
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  )
}
