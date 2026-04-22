import { useEffect, useMemo, useRef, useState, useCallback } from 'react'
import { RefreshCw, FolderSync, Settings2, Send } from 'lucide-react'
import { listArticles, importFromRepo, suggestNextArticlePublishDate } from '../../lib/tauri'
import type { Article, Project } from '../../lib/types'
import { PublishPanel } from './PublishPanel'
import { cn, formatDate } from '../../lib/utils'
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
import { useErrorHandler } from '../../lib/toast-context'
import { useQuery } from '../../hooks/useQuery'

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

type ReviewState = 'unreviewed' | 'in_review' | 'reviewed'

const REVIEW_BADGE: Record<ReviewState, string> = {
  unreviewed: 'bg-secondary text-secondary-foreground border-transparent',
  in_review: 'bg-amber-100 text-amber-700 border-transparent',
  reviewed: 'bg-emerald-100 text-emerald-700 border-transparent',
}

const REVIEW_LABEL: Record<ReviewState, string> = {
  unreviewed: 'unreviewed',
  in_review: 'in review',
  reviewed: 'reviewed',
}

const STATUS_OPTIONS = ['all', 'published', 'ready_to_publish', 'draft']

function getReviewState(article: Article): ReviewState {
  if (article.review_status === 'in_review') {
    return 'in_review'
  }
  if (article.review_status === 'reviewed' || !!article.last_reviewed_at) {
    return 'reviewed'
  }
  return 'unreviewed'
}

function getReviewMeta(article: Article, reviewState: ReviewState): string {
  if (reviewState === 'in_review') {
    return article.review_started_at ? `Started ${formatDate(article.review_started_at)}` : 'Currently in review'
  }
  if (reviewState === 'reviewed') {
    return article.last_reviewed_at ? `Last ${formatDate(article.last_reviewed_at)}` : 'Reviewed before'
  }
  return 'Not reviewed yet'
}

interface ArticleTableProps {
  projectId: string
  project?: Project
  onEditProject?: () => void
  onSelect?: (article: Article) => void
}

export function ArticleTable({ projectId, project, onEditProject, onSelect }: ArticleTableProps) {
  const { showError } = useErrorHandler()
  const [syncing, setSyncing] = useState(false)
  const [syncMsg, setSyncMsg] = useState<string | null>(null)
  const [nextSafeDate, setNextSafeDate] = useState<string | null>(null)
  const [statusFilter, setStatusFilter] = useState('all')
  const [selectedId, setSelectedId] = useState<Article['id'] | null>(null)
  const [publishOpen, setPublishOpen] = useState(false)
  const autoSyncDone = useRef(false)

  const { data: fetchedArticles = [], isLoading: loading, refetch } = useQuery(
    `articles-${projectId}`,
    () => listArticles(projectId),
    { enabled: !!projectId, staleTime: 0 }
  )

  const articles = fetchedArticles

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
    try {
      const result = await importFromRepo(projectId)
      await refetch()
      await loadNextSafeDate()
      setSyncMsg(
        result.articles_imported === 0
          ? `No articles.json found at: ${project?.path ?? projectId}/.github/automation/articles.json`
          : `Synced ${result.articles_imported} article${result.articles_imported !== 1 ? 's' : ''}.`
      )
    } catch (e: unknown) {
      showError(String(e))
    } finally {
      setSyncing(false)
    }
  }, [projectId, project, refetch, loadNextSafeDate, showError])

  useEffect(() => {
    if (!projectId) return
    autoSyncDone.current = false
    loadNextSafeDate()
  }, [projectId, loadNextSafeDate])

  useEffect(() => {
    if (!projectId || loading || autoSyncDone.current) return
    if (articles.length === 0) {
      autoSyncDone.current = true
      sync()
    }
  }, [projectId, loading, articles.length, sync])

  const filtered = useMemo(
    () => statusFilter === 'all'
      ? articles
      : articles.filter(a => a.status === statusFilter),
    [articles, statusFilter]
  )

  const reviewCounts = useMemo(
    () => articles.reduce<Record<ReviewState, number>>(
      (counts, article) => {
        counts[getReviewState(article)] += 1
        return counts
      },
      { unreviewed: 0, in_review: 0, reviewed: 0 }
    ),
    [articles]
  )

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
          {articles.length > 0 && (
            <p className="text-xs text-muted-foreground mt-0.5">
              Review queue: {reviewCounts.unreviewed} unreviewed · {reviewCounts.in_review} in review · {reviewCounts.reviewed} reviewed
            </p>
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
              refetch()
              loadNextSafeDate()
            }}
            disabled={loading}
            className="text-muted-foreground"
          >
            <RefreshCw size={14} className={loading ? 'animate-spin' : ''} />
          </Button>
        </div>
      </div>

      {syncMsg && (
        <div className={`mx-6 mt-4 px-3 py-2 rounded-md text-sm ${
          syncMsg.startsWith('No articles') ? 'bg-amber-100 text-amber-700' : 'bg-emerald-100 text-emerald-700'
        }`}>
          {syncMsg}
        </div>
      )}

      <div className="flex-1 overflow-auto">
        <Table>
          <TableHeader>
            <TableRow className="bg-card hover:bg-card border-border sticky top-0">
              <TableHead className="text-xs text-muted-foreground w-10">#</TableHead>
              <TableHead className="text-xs text-muted-foreground">Title</TableHead>
              <TableHead className="text-xs text-muted-foreground w-40">Keyword</TableHead>
              <TableHead className="text-xs text-muted-foreground w-20 text-right">Vol</TableHead>
              <TableHead className="text-xs text-muted-foreground w-28">Status</TableHead>
              <TableHead className="text-xs text-muted-foreground w-40">Review</TableHead>
              <TableHead className="text-xs text-muted-foreground w-28">Date</TableHead>
              <TableHead className="text-xs text-muted-foreground w-20 text-right">Words</TableHead>
              <TableHead className="text-xs text-muted-foreground w-16 text-center">Quality</TableHead>
              <TableHead className="text-xs text-muted-foreground w-24">Traffic</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {loading && filtered.length === 0 ? (
              <TableRow>
                <TableCell colSpan={10} className="py-10 text-center text-xs text-muted-foreground">
                  Loading…
                </TableCell>
              </TableRow>
            ) : filtered.length === 0 ? (
              <TableRow>
                <TableCell colSpan={10} className="py-10 text-center">
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
              filtered.map(article => {
                const reviewState = getReviewState(article)
                const reviewCount = Number(article.review_count ?? 0)

                return (
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
                    <TableCell>
                      <div className="flex flex-col gap-1">
                        <Badge className={cn('w-fit text-[10px]', REVIEW_BADGE[reviewState])}>
                          {REVIEW_LABEL[reviewState]}
                        </Badge>
                        <span className="text-[10px] leading-tight text-muted-foreground">
                          {getReviewMeta(article, reviewState)}
                        </span>
                        {reviewCount > 0 && (
                          <span className="text-[10px] leading-tight text-muted-foreground">
                            {reviewCount} review{reviewCount === 1 ? '' : 's'}
                          </span>
                        )}
                      </div>
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
                )
              })
            )}
          </TableBody>
          {filtered.length > 0 && (
            <TableFooter className="bg-card border-border">
              <TableRow>
                <TableCell colSpan={10} className="py-2.5 text-xs text-muted-foreground">
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
        refetch()
        loadNextSafeDate()
      }}
    />
    </>
  )
}
