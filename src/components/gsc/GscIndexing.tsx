import { useState } from 'react'
import { gscInspectUrls, gscGenerateIndexingReport } from '../../lib/tauri'
import type { InspectionRecord } from '../../lib/types'

interface Props {
  projectId: string
  siteUrl: string
  authVersion: number
}

const VERDICT_COLOR: Record<string, string> = {
  PASS: 'text-green-600',
  FAIL: 'text-red-500',
  NEUTRAL: 'text-yellow-500',
}

export function GscIndexing({ projectId, siteUrl: defaultSite }: Props) {
  const [site, setSite] = useState(defaultSite)
  const [urlsText, setUrlsText] = useState('')
  const [records, setRecords] = useState<InspectionRecord[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [reportPath, setReportPath] = useState<string | null>(null)
  const [filter, setFilter] = useState<string>('ALL')

  async function inspect() {
    const urls = urlsText
      .split('\n')
      .map(l => l.trim())
      .filter(Boolean)
    if (!urls.length || !site) return

    setLoading(true)
    setError(null)
    setReportPath(null)
    try {
      const result = await gscInspectUrls(site, urls)
      setRecords(result)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  async function saveReport() {
    try {
      const path = await gscGenerateIndexingReport(projectId, site, records)
      setReportPath(path)
    } catch (e) {
      setError(String(e))
    }
  }

  const verdicts = ['ALL', 'FAIL', 'NEUTRAL', 'PASS']
  const filtered =
    filter === 'ALL' ? records : records.filter(r => r.verdict === filter)

  return (
    <div className="flex flex-col gap-4 h-full overflow-hidden">
      {/* Controls */}
      <div className="flex gap-2 flex-wrap items-end shrink-0">
        <div className="flex flex-col gap-1">
          <label className="text-xs text-muted-foreground">Site URL</label>
          <input
            className="h-8 px-2 rounded border border-border bg-card text-sm w-64"
            value={site}
            onChange={e => setSite(e.target.value)}
            placeholder="https://example.com/"
          />
        </div>
        <button
          onClick={inspect}
          disabled={loading || !site || !urlsText.trim()}
          className="h-8 px-3 rounded text-xs bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40"
        >
          {loading ? `Inspecting…` : 'Inspect URLs'}
        </button>
        {records.length > 0 && (
          <button
            onClick={saveReport}
            className="h-8 px-3 rounded text-xs border border-border hover:bg-muted"
          >
            Save Report
          </button>
        )}
      </div>

      <textarea
        className="w-full h-28 px-3 py-2 rounded border border-border bg-card text-xs font-mono resize-y shrink-0"
        placeholder={"Enter one URL per line:\nhttps://example.com/blog/post-1\nhttps://example.com/about"}
        value={urlsText}
        onChange={e => setUrlsText(e.target.value)}
      />

      {error && <p className="text-xs text-destructive">{error}</p>}
      {reportPath && (
        <p className="text-xs text-muted-foreground">Report saved: {reportPath}</p>
      )}

      {/* Filter tabs */}
      {records.length > 0 && (
        <div className="flex gap-1 shrink-0">
          {verdicts.map(v => {
            const count = v === 'ALL' ? records.length : records.filter(r => r.verdict === v).length
            return (
              <button
                key={v}
                onClick={() => setFilter(v)}
                className={`px-2 py-1 rounded text-xs border ${filter === v ? 'bg-primary text-primary-foreground border-primary' : 'border-border hover:bg-muted'}`}
              >
                {v} ({count})
              </button>
            )
          })}
        </div>
      )}

      {/* Table */}
      <div className="flex-1 overflow-auto">
        {records.length === 0 && !loading && (
          <p className="text-xs text-muted-foreground">
            Enter URLs above and click Inspect. Note: each URL takes ~200ms due to API quotas.
          </p>
        )}
        {filtered.length > 0 && (
          <table className="w-full text-xs border-collapse">
            <thead className="sticky top-0 bg-card border-b" style={{ borderColor: 'var(--color-border)' }}>
              <tr>
                <th className="text-left px-3 py-2 font-medium text-muted-foreground">URL</th>
                <th className="text-center px-3 py-2 font-medium text-muted-foreground">Verdict</th>
                <th className="text-left px-3 py-2 font-medium text-muted-foreground">Coverage</th>
                <th className="text-left px-3 py-2 font-medium text-muted-foreground">Action</th>
                <th className="text-right px-3 py-2 font-medium text-muted-foreground">P</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map(r => (
                <tr
                  key={r.url}
                  className="border-b hover:bg-muted/30"
                  style={{ borderColor: 'var(--color-border)' }}
                >
                  <td className="px-3 py-1.5 max-w-xs truncate" title={r.url}>
                    {r.url.replace(/^https?:\/\/[^/]+/, '') || r.url}
                  </td>
                  <td className={`px-3 py-1.5 text-center font-medium ${VERDICT_COLOR[r.verdict ?? ''] ?? ''}`}>
                    {r.verdict}
                  </td>
                  <td className="px-3 py-1.5 max-w-[180px] truncate text-muted-foreground" title={r.coverage_state ?? undefined}>
                    {r.coverage_state}
                  </td>
                  <td className="px-3 py-1.5 max-w-[200px] truncate" title={r.action ?? undefined}>
                    {r.action}
                  </td>
                  <td className="px-3 py-1.5 text-right">{r.priority}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  )
}
