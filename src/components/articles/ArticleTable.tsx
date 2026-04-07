import { useEffect, useRef, useState, useCallback } from 'react'
import { RefreshCw, FolderSync, Settings2, Send } from 'lucide-react'
import { listArticles, importFromRepo, suggestNextArticlePublishDate } from '../../lib/tauri'
import type { Article, Project } from '../../lib/types'
import { PublishPanel } from './PublishPanel'
import { cn } from '../../lib/utils'
import { Button } from '@/components/ui/button'
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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'

const STATUS_BADGE: Record<string, string> = {
  published: 'bg-emerald-100 text-emerald-700 border-transparent',
  ready_to_publish: 'bg-sky-100 text-sky-700 border-transparent',
  draft: 'bg-secondary text-secondary-foreground border-transparent',
}

const GRADE_BADGE: Record<string, string> = {
  A: 'bg-emerald-100 text-emerald-700 border-transparent',
  B: 'bg-sky-100 text-sky-700 border-transparent',
  C: 'bg-amber-100 text-amber-700 border-transparent',
  D: 'bg-orange-100 text-orange-700 border-transparent',
  F: 'bg-red-100 text-red-700 border-transparent',
}

const STATUS_OPTIONS = ['all', 'published', 'ready_to_publish', 'draft']

interface ArticleTableProps {
  projectId: string
  project?: Project
  onEditProject?: () => void
  onSelect?: (article: Article) => void
}

