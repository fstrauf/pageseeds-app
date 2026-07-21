import { useState, useEffect, useCallback } from 'react'
import {
  BarChart3,
  CheckCircle2,
  AlertTriangle,
  FileX,
  Type,
  AlignLeft,
  MessageSquareQuote,
  HelpCircle,
  RefreshCw,
  ArrowRight,
} from 'lucide-react'
import { useErrorHandler } from '../../lib/toast-context'
import { getCtrHealthSummary, listCtrOutcomes } from '../../lib/tauri'
import type { CtrHealthSummary, CtrHealthArticle, CtrOutcome } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { ScrollArea } from '@/components/ui/scroll-area'
import { Separator } from '@/components/ui/separator'

interface CtrHealthPanelProps {
  projectId: string
  runCompletedTick?: number
}

const ISSUE_ICONS: Record<string, React.ReactNode> = {
  file_not_found: <FileX size={14} className="text-amber-600" />,
  title_too_long: <Type size={14} className="text-rose-600" />,
  meta_too_short: <AlignLeft size={14} className="text-sky-600" />,
  snippet_suboptimal: <MessageSquareQuote size={14} className="text-violet-600" />,
  missing_faq_schema: <HelpCircle size={14} className="text-emerald-600" />,
}

const ISSUE_LABELS: Record<string, string> = {
  file_not_found: 'Missing file',
  title_too_long: 'Title too long',
  meta_too_short: 'Meta too short',
  snippet_suboptimal: 'Snippet suboptimal',
  missing_faq_schema: 'Missing FAQ',
}

const ISSUE_BADGE: Record<string, string> = {
  file_not_found: 'bg-amber-50 text-amber-700 border-amber-200',
  title_too_long: 'bg-rose-50 text-rose-700 border-rose-200',
  meta_too_short: 'bg-sky-50 text-sky-700 border-sky-200',
  snippet_suboptimal: 'bg-violet-50 text-violet-700 border-violet-200',
  missing_faq_schema: 'bg-emerald-50 text-emerald-700 border-emerald-200',
}

const AUDIT_STATUS_LABELS: Record<string, string> = {
  needs_fix: 'Needs fix',
  improved_since_last_audit: 'Improved',
  already_improved: 'Already improved',
  already_healthy: 'Already healthy',
  healthy: 'Healthy',
  regressed: 'Regressed',
}

const AUDIT_STATUS_BADGE: Record<string, string> = {
  needs_fix: 'bg-amber-50 text-amber-700 border-amber-200',
  improved_since_last_audit: 'bg-emerald-50 text-emerald-700 border-emerald-200',
  already_improved: 'bg-emerald-50 text-emerald-700 border-emerald-200',
  already_healthy: 'bg-secondary text-secondary-foreground border-border',
  healthy: 'bg-secondary text-secondary-foreground border-border',
  regressed: 'bg-rose-50 text-rose-700 border-rose-200',
}

const OUTCOME_STATUS_LABELS: Record<string, string> = {
  improved: 'Improved',
  regressed: 'Regressed',
  neutral: 'Neutral',
  insufficient_data: 'Insufficient data',
  deployment_unverified: 'Deployment unverified',
  deployed: 'Deployed',
  pending: 'Pending',
}

const OUTCOME_STATUS_BADGE: Record<string, string> = {
  improved: 'bg-emerald-50 text-emerald-700 border-emerald-200',
  regressed: 'bg-rose-50 text-rose-700 border-rose-200',
  neutral: 'bg-secondary text-secondary-foreground border-border',
  insufficient_data: 'bg-sky-50 text-sky-700 border-sky-200',
  deployment_unverified: 'bg-amber-50 text-amber-700 border-amber-200',
  deployed: 'bg-secondary text-secondary-foreground border-border',
  pending: 'bg-secondary text-secondary-foreground border-border',
}

function StatCard({
  label,
  value,
  icon,
  variant,
}: {
  label: string
  value: number
  icon: React.ReactNode
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
    <Card className={`border ${bg}`}>
      <CardContent className="p-3 flex items-center gap-3">
        <div className="shrink-0">{icon}</div>
        <div>
          <div className={`text-lg font-semibold ${text}`}>{value}</div>
          <div className="text-xs text-muted-foreground">{label}</div>
        </div>
      </CardContent>
    </Card>
  )
}

