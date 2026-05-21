import { useCallback, useEffect, useMemo, useState } from 'react'
import {
  Activity,
  AlertTriangle,
  BarChart3,
  BookOpen,
  CheckCircle2,
  ChevronRight,
  Copy,
  ExternalLink,
  FileText,
  GitMerge,
  Globe,
  HeartPulse,
  Loader2,
  Play,
  RefreshCw,
  ShieldAlert,
  Type,
  Wrench,
  XCircle,
} from 'lucide-react'
import { useErrorHandler } from '@/lib/toast-context'
import {
  getCannibalizationStrategy,
  getContentAuditReport,
  getCtrHealthSummary,
  getIndexingHealthSummary,
  runHealthAudit,
} from '@/lib/tauri'
import type { CtrHealthSummary, StrategyWithReviews } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Separator } from '@/components/ui/separator'
import { cn } from '@/lib/utils'

// ═══════════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════════

interface ContentAuditReport {
  generated_at: string | null
  total_audited: number
  health_summary: { good: number; needs_improvement: number; poor: number }
  articles: ContentAuditArticle[]
}

interface ContentAuditArticle {
  id: number
  title: string
  slug: string
  file: string
  health: string
  priority_score: number
  checks: Record<string, CheckResult>
  md5_body_hash?: string
}

interface CheckResult {
  pass?: boolean
  value?: unknown
  label: string
}

interface IndexingSummary {
  total_urls: number
  indexed: number
  not_indexed: number
  issues_by_reason: Array<[string, number]>
  last_inspected_at: string | null
}

interface HealthData {
  contentAudit: ContentAuditReport | null
  ctrHealth: CtrHealthSummary | null
  cannibalization: StrategyWithReviews | null
  indexing: IndexingSummary | null
  loading: boolean
  error: string | null
}

// ═══════════════════════════════════════════════════════════════════════════════
// Helpers
// ═══════════════════════════════════════════════════════════════════════════════

function timeAgo(iso: string | null | undefined): string {
  if (!iso) return 'never'
  const diffMs = Date.now() - new Date(iso).getTime()
  if (diffMs < 0) return 'just now'
  const mins = Math.floor(diffMs / 60000)
  if (mins < 1) return 'just now'
  if (mins < 60) return `${mins}m ago`
  const hrs = Math.floor(mins / 60)
  if (hrs < 24) return `${hrs}h ago`
  const days = Math.floor(hrs / 24)
  return `${days}d ago`
}

function severityClass(severity: 'critical' | 'warning' | 'info'): string {
  switch (severity) {
    case 'critical':
      return 'bg-rose-50 border-rose-200 text-rose-700'
    case 'warning':
      return 'bg-amber-50 border-amber-200 text-amber-700'
    case 'info':
      return 'bg-sky-50 border-sky-200 text-sky-700'
  }
}

function severityIcon(severity: 'critical' | 'warning' | 'info') {
  switch (severity) {
    case 'critical':
      return <ShieldAlert size={14} className="text-rose-600" />
    case 'warning':
      return <AlertTriangle size={14} className="text-amber-600" />
    case 'info':
      return <InfoIcon size={14} className="text-sky-600" />
  }
}

function InfoIcon({ size, className }: { size: number; className?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
    >
      <circle cx="12" cy="12" r="10" />
      <line x1="12" y1="16" x2="12" y2="12" />
      <line x1="12" y1="8" x2="12.01" y2="8" />
    </svg>
  )
}

// ═══════════════════════════════════════════════════════════════════════════════
// Data extraction helpers
// ═══════════════════════════════════════════════════════════════════════════════

interface PriorityIssue {
  id: string
  severity: 'critical' | 'warning' | 'info'
  title: string
  description: string
  count: number
  fixType: 'auto' | 'developer' | 'review'
  actionLabel: string
  actionView: string
}