export function ArticleTable({ projectId, project, onEditProject, onSelect }: ArticleTableProps) {
  const [articles, setArticles] = useState<Article[]>([])
  const [loading, setLoading] = useState(false)
  const [syncing, setSyncing] = useState(false)
  const [syncMsg, setSyncMsg] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [nextSafeDate, setNextSafeDate] = useState<string | null>(null)
  const [statusFilter, setStatusFilter] = useState('all')
  const [selectedId, setSelectedId] = useState<number | null>(null)
  const [publishOpen, setPublishOpen] = useState(false)
  const autoSyncDone = useRef(false)

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const data = await listArticles(projectId)
      setArticles(data)
      return data.length
    } catch (e: unknown) {
      setError(String(e))
      return -1
    } finally {
      setLoading(false)
    }
  }, [projectId])

  const loadNextSafeDate = useCallback(async () => {
    try {
      const next = await suggestNextArticlePublishDate(projectId)
      setNextSafeDate(next)
    } catch {
      setNextSafeDate(null)
    }
  }, [projectId])

  const sync = useCallback(async () => {
    if (!projectId) return
    setSyncing(true)
    setSyncMsg(null)
    setError(null)
    try {
      const result = await importFromRepo(projectId)
      await load()
      await loadNextSafeDate()
      setSyncMsg(
        result.articles_imported === 0
          ? `No articles.json found at: ${project?.path ?? projectId}/.github/automation/articles.json`
          : `Synced ${result.articles_imported} article${result.articles_imported !== 1 ? 's' : ''}.`
      )
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setSyncing(false)
    }
  }, [projectId, project, load, loadNextSafeDate])

  useEffect(() => {
    if (!projectId) return
    autoSyncDone.current = false
    load().then(count => {
      if (count === 0 && !autoSyncDone.current) {
        autoSyncDone.current = true
        sync()
      }
    })
    loadNextSafeDate()
  }, [projectId]) // eslint-disable-line react-hooks/exhaustive-deps

  const filtered = statusFilter === 'all'
    ? articles
    : articles.filter(a => a.status === statusFilter)

  const publishCandidates = articles.filter(
    a => a.status === 'ready_to_publish' || a.status === 'draft'
  )

  function handleRowClick(article: Article) {
    setSelectedId(article.id === selectedId ? null : article.id)
    onSelect?.(article)
  }

  return (
    <>
      <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center justify-between px-6 py-4 border-b border-border">
        <div>
          <h2 className="text-sm font-semibold text-foreground">Articles ({filtered.length})</h2>
          {nextSafeDate && (
            <p className="text-xs text-muted-foreground mt-0.5">Next safe publish date: {nextSafeDate}</p>
          )}
        </div>
        <div className="flex items-center gap-2">
          <Select value={statusFilter} onValueChange={setStatusFilter}>
            <SelectTrigger className="h-7 w-32 text-xs bg-card border-border text-muted-foreground">
              <SelectValue />
            </SelectTrigger>
            <SelectContent className="bg-popover border-border text-popover-foreground">
              {STATUS_OPTIONS.map(s => (
                <SelectItem key={s} value={s} className="text-xs capitalize">
                  {s === 'all' ? 'All statuses' : s}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
          <Button
            variant="outline"
            size="sm"
            onClick={sync}
            disabled={syncing || loading}
            className="h-7 text-xs border-border text-muted-foreground hover:text-foreground gap-1.5"
            title="Sync from articles.json in the project repo"
          >
            <FolderSync size={13} className={syncing ? 'animate-spin' : ''} />
            {syncing ? 'Syncing…' : 'Sync'}
          </Button>
          {publishCandidates.length > 0 && (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setPublishOpen(true)}
              className="h-7 text-xs border-border text-muted-foreground hover:text-foreground gap-1.5"
              title="Publish draft and ready articles"
            >
              <Send size={13} />
              Publish ({publishCandidates.length})
            </Button>
          )}
          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => {
              load()
              loadNextSafeDate()
            }}
            disabled={loading}
            className="text-muted-foreground"
          >
            <RefreshCw size={14} className={loading ? 'animate-spin' : ''} />
          </Button>
        </div>
      </div>

      {error && (
        <div className="mx-6 mt-4 px-3 py-2 rounded-md text-sm bg-destructive/15 text-destructive">
          {error}
        </div>
      )}
      {syncMsg && !error && (
        <div className={`mx-6 mt-4 px-3 py-2 rounded-md text-sm ${
          syncMsg.startsWith('No articles') ? 'bg-amber-100 text-amber-700' : 'bg-emerald-100 text-emerald-700'
        }`}>
          {syncMsg}
        </div>
      )}

      <div className="flex-1 overflow-y-auto">
        <Table>
          <TableHeader>
            <TableRow className="bg-card hover:bg-card border-border sticky top-0">
              <TableHead className="text-xs text-muted-foreground w-10">#</TableHead>
              <TableHead className="text-xs text-muted-foreground">Title</TableHead>
              <TableHead className="text-xs text-muted-foreground w-40">Keyword</TableHead>
              <TableHead className="text-xs text-muted-foreground w-20 text-right">Vol</TableHead>
              <TableHead className="text-xs text-muted-foreground w-28">Status</TableHead>
              <TableHead className="text-xs text-muted-foreground w-28">Date</TableHead>
              <TableHead className="text-xs text-muted-foreground w-20 text-right">Words</TableHead>
              <TableHead className="text-xs text-muted-foreground w-16 text-center">Quality</TableHead>
              <TableHead className="text-xs text-muted-foreground w-24">Traffic</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading && filtered.length === 0 ? (
              <TableRow>
                <TableCell colSpan={9} className="py-10 text-center text-xs text-muted-foreground">
                  Loading…
                </TableCell>
              </TableRow>
            ) : filtered.length === 0 ? (
              <TableRow>
                <TableCell colSpan={9} className="py-10 text-center">
                  {syncing ? (
                    <p className="text-sm text-muted-foreground">Syncing articles from repo…</p>
                  ) : (
                    <div className="space-y-3">
                      <p className="text-sm text-muted-foreground">
                        {statusFilter !== 'all'
                          ? 'No articles match the filter.'
                          : project?.path
                            ? <>No <code className="text-xs bg-secondary px-1 rounded">.github/automation/articles.json</code> found in <code className="text-xs bg-secondary px-1 rounded">{project.path}</code></>
                            : 'No project path configured.'}
                      </p>
                      {statusFilter === 'all' && onEditProject && (
                        <button
                          onClick={onEditProject}
                          className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded text-xs border border-border text-muted-foreground hover:text-foreground"
                        >
                          <Settings2 size={12} />
                          Edit project path
                        </button>
                      )}
                    </div>
                  )}
                </TableCell>
              </TableRow>
            ) : (
              filtered.map(article => (
                <TableRow
                  key={article.id}
                  onClick={() => handleRowClick(article)}
                  className={cn(
                    'border-border cursor-pointer',
                    selectedId === article.id ? 'bg-accent/40' : 'hover:bg-accent/20',
                  )}
                >
                  <TableCell className="text-xs text-muted-foreground font-mono">
                    {article.id}
                  </TableCell>
                  <TableCell>
                    <div className="text-sm text-foreground font-medium max-w-xs truncate">
                      {article.title || article.url_slug}
                    </div>
                    <div className="text-xs text-muted-foreground font-mono truncate max-w-xs">
                      {article.url_slug}
                    </div>
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground truncate max-w-40">
                    {article.target_keyword ?? '—'}
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground text-right">
                    {article.target_volume > 0 ? article.target_volume.toLocaleString() : '—'}
                  </TableCell>
                  <TableCell>
                    <Badge className={cn('text-xs', STATUS_BADGE[article.status] ?? STATUS_BADGE.draft)}>
                      {article.status}
                    </Badge>
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {article.published_date ?? '—'}
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground text-right">
                    {article.word_count > 0 ? article.word_count.toLocaleString() : '—'}
                  </TableCell>
                  <TableCell className="text-center">
                    {article.quality_score ? (
                      <div className="flex flex-col items-center">
                        <Badge 
                          className={cn('text-[10px] px-1.5 py-0', GRADE_BADGE[article.quality_grade ?? ''] ?? 'bg-secondary text-secondary-foreground')}
                          title={`Quality Score: ${article.quality_score}/100${article.publishing_ready ? ' • Ready to publish' : ''}`}
                        >
                          {article.quality_grade}
                        </Badge>
                        <span className="text-[9px] text-muted-foreground mt-0.5">
                          {article.quality_score}
                        </span>
                      </div>
                    ) : (
                      <span className="text-xs text-muted-foreground">—</span>
                    )}
                  </TableCell>
                  <TableCell className="text-xs text-muted-foreground">
                    {article.estimated_traffic_monthly ?? '—'}
                  </TableCell>
                </TableRow>
              ))
            )}
          </TableBody>
          {filtered.length > 0 && (
            <TableFooter className="bg-card border-border">
              <TableRow>
                <TableCell colSpan={9} className="py-2.5 text-xs text-muted-foreground">
                  {filtered.length} article{filtered.length !== 1 ? 's' : ''}
                  {articles.length !== filtered.length && ` (${articles.length} total)`}
                </TableCell>
              </TableRow>
            </TableFooter>
          )}
        </Table>
      </div>
    </div>
    <PublishPanel
      open={publishOpen}
      onOpenChange={setPublishOpen}
      projectId={projectId}
      candidates={publishCandidates}
      onPublished={() => {
        load()
        loadNextSafeDate()
      }}
    />
    </>
  )
}
