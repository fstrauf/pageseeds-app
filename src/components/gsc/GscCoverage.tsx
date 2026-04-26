import { useState } from 'react'
import { useErrorHandler } from '../../lib/toast-context'
import { gscParseCoverageCsv } from '../../lib/tauri'
import type { Coverage404Record } from '../../lib/types'

const CATEGORY_COLORS: Record<string, string> = {
  'Misspelled URL': 'text-orange-500',
  'Old or deprecated content': 'text-yellow-600',
  'URL with query parameters': 'text-blue-500',
  'Malformed URL with ampersand': 'text-red-600',
  'Not found — unknown cause': 'text-muted-foreground',
}

export function GscCoverage() {
  const [csvText, setCsvText] = useState('')
  const [records, setRecords] = useState<Coverage404Record[]>([])
  const { showError } = useErrorHandler()
  const [filter, setFilter] = useState('ALL')

  async function parse() {
    if (!csvText.trim()) return
    try {
      const rows = await gscParseCoverageCsv(csvText)
      setRecords(rows)
    } catch (e) {
      showError(String(e))
    }
  }

  const categories = ['ALL', ...Array.from(new Set(records.map(r => r.category)))]
  const filtered = filter === 'ALL' ? records : records.filter(r => r.category === filter)

  return (
    <div className="flex flex-col gap-4">
      <div>
        <h2 className="text-sm font-semibold mb-2">Coverage 404 Analysis</h2>
        <p className="text-xs text-muted-foreground mb-3">
          Export the "Not found (404)" table from Google Search Console → Coverage report → Copy CSV and paste below.
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
            {categories.map(cat => {
              const count = cat === 'ALL' ? records.length : records.filter(r => r.category === cat).length
              return (
                <button
                  key={cat}
                  onClick={() => setFilter(cat)}
                  className={`px-2 py-1 rounded text-xs border ${filter === cat ? 'bg-primary text-primary-foreground border-primary' : 'border-border hover:bg-muted'}`}
                >
                  {cat} ({count})
                </button>
              )
            })}
          </div>

          <div className="overflow-auto">
            <table className="w-full text-xs border-collapse">
              <thead className="sticky top-0 bg-card border-b" style={{ borderColor: 'var(--color-border)' }}>
                <tr>
                  <th className="text-left px-3 py-2 font-medium text-muted-foreground">URL</th>
                  <th className="text-left px-3 py-2 font-medium text-muted-foreground">Reason</th>
                  <th className="text-left px-3 py-2 font-medium text-muted-foreground">Action</th>
                  <th className="text-right px-3 py-2 font-medium text-muted-foreground">P</th>
                  <th className="text-left px-3 py-2 font-medium text-muted-foreground">Last Crawled</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((r, i) => (
                  <tr key={i} className="border-b hover:bg-muted/30" style={{ borderColor: 'var(--color-border)' }}>
                    <td className="px-3 py-1.5 max-w-xs truncate font-mono text-[10px]" title={r.url}>
                      {r.path || r.url}
                    </td>
                    <td className={`px-3 py-1.5 max-w-[180px] truncate ${CATEGORY_COLORS[r.category] ?? ''}`}>
                      {r.reason}
                    </td>
                    <td className="px-3 py-1.5 max-w-[200px] truncate text-muted-foreground" title={r.suggested_action}>
                      {r.suggested_action}
                    </td>
                    <td className="px-3 py-1.5 text-right">{r.priority}</td>
                    <td className="px-3 py-1.5 text-muted-foreground">{r.last_crawled ?? '—'}</td>
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
