import { useMemo, useState } from 'react'
import { Globe, RefreshCw } from 'lucide-react'
import { importLiveSite, listLiveSitePages, syncLiveSiteGsc } from '../../lib/tauri'
import type { LiveSitePage, Project } from '../../lib/types'
import { useErrorHandler } from '../../lib/toast-context'
import { useQuery } from '../../hooks/useQuery'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Badge } from '@/components/ui/badge'
import {
  Table,
  TableBody,
  TableCell,
  TableFooter,
  TableHead,
  TableHeader,
  TableRow,
} from '@/components/ui/table'

interface LiveSitePageTableProps {
  projectId: string
  project?: Project
}

function isoToday() {
  return new Date().toISOString().slice(0, 10)
}

function isoNDaysAgo(days: number) {
  const date = new Date()
  date.setDate(date.getDate() - days)
  return date.toISOString().slice(0, 10)
}

export function LiveSitePageTable({ projectId, project }: LiveSitePageTableProps) {
  const { showError } = useErrorHandler()
  const [importing, setImporting] = useState(false)
  const [syncingGsc, setSyncingGsc] = useState(false)
  const [importMsg, setImportMsg] = useState<string | null>(null)
  const [gscMsg, setGscMsg] = useState<string | null>(null)
  const [query, setQuery] = useState('')

  const { data: pages = [], isLoading: loading, refetch } = useQuery(
    `live-site-pages-${projectId}`,
    () => listLiveSitePages(projectId),
    { enabled: !!projectId, staleTime: 0 },
  )

  const filteredPages = useMemo(() => {
    const needle = query.trim().toLowerCase()
    if (!needle) return pages
    return pages.filter(page =>
      page.title.toLowerCase().includes(needle) ||
      page.path.toLowerCase().includes(needle) ||
      page.url.toLowerCase().includes(needle),
    )
  }, [pages, query])

  async function handleImport() {
    if (!projectId) return
    setImporting(true)
    setImportMsg(null)
    setGscMsg(null)
    try {
      const result = await importLiveSite(projectId, 50)
      setImportMsg(
        `Imported ${result.pages_imported} page${result.pages_imported !== 1 ? 's' : ''} from ${result.discovered_urls} sitemap URL${result.discovered_urls !== 1 ? 's' : ''}${result.pages_failed > 0 ? `, with ${result.pages_failed} crawl failure${result.pages_failed !== 1 ? 's' : ''}` : ''}.`,
      )
      await refetch()
    } catch (error) {
      showError(String(error))
    } finally {
      setImporting(false)
    }
  }

  async function handleSyncGsc() {
    if (!projectId) return
    setSyncingGsc(true)
    setImportMsg(null)
    setGscMsg(null)
    try {
      const result = await syncLiveSiteGsc(projectId, isoNDaysAgo(28), isoToday(), 250)
      setGscMsg(
        `Synced GSC metrics to ${result.pages_synced} page${result.pages_synced !== 1 ? 's' : ''} (${result.pages_unmatched} unmatched, ${result.rows_fetched} rows fetched).`,
      )
      await refetch()
    } catch (error) {
      showError(String(error))
    } finally {
      setSyncingGsc(false)
    }
  }

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center justify-between border-b border-border px-6 py-4">
        <div>
          <h2 className="text-sm font-semibold text-foreground">Site Pages ({filteredPages.length})</h2>
          {project?.site_url && (
            <p className="mt-0.5 text-xs text-muted-foreground">{project.site_url}</p>
          )}
        </div>
        <div className="flex items-center gap-2">
          <Input
            value={query}
            onChange={event => setQuery(event.target.value)}
            placeholder="Filter pages"
            className="h-8 w-48 bg-card text-xs"
          />
          <Button
            variant="outline"
            size="sm"
            onClick={handleSyncGsc}
            disabled={syncingGsc}
            className="h-8 gap-1.5 text-xs"
          >
            <RefreshCw size={13} className={syncingGsc ? 'animate-spin' : ''} />
            {syncingGsc ? 'Syncing GSC…' : 'Sync GSC'}
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={handleImport}
            disabled={importing}
            className="h-8 gap-1.5 text-xs"
          >
            <Globe size={13} />
            {importing ? 'Importing…' : 'Import Site'}
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => refetch()}
            disabled={loading}
            className="text-muted-foreground"
          >
            <RefreshCw size={14} className={loading ? 'animate-spin' : ''} />
          </Button>
        </div>
      </div>

      {importMsg && (
        <div className="mx-6 mt-4 rounded-md bg-emerald-100 px-3 py-2 text-sm text-emerald-700">
          {importMsg}
        </div>
      )}

      {gscMsg && (
        <div className="mx-6 mt-4 rounded-md bg-sky-100 px-3 py-2 text-sm text-sky-700">
          {gscMsg}
        </div>
      )}

      <div className="flex-1 overflow-auto">
        <Table>
          <TableHeader>
            <TableRow className="sticky top-0 bg-card hover:bg-card border-border">
              <TableHead className="text-xs text-muted-foreground">Title</TableHead>
              <TableHead className="w-44 text-xs text-muted-foreground">Path</TableHead>
              <TableHead className="w-20 text-right text-xs text-muted-foreground">Words</TableHead>
              <TableHead className="w-24 text-right text-xs text-muted-foreground">Headings</TableHead>
              <TableHead className="w-24 text-right text-xs text-muted-foreground">Links Out</TableHead>
              <TableHead className="w-20 text-right text-xs text-muted-foreground">Clicks</TableHead>
              <TableHead className="w-20 text-right text-xs text-muted-foreground">Impr.</TableHead>
              <TableHead className="w-20 text-right text-xs text-muted-foreground">Pos.</TableHead>
              <TableHead className="w-32 text-xs text-muted-foreground">Last Crawled</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading && filteredPages.length === 0 ? (
              <TableRow>
                <TableCell colSpan={9} className="py-10 text-center text-xs text-muted-foreground">
                  Loading…
                </TableCell>
              </TableRow>
            ) : filteredPages.length === 0 ? (
              <TableRow>
                <TableCell colSpan={9} className="py-10 text-center">
                  <div className="space-y-3 text-sm text-muted-foreground">
                    <p>
                      No live-site pages imported yet. Import the sitemap for this project to build the first site inventory.
                    </p>
                    <Button variant="outline" size="sm" onClick={handleImport} disabled={importing} className="gap-1.5 text-xs">
                      <Globe size={13} />
                      {importing ? 'Importing…' : 'Import Site'}
                    </Button>
                  </div>
                </TableCell>
              </TableRow>
            ) : (
              filteredPages.map((page: LiveSitePage) => (
                <TableRow key={page.url} className="border-border align-top">
                  <TableCell>
                    <div className="max-w-xl">
                      <div className="truncate text-sm font-medium text-foreground">
                        {page.title || page.path}
                      </div>
                      <div className="mt-0.5 truncate text-xs text-muted-foreground">{page.url}</div>
                      {page.meta_description && (
                        <div className="mt-1 line-clamp-2 text-xs text-muted-foreground">{page.meta_description}</div>
                      )}
                    </div>
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground font-mono">{page.path}</TableCell>
                  <TableCell className="text-right text-xs text-muted-foreground">
                    {page.word_count > 0 ? page.word_count.toLocaleString() : '—'}
                  </TableCell>
                  <TableCell className="text-right text-xs text-muted-foreground">
                    {page.heading_count > 0 ? page.heading_count.toLocaleString() : '—'}
                  </TableCell>
                  <TableCell className="text-right text-xs text-muted-foreground">
                    {page.internal_links_out > 0 ? page.internal_links_out.toLocaleString() : '—'}
                  </TableCell>
                  <TableCell className="text-right text-xs text-muted-foreground">
                    {page.gsc_clicks != null ? Math.round(page.gsc_clicks).toLocaleString() : '—'}
                  </TableCell>
                  <TableCell className="text-right text-xs text-muted-foreground">
                    {page.gsc_impressions != null ? Math.round(page.gsc_impressions).toLocaleString() : '—'}
                  </TableCell>
                  <TableCell className="text-right text-xs text-muted-foreground">
                    {page.gsc_position != null ? page.gsc_position.toFixed(1) : '—'}
                  </TableCell>
                  <TableCell>
                    <Badge variant="outline" className="text-[10px]">
                      {new Date(page.last_crawled_at).toLocaleDateString()}
                    </Badge>
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
          {filteredPages.length > 0 && (
            <TableFooter className="bg-card border-border">
              <TableRow>
                <TableCell colSpan={9} className="py-2.5 text-xs text-muted-foreground">
                  {filteredPages.length} page{filteredPages.length !== 1 ? 's' : ''}
                  {pages.length !== filteredPages.length && ` (${pages.length} total)`}
                </TableCell>
              </TableRow>
            </TableFooter>
          )}
        </Table>
      </div>
    </div>
  )
}