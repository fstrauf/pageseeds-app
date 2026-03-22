import { useState } from 'react'
import { seoGetBacklinks } from '../../lib/tauri'
import type { BacklinksResult, BacklinkItem, DomainOverview } from '../../lib/types'

interface Props {
  projectId: string
}

function OverviewCard({ overview, domain: _domain }: { overview?: DomainOverview; domain: string }) {
  if (!overview) return null
  return (
    <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 mb-4">
      {[
        { label: 'Domain Rating', value: overview.domain_rating?.toFixed(0) ?? '—' },
        { label: 'Est. Traffic', value: overview.traffic?.toLocaleString() ?? '—' },
        { label: 'Referring Domains', value: overview.referring_domains?.toLocaleString() ?? '—' },
        { label: 'Backlinks', value: overview.backlinks?.toLocaleString() ?? '—' },
      ].map(({ label, value }) => (
        <div key={label} className="rounded border border-border bg-card p-3">
          <div className="text-xs text-muted-foreground mb-1">{label}</div>
          <div className="text-xl font-bold" style={{ color: 'var(--color-text)' }}>
            {value}
          </div>
        </div>
      ))}
    </div>
  )
}

function BacklinkTable({ backlinks }: { backlinks: BacklinkItem[] }) {
  if (backlinks.length === 0) {
    return <p className="text-sm text-muted-foreground py-4">No backlinks found.</p>
  }
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-sm border-collapse">
        <thead>
          <tr className="border-b border-border text-left text-xs text-muted-foreground">
            <th className="py-2 pr-3 font-medium w-8">DR</th>
            <th className="py-2 pr-3 font-medium">Source</th>
            <th className="py-2 pr-3 font-medium">Anchor</th>
            <th className="py-2 font-medium">Flags</th>
          </tr>
        </thead>
        <tbody>
          {backlinks.map((bl, i) => (
            <tr key={i} className="border-b border-border/50 hover:bg-secondary/30">
              <td className="py-2 pr-3 font-medium text-primary">{bl.domain_rating.toFixed(0)}</td>
              <td className="py-2 pr-3 max-w-xs">
                <div
                  className="text-xs truncate"
                  style={{ color: 'var(--color-text)' }}
                  title={bl.url_from}
                >
                  {bl.title || bl.url_from}
                </div>
                {bl.title && (
                  <div className="text-xs text-muted-foreground truncate" title={bl.url_from}>
                    {bl.url_from}
                  </div>
                )}
              </td>
              <td className="py-2 pr-3 text-muted-foreground text-xs max-w-xs">
                <span className="truncate block" title={bl.anchor}>
                  {bl.anchor || '(no anchor)'}
                </span>
              </td>
              <td className="py-2">
                <div className="flex gap-1">
                  {bl.edu && (
                    <span className="text-xs px-1 py-0.5 rounded bg-blue-100 text-blue-700">
                      .edu
                    </span>
                  )}
                  {bl.gov && (
                    <span className="text-xs px-1 py-0.5 rounded bg-orange-500/15 text-orange-400">
                      .gov
                    </span>
                  )}
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

export function BacklinkView({ projectId }: Props) {
  const [domain, setDomain] = useState('')
  const [result, setResult] = useState<BacklinksResult | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function fetch() {
    const d = domain.trim().replace(/^https?:\/\//, '').replace(/\/$/, '')
    if (!d) return
    setLoading(true)
    setError(null)
    setResult(null)
    try {
      const data = await seoGetBacklinks(projectId, d)
      setResult(data)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Controls */}
      <div
        className="flex gap-2 items-end flex-wrap p-3 border-b shrink-0"
        style={{ borderColor: 'var(--color-border)' }}
      >
        <div className="flex flex-col gap-1 flex-1 min-w-40">
          <label className="text-xs text-muted-foreground">Domain</label>
          <input
            className="h-8 px-2 rounded border border-border bg-card text-sm"
            value={domain}
            onChange={e => setDomain(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && fetch()}
            placeholder="e.g. ahrefs.com"
          />
        </div>
        <button
          className="h-8 px-3 rounded bg-primary text-primary-foreground text-sm font-medium disabled:opacity-50"
          onClick={fetch}
          disabled={!domain.trim() || loading}
        >
          {loading ? 'Solving CAPTCHA…' : 'Get Backlinks'}
        </button>
      </div>

      {error && (
        <div className="mx-3 mt-3 rounded border border-red-200 bg-red-100 px-3 py-2 text-sm text-red-700">
          {error}
        </div>
      )}

      <div className="flex-1 overflow-y-auto p-3">
        {result && (
          <>
            <OverviewCard overview={result.overview} domain={result.domain} />
            <div className="text-xs font-medium text-muted-foreground mb-2 uppercase tracking-wide">
              Top Backlinks ({result.backlinks.length})
            </div>
            <BacklinkTable backlinks={result.backlinks} />
          </>
        )}

        {!result && !loading && (
          <p className="text-sm text-muted-foreground">
            Enter a domain to see its backlink profile.
            <br />
            Requires <code className="text-xs">CAPSOLVER_API_KEY</code> in your secrets file.
          </p>
        )}
      </div>
    </div>
  )
}