function ArticleRow({ article }: { article: CtrHealthArticle }) {
  return (
    <div className="flex items-start gap-2 py-2">
      {article.audit_status === 'regressed' ? (
        <AlertTriangle size={14} className="text-rose-500 mt-0.5 shrink-0" />
      ) : article.healthy ? (
        <CheckCircle2 size={14} className="text-emerald-500 mt-0.5 shrink-0" />
      ) : (
        <AlertTriangle size={14} className="text-amber-500 mt-0.5 shrink-0" />
      )}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5 flex-wrap">
          <div className="text-xs text-foreground truncate max-w-full">
            {article.title || article.url_slug}
          </div>
          <Badge
            variant="outline"
            className={`text-[10px] px-1 py-0 h-auto ${AUDIT_STATUS_BADGE[article.audit_status] || 'bg-secondary text-secondary-foreground border-border'}`}
          >
            {AUDIT_STATUS_LABELS[article.audit_status] || article.audit_status}
          </Badge>
        </div>
        <div className="text-[10px] text-muted-foreground truncate">{article.file}</div>
        {article.last_audited_at && (
          <div className="text-[10px] text-muted-foreground mt-0.5">
            Last audited {new Date(article.last_audited_at).toLocaleString()}
          </div>
        )}
        {article.resolved_issues.length > 0 && (
          <div className="flex flex-wrap gap-1 mt-1">
            {article.resolved_issues.map((issue) => (
              <Badge
                key={`resolved-${issue}`}
                variant="outline"
                className="text-[10px] px-1 py-0 h-auto bg-emerald-50 text-emerald-700 border-emerald-200"
              >
                Fixed: {ISSUE_LABELS[issue] || issue}
              </Badge>
            ))}
          </div>
        )}
        {!article.healthy && (
          <div className="flex flex-wrap gap-1 mt-1">
            {article.issues.map((issue) => (
              <Badge
                key={issue}
                variant="outline"
                className={`text-[10px] px-1 py-0 h-auto ${ISSUE_BADGE[issue] || 'bg-secondary text-secondary-foreground'}`}
              >
                <span className="mr-1">{ISSUE_ICONS[issue]}</span>
                {ISSUE_LABELS[issue] || issue}
              </Badge>
            ))}
          </div>
        )}
      </div>
    </div>
  )
}

function formatCtr(ctr: number): string {
  return `${(ctr * 100).toFixed(2)}%`
}

function OutcomeRow({
  outcome,
  articles,
}: {
  outcome: CtrOutcome
  articles: CtrHealthArticle[]
}) {
  const articleId = Number(outcome.article_id)
  const article = articles.find((a) => a.id === articleId)
  const label = article?.title || article?.url_slug || `Article #${articleId}`
  const reviewed = outcome.outcome_status !== 'pending' && outcome.outcome_status !== 'deployed'

  return (
    <div className="flex items-start gap-2 py-2">
      {outcome.outcome_status === 'regressed' ? (
        <AlertTriangle size={14} className="text-rose-500 mt-0.5 shrink-0" />
      ) : outcome.outcome_status === 'improved' ? (
        <CheckCircle2 size={14} className="text-emerald-500 mt-0.5 shrink-0" />
      ) : (
        <BarChart3 size={14} className="text-muted-foreground mt-0.5 shrink-0" />
      )}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-1.5 flex-wrap">
          <div className="text-xs text-foreground truncate max-w-full">{label}</div>
          <Badge
            variant="outline"
            className={`text-[10px] px-1 py-0 h-auto ${OUTCOME_STATUS_BADGE[outcome.outcome_status] || 'bg-secondary text-secondary-foreground border-border'}`}
          >
            {OUTCOME_STATUS_LABELS[outcome.outcome_status] || outcome.outcome_status}
          </Badge>
        </div>
        <div className="text-[10px] text-muted-foreground mt-0.5">
          Baseline: {outcome.baseline_clicks} clicks, {formatCtr(outcome.baseline_ctr)} CTR
          {reviewed && outcome.after_clicks != null && outcome.after_ctr != null && (
            <>
              {' → '}
              {outcome.after_clicks} clicks, {formatCtr(outcome.after_ctr)} CTR
            </>
          )}
        </div>
        {outcome.reviewed_at && (
          <div className="text-[10px] text-muted-foreground">
            Reviewed {new Date(outcome.reviewed_at).toLocaleString()}
          </div>
        )}
      </div>
    </div>
  )
}