function extractPriorityIssues(
  contentAudit: ContentAuditReport | null,
  ctrHealth: CtrHealthSummary | null,
  cannibalization: StrategyWithReviews | null,
  indexing: IndexingSummary | null,
): PriorityIssue[] {
  const issues: PriorityIssue[] = []

  // Content audit issues
  if (contentAudit) {
    const articles = contentAudit.articles || []

    // Title token duplication
    const dupTitles = articles.filter(
      (a) => a.checks?.title_token_duplication?.pass === false,
    )
    if (dupTitles.length > 0) {
      issues.push({
        id: 'title-dup',
        severity: 'critical',
        title: 'Title token duplication',
        description: `${dupTitles.length} articles have repeated words in titles`,
        count: dupTitles.length,
        fixType: 'developer',
        actionLabel: 'View details',
        actionView: 'articles',
      })
    }

    // Literal template variables
    const literalVars = articles.filter(
      (a) => a.checks?.literal_template_variable?.pass === false,
    )
    if (literalVars.length > 0) {
      issues.push({
        id: 'literal-vars',
        severity: 'critical',
        title: 'Literal template variables',
        description: `${literalVars.length} articles have unrendered template variables like "| Brand |"`,
        count: literalVars.length,
        fixType: 'developer',
        actionLabel: 'View details',
        actionView: 'articles',
      })
    }

    // Temporal URLs
    const temporalUrls = articles.filter(
      (a) => a.checks?.temporal_url?.pass === false,
    )
    if (temporalUrls.length > 0) {
      issues.push({
        id: 'temporal-urls',
        severity: 'warning',
        title: 'Temporal URLs',
        description: `${temporalUrls.length} articles have time-sensitive slugs`,
        count: temporalUrls.length,
        fixType: 'auto',
        actionLabel: 'Fix content',
        actionView: 'tasks',
      })
    }

    // Page bloat
    const bloat = articles.filter((a) => a.checks?.page_bloat_proxy?.pass === false)
    if (bloat.length > 0) {
      issues.push({
        id: 'page-bloat',
        severity: 'warning',
        title: 'Page bloat',
        description: `${bloat.length} articles are excessively large`,
        count: bloat.length,
        fixType: 'review',
        actionLabel: 'View details',
        actionView: 'articles',
      })
    }

    // Exact duplicates (group by md5_body_hash)
    const hashGroups = new Map<string, ContentAuditArticle[]>()
    for (const a of articles) {
      if (a.md5_body_hash) {
        const group = hashGroups.get(a.md5_body_hash) || []
        group.push(a)
        hashGroups.set(a.md5_body_hash, group)
      }
    }
    const dupGroups = Array.from(hashGroups.values()).filter((g) => g.length > 1)
    const dupCount = dupGroups.reduce((sum, g) => sum + g.length, 0)
    if (dupCount > 0) {
      issues.push({
        id: 'exact-dupes',
        severity: 'critical',
        title: 'Exact duplicate content',
        description: `${dupCount} articles share identical body content`,
        count: dupCount,
        fixType: 'developer',
        actionLabel: 'View details',
        actionView: 'articles',
      })
    }
  }

  // CTR issues
  if (ctrHealth && ctrHealth.unhealthy_count > 0) {
    const templateIssues = ctrHealth.articles?.filter((a) =>
      a.issues?.some((i) => i.includes('template') || i.includes('title')),
    )
    if (templateIssues && templateIssues.length > 0) {
      issues.push({
        id: 'ctr-template',
        severity: 'warning',
        title: 'CTR / template issues',
        description: `${templateIssues.length} articles have title or template problems`,
        count: templateIssues.length,
        fixType: 'review',
        actionLabel: 'View CTR panel',
        actionView: 'articles',
      })
    }
  }

  // Cannibalization
  if (cannibalization?.strategy) {
    const mergeCount = cannibalization.strategy.merge_recommendations?.length || 0
    if (mergeCount > 0) {
      issues.push({
        id: 'cannibalization',
        severity: 'warning',
        title: 'Keyword cannibalization',
        description: `${mergeCount} merge clusters detected`,
        count: mergeCount,
        fixType: 'review',
        actionLabel: 'Review clusters',
        actionView: 'cannibalization',
      })
    }
  }

  // Indexing
  if (indexing && indexing.not_indexed > 0) {
    issues.push({
      id: 'indexing',
      severity: indexing.not_indexed > 10 ? 'critical' : 'warning',
      title: 'Indexing issues',
      description: `${indexing.not_indexed} of ${indexing.total_urls} URLs not indexed`,
      count: indexing.not_indexed,
      fixType: 'review',
      actionLabel: 'View GSC',
      actionView: 'gsc',
    })
  }

  // Sort by severity then count
  const severityOrder = { critical: 0, warning: 1, info: 2 }
  issues.sort((a, b) => {
    const sevDiff = severityOrder[a.severity] - severityOrder[b.severity]
    if (sevDiff !== 0) return sevDiff
    return b.count - a.count
  })

  return issues.slice(0, 5)
}

