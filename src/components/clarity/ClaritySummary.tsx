import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../ui/card'
import { Badge } from '../ui/badge'
import { Button } from '../ui/button'
import { ExternalLink } from 'lucide-react'
import { clarityGetSummary } from '../../lib/tauri'
import { useQuery } from '../../hooks/useQuery'
import type { ClarityFindingPayload, ClarityPageScorePayload, ClaritySummaryPayload, Project } from '../../lib/types'

interface Props {
  project?: Project
  version?: number
}

export function ClaritySummary({ project, version }: Props) {
  const queryKey = `clarity-summary-${project?.id ?? 'none'}-${version ?? 0}`
  const { data: summary, isLoading: loading, error } = useQuery<ClaritySummaryPayload | null>(
    queryKey,
    () => (project ? clarityGetSummary(project) : Promise.resolve(null)),
    { enabled: !!project },
  )

  if (loading) return <p className="text-sm text-muted-foreground">Loading Clarity summary…</p>
  if (error) return <p className="text-sm text-destructive">{error.message}</p>
  if (!summary) {
    return (
      <div className="space-y-4">
        <p className="text-sm text-muted-foreground">
          No Clarity summary available yet. Make sure Clarity is connected in the Settings tab,
          then run the <strong>collect_clarity</strong> and <strong>investigate_clarity</strong> tasks.
        </p>
      </div>
    )
  }

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-lg font-semibold">Clarity Findings</h2>
          <p className="text-sm text-muted-foreground">
            Generated {new Date(summary.generated_at).toLocaleString()} ·{' '}
            {summary.days_analyzed} days analyzed
          </p>
        </div>
      </div>

      {summary.top_findings.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          No significant anomalies detected. Run investigate_clarity again after more data is collected.
        </p>
      ) : (
        <div className="space-y-4">
          {summary.top_findings.map((finding, idx) => (
            <FindingCard key={idx} finding={finding} />
          ))}
        </div>
      )}

      {summary.page_scores.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">Top Anomalous Pages</CardTitle>
            <CardDescription>
              Pages ranked by behavioral anomaly score (rage clicks, dead clicks, quickbacks).
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-3">
            {summary.page_scores.map(page => (
              <PageScoreRow key={page.url} page={page} />
            ))}
          </CardContent>
        </Card>
      )}
    </div>
  )
}

function FindingCard({ finding }: { finding: ClarityFindingPayload }) {
  const severityColor =
    finding.severity === 'high'
      ? 'destructive'
      : finding.severity === 'medium'
      ? 'default'
      : 'secondary'

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <CardTitle className="text-base">{finding.issue_type}</CardTitle>
            <Badge variant={severityColor}>{finding.severity}</Badge>
          </div>
          <Button variant="ghost" size="sm" asChild>
            <a
              href={finding.clarity_dashboard_url}
              target="_blank"
              rel="noopener noreferrer"
              className="flex items-center gap-1"
            >
              Watch in Clarity <ExternalLink className="h-3 w-3" />
            </a>
          </Button>
        </div>
        <CardDescription>{finding.url}</CardDescription>
      </CardHeader>
      <CardContent className="space-y-2">
        <p className="text-sm">
          <span className="font-medium">Evidence:</span> {finding.evidence}
        </p>
        <p className="text-sm">
          <span className="font-medium">Recommendation:</span> {finding.recommendation}
        </p>
      </CardContent>
    </Card>
  )
}

function PageScoreRow({ page }: { page: ClarityPageScorePayload }) {
  return (
    <div className="flex items-center justify-between border-b last:border-0 pb-3 last:pb-0">
      <div className="min-w-0 flex-1">
        <p className="text-sm font-medium truncate">{page.url}</p>
        <p className="text-xs text-muted-foreground">
          {page.total_sessions.toLocaleString()} sessions · z-score {page.z_score.toFixed(2)}
        </p>
      </div>
      <div className="flex items-center gap-4 text-xs text-muted-foreground">
        <span>rage {(page.rage_click_rate * 100).toFixed(1)}%</span>
        <span>dead {(page.dead_click_rate * 100).toFixed(1)}%</span>
        <span>quick {(page.quickback_rate * 100).toFixed(1)}%</span>
        <Button variant="ghost" size="sm" asChild>
          <a
            href={page.clarity_dashboard_url}
            target="_blank"
            rel="noopener noreferrer"
          >
            <ExternalLink className="h-3 w-3" />
          </a>
        </Button>
      </div>
    </div>
  )
}
