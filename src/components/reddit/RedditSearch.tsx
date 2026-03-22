import { useState } from 'react'
import { Search, Loader2 } from 'lucide-react'
import { searchReddit, upsertRedditOpportunity } from '../../lib/tauri'
import type { RedditOpportunity, SubmissionSummary } from '../../lib/types'

interface Props {
  projectId: string
  onSaved: () => void
}

export function RedditSearch({ projectId, onSaved }: Props) {
  const [query, setQuery] = useState('')
  const [subreddit, setSubreddit] = useState('')
  const [limit, setLimit] = useState(25)
  const [sort, setSort] = useState('relevance')
  const [timeFilter, setTimeFilter] = useState('all')
  const [results, setResults] = useState<SubmissionSummary[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState<string | null>(null)
  const [savedIds, setSavedIds] = useState<Set<string>>(new Set())

  async function handleSearch(e: React.FormEvent) {
    e.preventDefault()
    if (!query.trim()) return
    setLoading(true)
    setError(null)
    setResults([])
    try {
      const data = await searchReddit(query.trim(), subreddit.trim(), limit, sort, timeFilter)
      setResults(data)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  async function handleSave(submission: SubmissionSummary) {
    if (!projectId) return
    setSaving(submission.post_id)
    try {
      const now = new Date().toISOString()
      const opp: RedditOpportunity = {
        post_id: submission.post_id,
        title: submission.title,
        url: submission.url,
        subreddit: submission.subreddit,
        author: submission.author,
        upvotes: submission.upvotes,
        comment_count: submission.comment_count,
        posted_date: submission.created_at,
        key_pain_points: [],
        reply_status: 'pending',
        project_id: projectId,
        created_at: now,
        updated_at: now,
      }
      await upsertRedditOpportunity(opp)
      setSavedIds(ids => new Set([...ids, submission.post_id]))
      onSaved()
    } catch (e) {
      setError(String(e))
    } finally {
      setSaving(null)
    }
  }

  const inputClass = 'rounded px-2 py-1.5 text-xs outline-none focus:ring-1 focus:ring-primary/50'
  const inputStyle = {
    background: 'var(--color-background)',
    border: '1px solid var(--color-border)',
    color: 'var(--color-text)',
  }

  return (
    <div className="flex flex-col h-full">
      {/* search form */}
      <form onSubmit={handleSearch} className="p-4 space-y-3 border-b shrink-0" style={{ borderColor: 'var(--color-border)' }}>
        <div className="flex gap-2">
          <input
            className={`${inputClass} flex-1`}
            style={inputStyle}
            placeholder="Search query…"
            value={query}
            onChange={e => setQuery(e.target.value)}
          />
          <input
            className={`${inputClass} w-40`}
            style={inputStyle}
            placeholder="Subreddit (optional)"
            value={subreddit}
            onChange={e => setSubreddit(e.target.value)}
          />
        </div>
        <div className="flex gap-2 items-center">
          <select
            className={inputClass}
            style={inputStyle}
            value={sort}
            onChange={e => setSort(e.target.value)}
          >
            {['relevance', 'new', 'hot', 'top', 'comments'].map(s => (
              <option key={s} value={s}>{s}</option>
            ))}
          </select>
          <select
            className={inputClass}
            style={inputStyle}
            value={timeFilter}
            onChange={e => setTimeFilter(e.target.value)}
          >
            {['all', 'year', 'month', 'week', 'day'].map(t => (
              <option key={t} value={t}>{t}</option>
            ))}
          </select>
          <select
            className={inputClass}
            style={inputStyle}
            value={limit}
            onChange={e => setLimit(Number(e.target.value))}
          >
            {[10, 25, 50].map(n => (
              <option key={n} value={n}>{n} results</option>
            ))}
          </select>
          <div className="flex-1" />
          <button
            type="submit"
            disabled={loading || !query.trim()}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded text-xs font-medium transition-colors disabled:opacity-50"
            style={{ background: 'var(--color-primary)', color: 'var(--color-primary-foreground)' }}
          >
            {loading ? <Loader2 className="h-3.5 w-3.5 animate-spin" /> : <Search className="h-3.5 w-3.5" />}
            Search
          </button>
        </div>
      </form>

      {error && (
        <div className="mx-4 my-2 px-3 py-2 rounded text-xs bg-red-100 text-red-700">{error}</div>
      )}

      {/* results */}
      <div className="flex-1 overflow-y-auto">
        {results.length === 0 && !loading ? (
          <div className="flex items-center justify-center h-32 text-sm" style={{ color: 'var(--color-text-muted)' }}>
            {query ? 'No results.' : 'Enter a query above to search Reddit.'}
          </div>
        ) : (
          <div className="divide-y" style={{ borderColor: 'var(--color-border)' }}>
            {results.map(sub => (
              <div key={sub.post_id} className="px-4 py-3 flex gap-3 hover:bg-white/3 transition-colors">
                <div className="flex-1 min-w-0">
                  <p className="text-xs font-medium truncate" style={{ color: 'var(--color-text)' }}>
                    {sub.title ?? '—'}
                  </p>
                  <p className="text-[10px] mt-0.5" style={{ color: 'var(--color-text-muted)' }}>
                    {sub.subreddit ? `r/${sub.subreddit}` : ''}
                    {sub.author ? ` · ${sub.author}` : ''}
                    {sub.upvotes != null ? ` · ${sub.upvotes} pts` : ''}
                    {sub.days_old != null ? ` · ${sub.days_old}d old` : ''}
                    {sub.comment_count != null ? ` · ${sub.comment_count} comments` : ''}
                  </p>
                  {sub.selftext && (
                    <p className="text-[10px] mt-1 line-clamp-2 leading-relaxed" style={{ color: 'var(--color-text-muted)' }}>
                      {sub.selftext}
                    </p>
                  )}
                </div>
                <div className="shrink-0 flex items-start pt-0.5">
                  {savedIds.has(sub.post_id) ? (
                    <span className="px-2 py-1 rounded text-[10px] bg-green-100 text-green-700">Saved</span>
                  ) : (
                    <button
                      onClick={() => handleSave(sub)}
                      disabled={saving === sub.post_id}
                      className="px-2 py-1 rounded text-[10px] border transition-colors hover:bg-white/5 disabled:opacity-50"
                      style={{ borderColor: 'var(--color-border)', color: 'var(--color-text-muted)' }}
                    >
                      {saving === sub.post_id ? '…' : 'Save'}
                    </button>
                  )}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
