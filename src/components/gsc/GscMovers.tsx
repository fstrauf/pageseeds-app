import { useState } from 'react'
import { useErrorHandler } from '../../lib/toast-context'
import { gscComputeMovers } from '../../lib/tauri'
import type { MoverMetrics } from '../../lib/types'

interface Props {
  siteUrl: string
  authVersion: number
}

function isoToday() {
  return new Date().toISOString().slice(0, 10)
}
function isoNDaysAgo(n: number) {
  const d = new Date()
  d.setDate(d.getDate() - n)
  return d.toISOString().slice(0, 10)
}

export function GscMovers({ siteUrl: defaultSite }: Props) {
  const [site, setSite] = useState(defaultSite)

  const [currStart, setCurrStart] = useState(() => isoNDaysAgo(28))
  const [currEnd, setCurrEnd] = useState(isoToday)
  const [prevStart, setPrevStart] = useState(() => isoNDaysAgo(56))
  const [prevEnd, setPrevEnd] = useState(() => isoNDaysAgo(28))
  const [limit, setLimit] = useState(50)

  const [movers, setMovers] = useState<MoverMetrics[]>([])
  const [loading, setLoading] = useState(false)
  const { showError } = useErrorHandler()

  async function compute() {
    if (!site) return
    setLoading(true)
    try {
      const rows = await gscComputeMovers(site, currStart, currEnd, prevStart, prevEnd, limit)
      setMovers(rows)
    } catch (e) {
      showError(String(e))
    } finally {
      setLoading(false)
    }
  }

  function delta(val: number, inverse = false) {
    if (val === 0) return <span className="text-muted-foreground">—</span>
    const positive = inverse ? val < 0 : val > 0
    const sign = val > 0 ? '+' : ''
    return (
      <span className={positive ? 'text-green-600' : 'text-red-500'}>
        {sign}{val.toFixed(inverse ? 1 : 0)}
      </span>
    )
  }

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Controls */}
      <div
        className="flex gap-2 items-end flex-wrap p-3 border-b shrink-0"
        style={{ borderColor: 'var(--color-border)' }}
      >
        <div className="flex flex-col gap-1">
          <label className="text-xs text-muted-foreground">Site URL</label>
          <input
            className="h-8 px-2 rounded border border-border bg-card text-sm w-56"
            value={site}
            onChange={e => setSite(e.target.value)}
            placeholder="https://example.com/"
          />
        </div>
        <div className="flex flex-col gap-1">
          <label className="text-xs text-muted-foreground">Current period</label>
          <div className="flex gap-1">
            <input
              type="date"
              className="h-8 px-2 rounded border border-border bg-card text-sm"
              value={currStart}
              onChange={e => setCurrStart(e.target.value)}
            />
            <span className="self-center text-xs text-muted-foreground">→</span>
            <input
              type="date"
              className="h-8 px-2 rounded border border-border bg-card text-sm"
              value={currEnd}
              onChange={e => setCurrEnd(e.target.value)}
            />
          </div>
        </div>
        <div className="flex flex-col gap-1">
          <label className="text-xs text-muted-foreground">Previous period</label>
          <div className="flex gap-1">
            <input
              type="date"
              className="h-8 px-2 rounded border border-border bg-card text-sm"
              value={prevStart}
              onChange={e => setPrevStart(e.target.value)}
            />
            <span className="self-center text-xs text-muted-foreground">→</span>
            <input
              type="date"
              className="h-8 px-2 rounded border border-border bg-card text-sm"
              value={prevEnd}
              onChange={e => setPrevEnd(e.target.value)}
            />
          </div>
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
          onClick={compute}
          disabled={loading || !site}
          className="h-8 px-3 rounded text-xs bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40"
        >
          {loading ? 'Computing…' : 'Compute'}
        </button>
      </div>

      {/* Table */}
      <div className="flex-1 overflow-auto">
        {movers.length === 0 && !loading && (
          <p className="text-xs text-muted-foreground p-4">
            Set date ranges and click Compute to see traffic movers.
          </p>
        )}
        {movers.length > 0 && (
          <table className="w-full text-xs border-collapse">
            <thead className="sticky top-0 bg-card border-b" style={{ borderColor: 'var(--color-border)' }}>
              <tr>
                <th className="text-left px-3 py-2 font-medium text-muted-foreground">Page</th>
                <th className="text-right px-3 py-2 font-medium text-muted-foreground">Clicks Δ</th>
                <th className="text-right px-3 py-2 font-medium text-muted-foreground">Impr. Δ</th>
                <th className="text-right px-3 py-2 font-medium text-muted-foreground">Pos. Δ</th>
                <th className="text-right px-3 py-2 font-medium text-muted-foreground">Curr Clicks</th>
                <th className="text-right px-3 py-2 font-medium text-muted-foreground">Prev Clicks</th>
              </tr>
            </thead>
            <tbody>
              {movers.map(row => (
                <tr
                  key={row.key}
                  className="border-b hover:bg-muted/30"
                  style={{ borderColor: 'var(--color-border)' }}
                >
                  <td className="px-3 py-1.5 max-w-xs truncate" title={row.key}>
                    {row.key.replace(/^https?:\/\/[^/]+/, '') || '/'}
                  </td>
                  <td className="px-3 py-1.5 text-right font-medium">{delta(row.clicks_delta)}</td>
                  <td className="px-3 py-1.5 text-right">{delta(row.impressions_delta)}</td>
                  <td className="px-3 py-1.5 text-right">{delta(row.position_delta, true)}</td>
                  <td className="px-3 py-1.5 text-right">{row.current_clicks.toLocaleString()}</td>
                  <td className="px-3 py-1.5 text-right text-muted-foreground">{row.previous_clicks.toLocaleString()}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  )
}