export function CtrHealthPanel({ projectId, runCompletedTick }: CtrHealthPanelProps) {
  const [summary, setSummary] = useState<CtrHealthSummary | null>(null)
  const [outcomes, setOutcomes] = useState<CtrOutcome[]>([])
  const [loading, setLoading] = useState(false)
  const { showError } = useErrorHandler()

  const load = useCallback(async () => {
    if (!projectId) return
    setLoading(true)
    try {
      const [summaryResult, outcomesResult] = await Promise.all([
        getCtrHealthSummary(projectId),
        listCtrOutcomes(projectId),
      ])
      setSummary(summaryResult)
      setOutcomes(outcomesResult)
    } catch (e: unknown) {
      showError(String(e))
    } finally {
      setLoading(false)
    }
  }, [projectId, showError])

  useEffect(() => {
    load()
  }, [load])

  useEffect(() => {
    if (runCompletedTick && runCompletedTick > 0) {
      load()
    }
  }, [runCompletedTick, load])

  const healthyPct = summary
    ? Math.round((summary.healthy_count / Math.max(summary.total_articles, 1)) * 100)
    : 0

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="px-6 py-4 border-b border-border shrink-0 flex items-center justify-between">
        <div>
          <h2 className="text-sm font-semibold flex items-center gap-2">
            <BarChart3 size={16} />
            CTR Health Summary
          </h2>
          <p className="text-xs text-muted-foreground mt-0.5">
            Live scan of titles, meta descriptions, snippets, and FAQ schema
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          onClick={load}
          disabled={loading}
          className="h-8 text-xs gap-1.5"
        >
          <RefreshCw size={12} className={loading ? 'animate-spin' : ''} />
          {loading ? 'Scanning…' : 'Refresh'}
        </Button>
      </div>

      <ScrollArea className="flex-1">
        <div className="p-6 space-y-5">
          {!summary && !loading && (
            <div className="text-xs text-muted-foreground py-4">
              No data yet. Click Refresh to scan.
            </div>
          )}

          {summary && (
            <>
              {/* Top stats row */}
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
                <StatCard
                  label="Total articles"
                  value={summary.total_articles}
                  icon={<BarChart3 size={18} className="text-muted-foreground" />}
                  variant="default"
                />
                <StatCard
                  label="Healthy"
                  value={summary.healthy_count}
                  icon={<CheckCircle2 size={18} className="text-emerald-600" />}
                  variant="success"
                />
                <StatCard
                  label="Need fixes"
                  value={summary.unhealthy_count}
                  icon={<AlertTriangle size={18} className="text-amber-600" />}
                  variant="warning"
                />
                <StatCard
                  label="Health score"
                  value={healthyPct}
                  icon={<ArrowRight size={18} className="text-muted-foreground" />}
                  variant={healthyPct >= 80 ? 'success' : healthyPct >= 50 ? 'default' : 'warning'}
                />
              </div>

              <Card>
                <CardHeader className="pb-2">
                  <CardTitle className="text-xs font-medium">Audit state</CardTitle>
                </CardHeader>
                <CardContent className="pt-0">
                  <div className="grid grid-cols-2 sm:grid-cols-4 gap-2">
                    {[
                      { label: 'Improved', value: summary.improved_count, className: 'bg-emerald-50 text-emerald-700 border-emerald-200' },
                      { label: 'Already good', value: summary.already_healthy_count, className: 'bg-secondary text-secondary-foreground border-border' },
                      { label: 'Need fixes', value: summary.unhealthy_count, className: 'bg-amber-50 text-amber-700 border-amber-200' },
                      { label: 'Regressed', value: summary.regressed_count, className: 'bg-rose-50 text-rose-700 border-rose-200' },
                    ].map((item) => (
                      <div key={item.label} className={`rounded-md border px-2 py-1.5 ${item.className}`}>
                        <div className="text-sm font-medium">{item.value}</div>
                        <div className="text-[10px] opacity-80">{item.label}</div>
                      </div>
                    ))}
                  </div>
                </CardContent>
              </Card>

              {/* Issue breakdown */}
              <Card>
                <CardHeader className="pb-2">
                  <CardTitle className="text-xs font-medium">Issue breakdown</CardTitle>
                </CardHeader>
                <CardContent className="pt-0">
                  <div className="grid grid-cols-2 sm:grid-cols-5 gap-2">
                    {[
                      { label: 'Missing files', value: summary.missing_files, icon: ISSUE_ICONS.file_not_found },
                      { label: 'Title issues', value: summary.title_issues, icon: ISSUE_ICONS.title_too_long },
                      { label: 'Meta issues', value: summary.meta_issues, icon: ISSUE_ICONS.meta_too_short },
                      { label: 'Snippet issues', value: summary.snippet_issues, icon: ISSUE_ICONS.snippet_suboptimal },
                      { label: 'FAQ issues', value: summary.faq_issues, icon: ISSUE_ICONS.missing_faq_schema },
                    ].map((item) => (
                      <div
                        key={item.label}
                        className="flex items-center gap-2 px-2 py-1.5 rounded-md bg-secondary/50"
                      >
                        <span className="shrink-0">{item.icon}</span>
                        <div>
                          <div className="text-sm font-medium">{item.value}</div>
                          <div className="text-[10px] text-muted-foreground">{item.label}</div>
                        </div>
                      </div>
                    ))}
                  </div>
                </CardContent>
              </Card>

              {/* Fix outcome tracking */}
              {outcomes.length > 0 && (
                <Card>
                  <CardHeader className="pb-2">
                    <CardTitle className="text-xs font-medium">
                      Fix outcomes ({outcomes.length})
                    </CardTitle>
                  </CardHeader>
                  <CardContent className="pt-0 space-y-0.5">
                    {outcomes.map((outcome) => (
                      <OutcomeRow
                        key={`${outcome.article_id}-${outcome.fix_task_id}`}
                        outcome={outcome}
                        articles={summary?.articles ?? []}
                      />
                    ))}
                  </CardContent>
                </Card>
              )}

              {/* Last audit + action */}
              <div className="flex items-center justify-between text-xs text-muted-foreground">
                <span>
                  Last audit:{' '}
                  {summary.last_audit_at
                    ? new Date(summary.last_audit_at).toLocaleString()
                    : 'Never'}
                </span>
                {summary.regressed_count > 0 && (
                  <span className="text-rose-700">
                    {summary.regressed_count} article{summary.regressed_count !== 1 ? 's' : ''} regressed since the last audit.
                  </span>
                )}
                {summary.regressed_count === 0 && summary.unhealthy_count > 0 && (
                  <span className="text-amber-700">
                    Run a CTR Audit task to generate fix recommendations for{' '}
                    {summary.unhealthy_count} article
                    {summary.unhealthy_count !== 1 ? 's' : ''}.
                  </span>
                )}
                {summary.unhealthy_count === 0 && summary.improved_count > 0 && (
                  <span className="text-emerald-700">
                    {summary.improved_count} article{summary.improved_count !== 1 ? 's have' : ' has'} improved since the last audit.
                  </span>
                )}
                {summary.unhealthy_count === 0 && summary.improved_count === 0 && summary.total_articles > 0 && (
                  <span className="text-emerald-700">
                    All articles look good. No CTR audit needed.
                  </span>
                )}
              </div>

              <Separator />

              {/* Article list */}
              <div>
                <h3 className="text-xs font-medium mb-2">
                  Articles ({summary.total_articles})
                </h3>
                <div className="space-y-0.5">
                  {summary.articles.map((article) => (
                    <ArticleRow key={article.id} article={article} />
                  ))}
                </div>
              </div>
            </>
          )}
        </div>
      </ScrollArea>
    </div>
  )
}