// ═══════════════════════════════════════════════════════════════════════════════
// Components
// ═══════════════════════════════════════════════════════════════════════════════

interface Props {
  projectId: string
  onViewChange?: (view: string) => void
}

export function HealthDashboard({ projectId, onViewChange }: Props) {
  const handleError = useErrorHandler()
  const [data, setData] = useState<HealthData>({
    contentAudit: null,
    ctrHealth: null,
    cannibalization: null,
    indexing: null,
    loading: true,
    error: null,
  })
  const [runningAudit, setRunningAudit] = useState(false)
  const [lastLoaded, setLastLoaded] = useState<string | null>(null)

  const loadData = useCallback(async () => {
    if (!projectId) return
    setData((d) => ({ ...d, loading: true, error: null }))
    try {
      const [contentAudit, ctrHealth, cannibalization, indexing] = await Promise.all([
        getContentAuditReport(projectId).catch(() => null),
        getCtrHealthSummary(projectId).catch(() => null),
        getCannibalizationStrategy(projectId).catch(() => null),
        getIndexingHealthSummary(projectId).catch(() => null),
      ])

      setData({
        contentAudit: contentAudit as ContentAuditReport | null,
        ctrHealth: ctrHealth as CtrHealthSummary | null,
        cannibalization: cannibalization as StrategyWithReviews | null,
        indexing: indexing as IndexingSummary | null,
        loading: false,
        error: null,
      })
      setLastLoaded(new Date().toISOString())
    } catch (e: unknown) {
      setData((d) => ({
        ...d,
        loading: false,
        error: String(e),
      }))
    }
  }, [projectId])

  useEffect(() => {
    loadData()
  }, [loadData])

  const handleRunAudit = useCallback(async () => {
    if (!projectId) return
    setRunningAudit(true)
    try {
      await runHealthAudit(projectId)
      // Data will refresh when tasks complete; for now just reload after a delay
      setTimeout(() => loadData(), 3000)
    } catch (e: unknown) {
      handleError(e)
    } finally {
      setRunningAudit(false)
    }
  }, [projectId, loadData, handleError])

  const priorityIssues = useMemo(
    () =>
      extractPriorityIssues(
        data.contentAudit,
        data.ctrHealth,
        data.cannibalization,
        data.indexing,
      ),
    [data],
  )

  const contentScore = useMemo(() => {
    if (!data.contentAudit) return null
    const { good, needs_improvement, poor } = data.contentAudit.health_summary
    const total = good + needs_improvement + poor
    if (total === 0) return null
    // Weighted score: good = 100, needs = 50, poor = 0
    const score = Math.round((good * 100 + needs_improvement * 50) / total)
    return score
  }, [data.contentAudit])

  const hasAnyData =
    data.contentAudit || data.ctrHealth || data.cannibalization || data.indexing

  return (
    <div className="flex flex-col h-full overflow-hidden">
      {/* Header */}
      <div className="px-6 py-4 border-b border-border shrink-0 flex items-center justify-between">
        <div>
          <h2 className="text-sm font-semibold text-foreground flex items-center gap-2">
            <HeartPulse size={16} className="text-primary" />
            Health Audit
          </h2>
          <p className="text-xs text-muted-foreground mt-0.5">
            {lastLoaded
              ? `Last updated: ${timeAgo(lastLoaded)}`
              : 'Load data by running an audit'}
          </p>
        </div>
        <Button
          size="sm"
          onClick={handleRunAudit}
          disabled={runningAudit || !projectId}
          className="gap-1.5"
        >
          {runningAudit ? (
            <Loader2 size={14} className="animate-spin" />
          ) : (
            <Play size={14} />
          )}
          Run Full Audit
        </Button>
      </div>

      <ScrollArea className="flex-1">
        <div className="p-6 space-y-6">
          {data.loading && !hasAnyData && (
            <div className="flex items-center justify-center py-12 text-muted-foreground text-sm">
              <Loader2 size={16} className="animate-spin mr-2" />
              Loading health data…
            </div>
          )}

          {!data.loading && !hasAnyData && (
            <EmptyState onRun={handleRunAudit} running={runningAudit} />
          )}

          {hasAnyData && (
            <>
              {/* Priority Issues */}
              {priorityIssues.length > 0 && (
                <section>
                  <h3 className="text-xs font-semibold text-muted-foreground uppercase tracking-wider mb-3">
                    Priority Issues
                  </h3>
                  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
                    {priorityIssues.map((issue) => (
                      <PriorityIssueCard
                        key={issue.id}
                        issue={issue}
                        onViewChange={onViewChange}
                      />
                    ))}
                  </div>
                </section>
              )}

              {/* Content Health */}
              {data.contentAudit && (
                <ContentHealthSection
                  audit={data.contentAudit}
                  score={contentScore}
                  onViewChange={onViewChange}
                />
              )}

              {/* CTR & Template */}
              {data.ctrHealth && (
                <CtrHealthSection health={data.ctrHealth} onViewChange={onViewChange} />
              )}

              {/* Cannibalization */}
              {data.cannibalization && (
                <CannibalizationSection
                  strategy={data.cannibalization}
                  onViewChange={onViewChange}
                />
              )}

              {/* Indexing */}
              {data.indexing && (
                <IndexingSection indexing={data.indexing} onViewChange={onViewChange} />
              )}
            </>
          )}
        </div>
      </ScrollArea>
    </div>
  )
}

