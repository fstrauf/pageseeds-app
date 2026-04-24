import { useMemo, useState } from 'react'
import { AlertCircle, RefreshCw } from 'lucide-react'
import { getLiveSiteAudit } from '../../lib/tauri'
import type { LiveSiteAuditPage } from '../../lib/types'
import { useQuery } from '../../hooks/useQuery'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Input } from '@/components/ui/input'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'

const AUDIT_ISSUES: Record<string, { label: string; className: string }> = {
  thin_content: {
    label: 'Thin content',
    className: 'bg-amber-100 text-amber-700 border-transparent',
  },
  missing_meta_description: {
    label: 'Missing meta',
    className: 'bg-rose-100 text-rose-700 border-transparent',
  },
  missing_h1: {
    label: 'Missing H1',
    className: 'bg-rose-100 text-rose-700 border-transparent',
  },
  weak_headings: {
    label: 'Weak headings',
    className: 'bg-sky-100 text-sky-700 border-transparent',
  },
  weak_interlinking: {
    label: 'Weak links',
    className: 'bg-indigo-100 text-indigo-700 border-transparent',
  },
  orphan_page: {
    label: 'Orphan',
    className: 'bg-orange-100 text-orange-700 border-transparent',
  },
  stale_crawl: {
    label: 'Stale crawl',
    className: 'bg-secondary text-muted-foreground border-transparent',
  },
}

const FILTERS = [
  { key: 'all', label: 'All pages' },
  { key: 'issues', label: 'Needs attention' },
  { key: 'thin_content', label: 'Thin content' },
  { key: 'missing_meta_description', label: 'Missing meta' },
  { key: 'missing_h1', label: 'Missing H1' },
  { key: 'weak_headings', label: 'Weak headings' },
  { key: 'weak_interlinking', label: 'Weak links' },
  { key: 'stale_crawl', label: 'Stale crawl' },
] as const

interface LiveSiteAuditProps {
  projectId: string
}

function formatIssueLabel(issue: string) {
  return AUDIT_ISSUES[issue]?.label ?? issue.replace(/_/g, ' ')
}

function issueBadgeClass(issue: string) {
  return AUDIT_ISSUES[issue]?.className ?? 'bg-secondary text-muted-foreground border-transparent'
}

function asNumber(value: bigint | number) {
  return Number(value)
}

