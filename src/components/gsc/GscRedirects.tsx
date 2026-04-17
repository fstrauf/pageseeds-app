import { useState } from 'react'
import { useErrorHandler } from '../../lib/toast-context'
import { gscParseRedirectCsv } from '../../lib/tauri'
import type { RedirectRecord } from '../../lib/types'

const TYPE_COLORS: Record<string, string> = {
  'Protocol redirect': 'text-red-500',
  'www canonicalization': 'text-orange-500',
  'Trailing slash redirect': 'text-yellow-600',
  '301 Permanent redirect': 'text-blue-500',
  '302 Temporary redirect': 'text-red-500',
  'Unknown redirect': 'text-muted-foreground',
}

export function GscRedirects() {
  const [csvText, setCsvText] = useState('')
  const [records, setRecords] = useState<RedirectRecord[]>([])
  const { showError } = useErrorHandler()
  const [filter, setFilter] = useState('ALL')

  async function parse() {
    if (!csvText.trim()) return
    try {
      const rows = await gscParseRedirectCsv(csvText)
      setRecords(rows)
    } catch (e) {
      showError(String(e))
    }
  }

  const types = ['ALL', ...Array.from(new Set(records.map(r => r.redirect_type)))]
  const filtered = filter === 'ALL' ? records : records.filter(r => r.redirect_type === filter)

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h2 className="text-sm font-semibold mb-2">Redirect Analysis</h2>
        <p className="text-xs text-muted-foreground mb-3">
          Export the "Page with redirect" table from Google Search Console → Coverage report → Copy CSV and paste below.
        </p>
      </div>

      <textarea
        className="w-full h-36 px-3 py-2 rounded border border-border bg-card text-xs font-mono resize-y"
        placeholder="Paste CSV content here…"
        value={csvText}
        onChange={e => setCsvText(e.target.value)}
      />

      <div className="flex gap-2">
        <button
          onClick={parse}
          disabled={!csvText.trim()}
          className="h-8 px-3 rounded text-xs bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-40"
        >
          Parse CSV
        </button>
        {records.length > 0 && (
          <button
            onClick={() => { setRecords([]); setCsvText('') }}
            className="h-8 px-3 rounded text-xs border border-border hover:bg-muted"
          >
            Clear
          </button>
        )}
      </div>

      {records.length > 0 && (
        <>
          <div className="flex gap-1 flex-wrap">
            {types.map(t => {
              const count = t === 'ALL' ? records.length : records.filter(r => r.redirect_type === t).length
              return (
                <button
                  key={t}
                  onClick={() => setFilter(t)}
                  className={`px-2 py-1 rounded text-xs border ${filter === t ? 'bg-primary text-primary-foreground border-primary' : 'border-border hover:bg-muted'}`}
                >
                  {t} ({count})
                </button>
              )
            })}
          </div>

          <div className="overflow-auto">
            <table className="w-full text-xs border-collapse">
              <thead className="sticky top-0 bg-card border-b" style={{ borderColor: 'var(--color-border)' }}>
                <tr>
                  <th className="text-left px-3 py-2 font-medium text-muted-foreground">URL</th>
                  <th className="text-left px-3 py-2 font-medium text-muted-foreground">Type</th>
                  <th className="text-left px-3 py-2 font-medium text-muted-foreground">Issue</th>
                  <th className="text-left px-3 py-2 font-medium text-muted-foreground">Action</th>
                  <th className="text-right px-3 py-2 font-medium text-muted-foreground">P</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((r, i) => (
                  <tr key={i} className="border-b hover:bg-muted/30" style={{ borderColor: 'var(--color-border)' }}>
                    <td className="px-3 py-1.5 max-w-xs truncate font-mono text-[10px]" title={r.url}>
                      {r.url}
                    </td>
                    <td className={`px-3 py-1.5 max-w-[160px] truncate font-medium ${TYPE_COLORS[r.redirect_type] ?? ''}`}>
                      {r.redirect_type}
                    </td>
                    <td className="px-3 py-1.5 max-w-[160px] truncate text-muted-foreground" title={r.issue}>
                      {r.issue}
                    </td>
                    <td className="px-3 py-1.5 max-w-[200px] truncate" title={r.suggested_action}>
                      {r.suggested_action}
                    </td>
                    <td className="px-3 py-1.5 text-right">{r.priority}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </div>
  )
}
