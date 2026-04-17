import { useState } from 'react'
import { useErrorHandler } from '../../lib/toast-context'
import { seoCheckTraffic } from '../../lib/tauri'
import type { TrafficResult, TrafficTopPage, TrafficTopKeyword, TrafficTopCountry } from '../../lib/types'

interface Props {
  projectId: string
}

const COUNTRY_OPTIONS = [
  { value: 'None', label: 'Worldwide' },
  { value: 'us', label: 'United States' },
  { value: 'uk', label: 'United Kingdom' },
  { value: 'au', label: 'Australia' },
  { value: 'ca', label: 'Canada' },
  { value: 'de', label: 'Germany' },
  { value: 'fr', label: 'France' },
  { value: 'in', label: 'India' },
  { value: 'br', label: 'Brazil' },
]

function TrafficCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded border border-border bg-card p-3">
      <div className="text-xs text-muted-foreground mb-1">{label}</div>
      <div className="text-xl font-bold" style={{ color: 'var(--color-text)' }}>
        {value}
      </div>
    </div>
  )
}

function TopPagesTable({ pages }: { pages: TrafficTopPage[] }) {
  if (pages.length === 0) return <p className="text-sm text-muted-foreground py-2">No data.</p>
  return (
    <table className="w-full text-sm border-collapse">
      <thead>
        <tr className="border-b border-border text-left text-xs text-muted-foreground">
          <th className="py-1.5 pr-3 font-medium">URL</th>
          <th className="py-1.5 pr-3 font-medium">Traffic</th>
          <th className="py-1.5 font-medium">Keywords</th>
        </tr>
      </thead>
      <tbody>
        {pages.map((p, i) => (
          <tr key={i} className="border-b border-border/50 hover:bg-secondary/30">
            <td className="py-1.5 pr-3 text-xs max-w-xs">
              <span
                className="truncate block"
                title={p.url}
                style={{ color: 'var(--color-text)' }}
              >
                {p.url ?? '—'}
              </span>
            </td>
            <td className="py-1.5 pr-3 text-muted-foreground">{p.traffic?.toLocaleString() ?? '—'}</td>
            <td className="py-1.5 text-muted-foreground">{p.keywords?.toLocaleString() ?? '—'}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

function TopKeywordsTable({ keywords }: { keywords: TrafficTopKeyword[] }) {
  if (keywords.length === 0) return <p className="text-sm text-muted-foreground py-2">No data.</p>
  return (
    <table className="w-full text-sm border-collapse">
      <thead>
        <tr className="border-b border-border text-left text-xs text-muted-foreground">
          <th className="py-1.5 pr-3 font-medium">Keyword</th>
          <th className="py-1.5 pr-3 font-medium">Traffic</th>
          <th className="py-1.5 font-medium">Position</th>
        </tr>
      </thead>
      <tbody>
        {keywords.map((k, i) => (
          <tr key={i} className="border-b border-border/50 hover:bg-secondary/30">
            <td className="py-1.5 pr-3" style={{ color: 'var(--color-text)' }}>
              {k.keyword ?? '—'}
            </td>
            <td className="py-1.5 pr-3 text-muted-foreground">{k.traffic?.toLocaleString() ?? '—'}</td>
            <td className="py-1.5 text-muted-foreground">{k.position?.toFixed(1) ?? '—'}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

function TopCountriesTable({ countries }: { countries: TrafficTopCountry[] }) {
  if (countries.length === 0) return <p className="text-sm text-muted-foreground py-2">No data.</p>
  return (
    <table className="w-full text-sm border-collapse">
      <thead>
        <tr className="border-b border-border text-left text-xs text-muted-foreground">
          <th className="py-1.5 pr-3 font-medium">Country</th>
          <th className="py-1.5 pr-3 font-medium">Traffic</th>
          <th className="py-1.5 font-medium">Share</th>
        </tr>
      </thead>
      <tbody>
        {countries.map((c, i) => (
          <tr key={i} className="border-b border-border/50 hover:bg-secondary/30">
            <td className="py-1.5 pr-3" style={{ color: 'var(--color-text)' }}>
              {c.country?.toUpperCase() ?? '—'}
            </td>
            <td className="py-1.5 pr-3 text-muted-foreground">{c.traffic?.toLocaleString() ?? '—'}</td>
            <td className="py-1.5 text-muted-foreground">
              {c.share != null ? `${(c.share * 100).toFixed(1)}%` : '—'}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

export function TrafficOverview({ projectId }: Props) {
  const [domain, setDomain] = useState('')
  const [country, setCountry] = useState('None')
  const [mode, setMode] = useState('subdomains')
  const [result, setResult] = useState<TrafficResult | null>(null)
  const [loading, setLoading] = useState(false)
  const { showError } = useErrorHandler()

  async function fetch() {
    const d = domain.trim().replace(/^https?:\/\//, '').replace(/\/$/, '')
    if (!d) return
    setLoading(true)
    setResult(null)
    try {
      const data = await seoCheckTraffic(projectId, d, mode, country)
      setResult(data)
    } catch (e) {
      showError(String(e))
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
            placeholder="e.g. pageseeds.com"
          />
        </div>
        <div className="flex flex-col gap-1">
          <label className="text-xs text-muted-foreground">Country</label>
          <select
            className="h-8 px-2 rounded border border-border bg-card text-sm"
            value={country}
            onChange={e => setCountry(e.target.value)}
          >
            {COUNTRY_OPTIONS.map(c => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
          </select>
        </div>
        <div className="flex flex-col gap-1">
          <label className="text-xs text-muted-foreground">Mode</label>
          <select
            className="h-8 px-2 rounded border border-border bg-card text-sm"
            value={mode}
            onChange={e => setMode(e.target.value)}
          >
            <option value="subdomains">Subdomains</option>
            <option value="exact">Exact URL</option>
          </select>
        </div>
        <button
          className="h-8 px-3 rounded bg-primary text-primary-foreground text-sm font-medium disabled:opacity-50"
          onClick={fetch}
          disabled={!domain.trim() || loading}
        >
          {loading ? 'Solving CAPTCHA…' : 'Check Traffic'}
        </button>
      </div>

      <div className="flex-1 overflow-y-auto p-3 space-y-4">
        {result && (
          <>
            {/* Summary cards */}
            <div className="grid grid-cols-2 gap-3">
              <TrafficCard
                label="Monthly Avg. Traffic"
                value={result.traffic.traffic_monthly_avg.toLocaleString()}
              />
              <TrafficCard
                label="Monthly Avg. Cost"
                value={`$${result.traffic.cost_monthly_avg.toLocaleString()}`}
              />
            </div>

            {/* Top Pages */}
            {result.top_pages.length > 0 && (
              <div>
                <div className="text-xs font-medium text-muted-foreground mb-2 uppercase tracking-wide">
                  Top Pages
                </div>
                <TopPagesTable pages={result.top_pages} />
              </div>
            )}

            {/* Top Keywords */}
            {result.top_keywords.length > 0 && (
              <div>
                <div className="text-xs font-medium text-muted-foreground mb-2 uppercase tracking-wide">
                  Top Keywords
                </div>
                <TopKeywordsTable keywords={result.top_keywords} />
              </div>
            )}

            {/* Top Countries */}
            {result.top_countries.length > 0 && (
              <div>
                <div className="text-xs font-medium text-muted-foreground mb-2 uppercase tracking-wide">
                  Top Countries
                </div>
                <TopCountriesTable countries={result.top_countries} />
              </div>
            )}
          </>
        )}

        {!result && !loading && (
          <p className="text-sm text-muted-foreground">
            Enter a domain to check its estimated organic traffic.
            <br />
            Requires <code className="text-xs">CAPSOLVER_API_KEY</code> in your secrets file.
          </p>
        )}
      </div>
    </div>
  )
}