export function LiveSiteAudit({ projectId }: LiveSiteAuditProps) {
  const [query, setQuery] = useState('')
  const [selectedFilter, setSelectedFilter] = useState<(typeof FILTERS)[number]['key']>('all')

  const { data, error, isLoading, refetch } = useQuery(
    `live-site-audit-${projectId}`,
    () => getLiveSiteAudit(projectId),
    { enabled: !!projectId, staleTime: 0 },
  )

  const filteredPages = useMemo(() => {
    const pages = data?.pages ?? []
    const needle = query.trim().toLowerCase()
    return pages.filter(page => {
      const matchesQuery = !needle ||
        page.title.toLowerCase().includes(needle) ||
        page.path.toLowerCase().includes(needle) ||
        page.url.toLowerCase().includes(needle)

      if (!matchesQuery) return false
      if (selectedFilter === 'all') return true
      if (selectedFilter === 'issues') return page.issue_flags.length > 0
      return page.issue_flags.includes(selectedFilter)
    })
  }, [data?.pages, query, selectedFilter])

  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="flex items-center justify-between border-b border-border px-6 py-4">
        <div>
          <h2 className="text-sm font-semibold text-foreground">Live Site Audit</h2>
          <p className="mt-0.5 text-xs text-muted-foreground">
            Deterministic crawl checks only. Agent recommendations are intentionally kept separate.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Input
            value={query}
            onChange={event => setQuery(event.target.value)}
            placeholder="Filter pages"
            className="h-8 w-48 bg-card text-xs"
          />
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => refetch()}
            disabled={isLoading}
            className="text-muted-foreground"
          >
            <RefreshCw size={14} className={isLoading ? 'animate-spin' : ''} />
          </Button>
        </div>
      </div>

      {error && (
        <div className="mx-6 mt-4 flex items-start gap-2 rounded-md bg-destructive/15 px-3 py-2.5 text-sm text-destructive">
          <AlertCircle size={14} className="mt-0.5 shrink-0" />
          {error.message}
        </div>
      )}

      <div className="flex-1 overflow-y-auto p-6 space-y-6">
        <div className="grid gap-3 md:grid-cols-2 xl:grid-cols-4">
          {[
            { label: 'Pages', value: data?.summary.total_pages ?? 0 },
            { label: 'Healthy', value: data?.summary.healthy_pages ?? 0 },
            { label: 'Needs attention', value: data?.summary.pages_with_issues ?? 0 },
            { label: 'Thin content', value: data?.summary.thin_content_pages ?? 0 },
            { label: 'Missing metadata', value: data?.summary.missing_metadata_pages ?? 0 },
            { label: 'Weak headings', value: data?.summary.weak_heading_pages ?? 0 },
            { label: 'Weak interlinking', value: data?.summary.weak_interlinking_pages ?? 0 },
            { label: 'Stale crawl', value: data?.summary.stale_crawl_pages ?? 0 },
          ].map(stat => (
            <Card key={stat.label} className="bg-card border-border">
              <CardContent className="pt-4 pb-3 px-4">
                <div className="text-xs text-muted-foreground mb-1">{stat.label}</div>
                <div className="text-2xl font-bold text-foreground">{stat.value}</div>
              </CardContent>
            </Card>
          ))}
        </div>

        <Card className="bg-card border-border">
          <CardHeader className="pb-3">
            <CardTitle className="text-sm font-semibold text-foreground">Filter audit facts</CardTitle>
          </CardHeader>
          <CardContent className="flex flex-wrap gap-2">
            {FILTERS.map(filter => (
              <Button
                key={filter.key}
                size="sm"
                variant={selectedFilter === filter.key ? 'default' : 'outline'}
                onClick={() => setSelectedFilter(filter.key)}
                className="h-8 text-xs"
              >
                {filter.label}
              </Button>
            ))}
            <Badge variant="outline" className="ml-auto text-[10px]">
              {filteredPages.length} shown
            </Badge>
          </CardContent>
        </Card>

        <Card className="bg-card border-border">
          <CardHeader className="pb-3">
            <CardTitle className="text-sm font-semibold text-foreground">
              Page facts
            </CardTitle>
          </CardHeader>
          <CardContent>
            {isLoading && filteredPages.length === 0 ? (
              <div className="py-8 text-center text-sm text-muted-foreground">Loading audit…</div>
            ) : filteredPages.length === 0 ? (
              <div className="py-8 text-center text-sm text-muted-foreground">
                No imported pages match the current filters.
              </div>
            ) : (
              <div className="rounded-lg border border-border overflow-hidden">
                <Table>
                  <TableHeader>
                    <TableRow className="bg-card hover:bg-card border-border">
                      <TableHead className="text-xs text-muted-foreground">Page</TableHead>
                      <TableHead className="w-24 text-right text-xs text-muted-foreground">Words</TableHead>
                      <TableHead className="w-28 text-xs text-muted-foreground">Metadata</TableHead>
                      <TableHead className="w-28 text-right text-xs text-muted-foreground">Headings</TableHead>
                      <TableHead className="w-28 text-right text-xs text-muted-foreground">Links</TableHead>
                      <TableHead className="w-24 text-xs text-muted-foreground">Last crawl</TableHead>
                      <TableHead className="text-xs text-muted-foreground">Issues</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {filteredPages.map((page: LiveSiteAuditPage) => {
                      const wordCount = asNumber(page.word_count)
                      const headingCount = asNumber(page.heading_count)
                      const linksOut = asNumber(page.internal_links_out)
                      const linksIn = asNumber(page.internal_links_in)
                      const crawlAgeDays = asNumber(page.crawl_age_days)

                      return (
                        <TableRow key={page.url} className="border-border align-top">
                          <TableCell>
                            <div className="max-w-md">
                              <div className="truncate text-sm font-medium text-foreground">{page.title || page.path}</div>
                              <div className="mt-0.5 truncate text-xs text-muted-foreground">{page.path}</div>
                            </div>
                          </TableCell>
                          <TableCell className="text-right text-xs text-muted-foreground">
                            {wordCount > 0 ? wordCount.toLocaleString() : '—'}
                          </TableCell>
                          <TableCell>
                            <div className="flex flex-wrap gap-1">
                              <Badge className={page.has_meta_description ? 'bg-emerald-100 text-emerald-700 border-transparent text-[10px]' : 'bg-rose-100 text-rose-700 border-transparent text-[10px]'}>
                                {page.has_meta_description ? 'Meta' : 'No meta'}
                              </Badge>
                              <Badge className={page.has_h1 ? 'bg-emerald-100 text-emerald-700 border-transparent text-[10px]' : 'bg-rose-100 text-rose-700 border-transparent text-[10px]'}>
                                {page.has_h1 ? 'H1' : 'No H1'}
                              </Badge>
                            </div>
                          </TableCell>
                          <TableCell className="text-right text-xs text-muted-foreground">
                            {headingCount > 0 ? headingCount.toLocaleString() : '—'}
                          </TableCell>
                          <TableCell className="text-right text-xs text-muted-foreground">
                            in {linksIn.toLocaleString()} / out {linksOut.toLocaleString()}
                          </TableCell>
                          <TableCell>
                            <Badge variant="outline" className="text-[10px]">
                              {crawlAgeDays === 0 ? 'today' : `${crawlAgeDays}d ago`}
                            </Badge>
                          </TableCell>
                          <TableCell>
                            {page.issue_flags.length === 0 ? (
                              <span className="text-xs text-emerald-600">Healthy</span>
                            ) : (
                              <div className="flex flex-wrap gap-1">
                                {page.issue_flags.map(issue => (
                                  <Badge key={`${page.url}-${issue}`} className={`text-[10px] ${issueBadgeClass(issue)}`}>
                                    {formatIssueLabel(issue)}
                                  </Badge>
                                ))}
                              </div>
                            )}
                          </TableCell>
                        </TableRow>
                      )
                    })}
                  </TableBody>
                </Table>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  )
}