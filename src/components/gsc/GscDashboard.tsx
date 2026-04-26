import { useState } from 'react'
import { useErrorHandler } from '../../lib/toast-context'
import { gscFetchAnalytics, gscFetchQueriesForPage } from '../../lib/tauri'
import type { PageMetrics, QueryMetrics } from '../../lib/types'

interface Props {
  siteUrl: string
  authVersion: number
}

function isoToday() {
  return new Date().toISOString().slice(0, 10)
}
function iso28DaysAgo() {
  const d = new Date()
  d.setDate(d.getDate() - 28)
  return d.toISOString().slice(0, 10)
}

export function GscDashboard({ siteUrl: defaultSite }: Props) {
  const [site, setSite] = useState(defaultSite)
  const [startDate, setStartDate] = useState(iso28DaysAgo)
  const [endDate, setEndDate] = useState(isoToday)
  const [limit, setLimit] = useState(25)

  const [pages, setPages] = useState<PageMetrics[]>([])
  const [queries, setQueries] = useState<QueryMetrics[]>([])
  const [selectedPage, setSelectedPage] = useState<string | null>(null)

  const [loading, setLoading] = useState(false)
  const { showError } = useErrorHandler()

  async function fetch() {
    if (!site) return
    setLoading(true)
    setQueries([])
    setSelectedPage(null)
    try {
      const rows = await gscFetchAnalytics(site, startDate, endDate, limit)
      setPages(rows)
    } catch (e) {
      showError(String(e))
    } finally {
      setLoading(false)
    }
  }

  async function fetchQueries(pageUrl: string) {
    setSelectedPage(pageUrl)
    try {
      const qs = await gscFetchQueriesForPage(site, pageUrl, startDate, endDate, 20)
      setQueries(qs)
    } catch (e) {
      showError(String(e))
    }
  }

  return (
    <div className="flex h-full overflow-hidden">
      {/* Left: pages */}
      <div className="flex flex-col flex-1 overflow-hidden">
        {/* Controls */}
        <div
          className="flex gap-2 items-end flex-wrap p-3 border-b shrink-0"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <div className="flex flex-col gap-1">
            <label className="text-xs text-muted-foreground">Site URL</label>
            <input
              className="h-8 px-2 rounded border border-border bg-card text-sm w-64"
              value={site}
              onChange={e => setSite(e.target.value)}
              placeholder="https://example.com/"
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-xs text-muted-foreground">Start</label>
            <input
              type="date"
              className="h-8 px-2 rounded border border-border bg-card text-sm"
              value={startDate}
              onChange={e => setStartDate(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-xs text-muted-foreground">End</label>
            <input
              type="date"
              className="h-8 px-2 rounded border border-border bg-card text-sm"
              value={endDate}
              onChange={e => setEndDate(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1">
            <label className="text-xs text-muted-foreground">Rows</label>
            <input
              type="number"
              className="h-8 px-2 rounded border border-border bg-card text-sm w-16"
              value={limit}
              min={1}
              max={500}
              onChange={e => setLimit(Number(e.target.value))}
            />
          </div>
          <button
            onClick={fetch}
            disabled={loading || !site}
            className="h-8 px-3 rounded text-xs bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40"
          >
            {loading ? 'Loading…' : 'Fetch'}
          </button>
        </div>

        {/* Table */}
        <div className="flex-1 overflow-auto">
          {pages.length === 0 && !loading && (
            <p className="text-xs text-muted-foreground p-4">No data. Configure the site URL and date range, then click Fetch.</p>
          )}
          {pages.length > 0 && (
            <table className="w-full text-xs border-collapse">
              <thead className="sticky top-0 bg-card border-b" style={{ borderColor: 'var(--color-border)' }}>
                <tr>
                  <th className="text-left px-3 py-2 font-medium text-muted-foreground">Page</th>
                  <th className="text-right px-3 py-2 font-medium text-muted-foreground">Clicks</th>
                  <th className="text-right px-3 py-2 font-medium text-muted-foreground">Impr.</th>
                  <th className="text-right px-3 py-2 font-medium text-muted-foreground">CTR</th>
                  <th className="text-right px-3 py-2 font-medium text-muted-foreground">Pos.</th>
                </tr>
              </thead>
              <tbody>
                {pages.map(row => (
                  <tr
                    key={row.page}
                    onClick={() => fetchQueries(row.page)}
                    className={`border-b cursor-pointer hover:bg-muted/30 ${selectedPage === row.page ? 'bg-primary/10' : ''}`}
                    style={{ borderColor: 'var(--color-border)' }}
                  >
                    <td className="px-3 py-1.5 max-w-xs truncate" title={row.page}>
                      {row.page.replace(/^https?:\/\/[^/]+/, '') || '/'}
                    </td>
                    <td className="px-3 py-1.5 text-right">{row.clicks.toLocaleString()}</td>
                    <td className="px-3 py-1.5 text-right">{row.impressions.toLocaleString()}</td>
                    <td className="px-3 py-1.5 text-right">{(row.ctr * 100).toFixed(1)}%</td>
                    <td className="px-3 py-1.5 text-right">{row.position.toFixed(1)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>

      {/* Right: query detail */}
      {selectedPage && (
        <div
          className="w-80 shrink-0 border-l flex flex-col overflow-hidden"
          style={{ borderColor: 'var(--color-border)' }}
        >
          <div className="px-3 py-2 border-b shrink-0 text-xs font-medium truncate" style={{ borderColor: 'var(--color-border)' }}>
            Queries — {selectedPage.replace(/^https?:\/\/[^/]+/, '') || '/'}
          </div>
          <div className="flex-1 overflow-auto">
            <table className="w-full text-xs border-collapse">
              <thead className="sticky top-0 bg-card border-b" style={{ borderColor: 'var(--color-border)' }}>
                <tr>
                  <th className="text-left px-3 py-2 font-medium text-muted-foreground">Query</th>
                  <th className="text-right px-2 py-2 font-medium text-muted-foreground">Clk</th>
                  <th className="text-right px-2 py-2 font-medium text-muted-foreground">Pos</th>
                </tr>
              </thead>
              <tbody>
                {queries.map(q => (
                  <tr key={q.query} className="border-b hover:bg-muted/30" style={{ borderColor: 'var(--color-border)' }}>
                    <td className="px-3 py-1.5 max-w-[160px] truncate" title={q.query}>{q.query}</td>
                    <td className="px-2 py-1.5 text-right">{q.clicks}</td>
                    <td className="px-2 py-1.5 text-right">{q.position.toFixed(1)}</td>
                  </tr>
                ))}
                {queries.length === 0 && (
                  <tr><td colSpan={3} className="px-3 py-4 text-muted-foreground">Loading…</td></tr>
                )}
              </tbody>
            </table>
          </div>
        </div>
      )}
    </div>
  )
}