// ═══════════════════════════════════════════════════════════════════════════════
// Sub-components
// ═══════════════════════════════════════════════════════════════════════════════

function EmptyState({ onRun, running }: { onRun: () => void; running: boolean }) {
  return (
    <div className="flex flex-col items-center justify-center py-16 text-center">
      <Activity size={32} className="text-muted-foreground mb-3" />
      <h3 className="text-sm font-medium text-foreground mb-1">No health data yet</h3>
      <p className="text-xs text-muted-foreground max-w-sm mb-4">
        Run a full audit to check content health, CTR, cannibalization, and indexing status.
      </p>
      <Button size="sm" onClick={onRun} disabled={running} className="gap-1.5">
        {running ? <Loader2 size={14} className="animate-spin" /> : <Play size={14} />}
        Run Full Audit
      </Button>
    </div>
  )
}

function PriorityIssueCard({
  issue,
  onViewChange,
}: {
  issue: PriorityIssue
  onViewChange?: (view: string) => void
}) {
  const fixTypeLabel =
    issue.fixType === 'auto'
      ? 'Auto-fixable'
      : issue.fixType === 'developer'
        ? 'Developer action'
        : 'Review required'

  return (
    <Card className={cn('border', severityClass(issue.severity))}>
      <CardContent className="p-3">
        <div className="flex items-start gap-2">
          <div className="shrink-0 mt-0.5">{severityIcon(issue.severity)}</div>
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-2 mb-0.5">
              <Badge variant="outline" className="text-[10px] px-1 py-0 h-auto capitalize">
                {issue.severity}
              </Badge>
              <span className="text-xs font-medium">{issue.title}</span>
            </div>
            <p className="text-xs opacity-80 mb-2">{issue.description}</p>
            <div className="flex items-center justify-between">
              <Badge variant="secondary" className="text-[10px] px-1 py-0 h-auto">
                {fixTypeLabel}
              </Badge>
              {onViewChange && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-auto py-0.5 px-1.5 text-[10px] gap-0.5"
                  onClick={() => onViewChange(issue.actionView)}
                >
                  {issue.actionLabel}
                  <ChevronRight size={10} />
                </Button>
              )}
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

function ContentHealthSection({
  audit,
  score,
  onViewChange,
}: {
  audit: ContentAuditReport
  score: number | null
  onViewChange?: (view: string) => void
}) {
  const articles = audit.articles || []

  const temporalCount = articles.filter(
    (a) => a.checks?.temporal_url?.pass === false,
  ).length
  const bloatCount = articles.filter(
    (a) => a.checks?.page_bloat_proxy?.pass === false,
  ).length
  const literalCount = articles.filter(
    (a) => a.checks?.literal_template_variable?.pass === false,
  ).length
  const dupTitleCount = articles.filter(
    (a) => a.checks?.title_token_duplication?.pass === false,
  ).length

  const hashGroups = new Map<string, number>()
  for (const a of articles) {
    if (a.md5_body_hash) {
      hashGroups.set(a.md5_body_hash, (hashGroups.get(a.md5_body_hash) || 0) + 1)
    }
  }
  const dupGroupCount = Array.from(hashGroups.values()).filter((c) => c > 1).length

  return (
    <Card className="border-border">
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-semibold flex items-center gap-2">
            <FileText size={15} className="text-primary" />
            Content Health
            {score !== null && (
              <Badge
                variant={score >= 70 ? 'default' : score >= 40 ? 'secondary' : 'destructive'}
                className="ml-1"
              >
                {score}/100
              </Badge>
            )}
          </CardTitle>
          {onViewChange && (
            <Button
              variant="ghost"
              size="sm"
              className="h-auto py-1 px-2 text-xs gap-1"
              onClick={() => onViewChange('articles')}
            >
              View details
              <ChevronRight size={12} />
            </Button>
          )}
        </div>
      </CardHeader>
      <CardContent className="pt-0">
        <div className="grid grid-cols-2 md:grid-cols-5 gap-2">
          <StatBadge
            icon={<Globe size={13} />}
            label="Temporal URLs"
            value={temporalCount}
            variant={temporalCount > 0 ? 'warning' : 'success'}
          />
          <StatBadge
            icon={<BarChart3 size={13} />}
            label="Bloat issues"
            value={bloatCount}
            variant={bloatCount > 0 ? 'warning' : 'success'}
          />
          <StatBadge
            icon={<Type size={13} />}
            label="Literal vars"
            value={literalCount}
            variant={literalCount > 0 ? 'warning' : 'success'}
          />
          <StatBadge
            icon={<AlertTriangle size={13} />}
            label="Dup titles"
            value={dupTitleCount}
            variant={dupTitleCount > 0 ? 'warning' : 'success'}
          />
          <StatBadge
            icon={<Copy size={13} />}
            label="Exact dupes"
            value={dupGroupCount}
            variant={dupGroupCount > 0 ? 'warning' : 'success'}
          />
        </div>
        <Separator className="my-3" />
        <div className="flex items-center gap-4 text-xs text-muted-foreground">
          <span className="flex items-center gap-1">
            <CheckCircle2 size={12} className="text-emerald-500" />
            {audit.health_summary.good} good
          </span>
          <span className="flex items-center gap-1">
            <AlertTriangle size={12} className="text-amber-500" />
            {audit.health_summary.needs_improvement} needs work
          </span>
          <span className="flex items-center gap-1">
            <XCircle size={12} className="text-rose-500" />
            {audit.health_summary.poor} poor
          </span>
          <span className="ml-auto">{audit.total_audited} articles audited</span>
        </div>
      </CardContent>
    </Card>
  )
}

function CtrHealthSection({
  health,
  onViewChange,
}: {
  health: CtrHealthSummary
  onViewChange?: (view: string) => void
}) {
  return (
    <Card className="border-border">
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-semibold flex items-center gap-2">
            <BarChart3 size={15} className="text-primary" />
            CTR &amp; Template Health
          </CardTitle>
          {onViewChange && (
            <Button
              variant="ghost"
              size="sm"
              className="h-auto py-1 px-2 text-xs gap-1"
              onClick={() => onViewChange('articles')}
            >
              View CTR panel
              <ChevronRight size={12} />
            </Button>
          )}
        </div>
      </CardHeader>
      <CardContent className="pt-0">
        <div className="grid grid-cols-2 md:grid-cols-4 gap-2 mb-3">
          <StatBadge
            icon={<CheckCircle2 size={13} />}
            label="Healthy"
            value={health.healthy_count}
            variant="success"
          />
          <StatBadge
            icon={<AlertTriangle size={13} />}
            label="Unhealthy"
            value={health.unhealthy_count}
            variant={health.unhealthy_count > 0 ? 'warning' : 'success'}
          />
          <StatBadge
            icon={<Type size={13} />}
            label="Title issues"
            value={health.title_issues}
            variant={health.title_issues > 0 ? 'warning' : 'success'}
          />
          <StatBadge
            icon={<FileText size={13} />}
            label="Meta issues"
            value={health.meta_issues}
            variant={health.meta_issues > 0 ? 'warning' : 'success'}
          />
        </div>
        {health.unhealthy_count > 0 && health.articles && (
          <div className="space-y-1">
            {health.articles
              .filter((a) => !a.healthy)
              .slice(0, 3)
              .map((a) => (
                <div
                  key={a.id}
                  className="flex items-center justify-between text-xs py-1 px-2 rounded bg-secondary/50"
                >
                  <span className="truncate max-w-[60%]">{a.title}</span>
                  <div className="flex gap-1">
                    {a.issues?.slice(0, 2).map((issue) => (
                      <Badge
                        key={issue}
                        variant="outline"
                        className="text-[10px] px-1 py-0 h-auto"
                      >
                        {issue}
                      </Badge>
                    ))}
                  </div>
                </div>
              ))}
          </div>
        )}
      </CardContent>
    </Card>
  )
}

function CannibalizationSection({
  strategy,
  onViewChange,
}: {
  strategy: StrategyWithReviews
  onViewChange?: (view: string) => void
}) {
  const mergeCount = strategy.strategy.merge_recommendations?.length || 0
  const hubCount = strategy.strategy.hub_recommendations?.length || 0
  const territoryCount = strategy.strategy.territory_recommendations?.length || 0

  return (
    <Card className="border-border">
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-semibold flex items-center gap-2">
            <GitMerge size={15} className="text-primary" />
            Cannibalization
          </CardTitle>
          {onViewChange && (
            <Button
              variant="ghost"
              size="sm"
              className="h-auto py-1 px-2 text-xs gap-1"
              onClick={() => onViewChange('cannibalization')}
            >
              Review clusters
              <ChevronRight size={12} />
            </Button>
          )}
        </div>
      </CardHeader>
      <CardContent className="pt-0">
        <div className="grid grid-cols-3 gap-2 mb-3">
          <StatBadge
            icon={<GitMerge size={13} />}
            label="Merge clusters"
            value={mergeCount}
            variant={mergeCount > 0 ? 'warning' : 'success'}
          />
          <StatBadge
            icon={<BookOpen size={13} />}
            label="Hub gaps"
            value={hubCount}
            variant={hubCount > 0 ? 'warning' : 'success'}
          />
          <StatBadge
            icon={<Globe size={13} />}
            label="Territories"
            value={territoryCount}
            variant={territoryCount > 0 ? 'warning' : 'success'}
          />
        </div>
        {strategy.strategy.merge_recommendations?.slice(0, 2).map((rec) => (
          <div
            key={rec.cluster_id}
            className="text-xs py-1.5 px-2 rounded bg-secondary/50 mb-1"
          >
            <span className="font-medium">{rec.cluster_name}</span>
            <span className="text-muted-foreground ml-2">
              {rec.articles?.length || 0} pages
            </span>
          </div>
        ))}
      </CardContent>
    </Card>
  )
}

function IndexingSection({
  indexing,
  onViewChange,
}: {
  indexing: IndexingSummary
  onViewChange?: (view: string) => void
}) {
  return (
    <Card className="border-border">
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-semibold flex items-center gap-2">
            <Globe size={15} className="text-primary" />
            Indexing Health
          </CardTitle>
          {onViewChange && (
            <Button
              variant="ghost"
              size="sm"
              className="h-auto py-1 px-2 text-xs gap-1"
              onClick={() => onViewChange('gsc')}
            >
              View GSC
              <ChevronRight size={12} />
            </Button>
          )}
        </div>
      </CardHeader>
      <CardContent className="pt-0">
        <div className="grid grid-cols-3 gap-2 mb-3">
          <StatBadge
            icon={<CheckCircle2 size={13} />}
            label="Indexed"
            value={indexing.indexed}
            variant="success"
          />
          <StatBadge
            icon={<XCircle size={13} />}
            label="Not indexed"
            value={indexing.not_indexed}
            variant={indexing.not_indexed > 0 ? 'warning' : 'success'}
          />
          <StatBadge
            icon={<RefreshCw size={13} />}
            label="Total URLs"
            value={indexing.total_urls}
            variant="default"
          />
        </div>
        {indexing.issues_by_reason.length > 0 && (
          <div className="space-y-1">
            {indexing.issues_by_reason.slice(0, 3).map(([reason, count]) => (
              <div
                key={reason}
                className="flex items-center justify-between text-xs py-1 px-2 rounded bg-secondary/50"
              >
                <span className="capitalize">{reason.replace(/_/g, ' ')}</span>
                <Badge variant="outline" className="text-[10px] px-1 py-0 h-auto">
                  {count}
                </Badge>
              </div>
            ))}
          </div>
        )}
        {indexing.last_inspected_at && (
          <p className="text-[10px] text-muted-foreground mt-2">
            Last inspected: {timeAgo(indexing.last_inspected_at)}
          </p>
        )}
      </CardContent>
    </Card>
  )
}

function StatBadge({
  icon,
  label,
  value,
  variant,
}: {
  icon: React.ReactNode
  label: string
  value: number
  variant: 'success' | 'warning' | 'default'
}) {
  const bg =
    variant === 'success'
      ? 'bg-emerald-50 border-emerald-200'
      : variant === 'warning'
        ? 'bg-amber-50 border-amber-200'
        : 'bg-card border-border'
  const text =
    variant === 'success'
      ? 'text-emerald-700'
      : variant === 'warning'
        ? 'text-amber-700'
        : 'text-foreground'

  return (
    <div className={`border rounded-md p-2.5 ${bg}`}>
      <div className="flex items-center gap-1.5 mb-1">
        <span className={text}>{icon}</span>
        <span className="text-[10px] text-muted-foreground uppercase tracking-wide">{label}</span>
      </div>
      <div className={`text-lg font-semibold ${text}`}>{value}</div>
    </div>
  )
}
