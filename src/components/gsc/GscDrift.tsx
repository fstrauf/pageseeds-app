import { useCallback, useEffect, useState } from 'react'
import { gscComputeDrift, createGscIndexingRecoveryTask, getGscRecoveryStats } from '../../lib/tauri'
import { useTaskQueueActions } from '../../lib/taskQueueActions'
import type { GscDriftReport, ResubmitCandidate, DriftUrl, RecoveryStats } from '../../lib/types'
import { Card, CardContent } from '../ui/card'
import { Badge } from '../ui/badge'
import { Button } from '../ui/button'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '../ui/tabs'
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '../ui/table'
import { ScrollArea } from '../ui/scroll-area'
import { Copy, Check, Info, Link2, AlertCircle, CheckCircle2 } from 'lucide-react'

interface Props {
  projectId: string
}

export function GscDrift({ projectId }: Props) {
  const [report, setReport] = useState<GscDriftReport | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [creatingTask, setCreatingTask] = useState(false)
  const [taskMessage, setTaskMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null)
  const [recoveryStats, setRecoveryStats] = useState<RecoveryStats | null>(null)
  const { enqueueTasks } = useTaskQueueActions()

  const load = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const [data, stats] = await Promise.all([
        gscComputeDrift(projectId),
        getGscRecoveryStats(projectId).catch(() => null),
      ])
      setReport(data)
      setRecoveryStats(stats)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [projectId])

  const handleCreateRecoveryTask = async () => {
    setCreatingTask(true)
    setTaskMessage(null)
    try {
      const task = await createGscIndexingRecoveryTask(projectId)
      enqueueTasks([task])
      setTaskMessage({
        type: 'success',
        text: `Recovery campaign created and queued: ${task.title || task.type} (${task.id.slice(0, 8)}…). Child tasks will be spawned automatically after planning.`,
      })
    } catch (e) {
      const msg = String(e)
      setTaskMessage({ type: 'error', text: msg })
      console.error('Failed to create GSC indexing recovery task:', e)
    } finally {
      setCreatingTask(false)
    }
  }

  useEffect(() => {
    load()
  }, [load])

  if (loading && !report) {
    return (
      <div className="flex items-center justify-center h-full text-sm text-muted-foreground">
        Analysing sitemap ↔ GSC drift…
      </div>
    )
  }

  if (error && !report) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3">
        <p className="text-sm text-destructive">{error}</p>
        <Button variant="outline" size="sm" onClick={load}>
          Retry
        </Button>
      </div>
    )
  }

  if (!report) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-3">
        <p className="text-sm text-muted-foreground">No drift data available.</p>
        <Button variant="outline" size="sm" onClick={load}>
          Compute drift
        </Button>
      </div>
    )
  }

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <div className="px-4 py-3 border-b shrink-0 space-y-2">
        <div className="flex items-center justify-between">
          <div className="text-xs text-muted-foreground">
            Sitemap: {report.sitemap_total} URLs · GSC: {report.gsc_total} URLs · Checked{' '}
            {new Date(report.checked_at).toLocaleString()}
          </div>
          <Button variant="outline" size="sm" onClick={load} disabled={loading}>
            {loading ? 'Refreshing…' : 'Refresh'}
          </Button>
        </div>
        {/* Data freshness row */}
        <div className="flex items-center gap-3 text-[11px]">
          {report.gsc_data_age_hours !== null && report.gsc_data_age_hours !== undefined && (
            <FreshnessBadge
              label="GSC data"
              hours={report.gsc_data_age_hours}
              warnAfter={24}
            />
          )}
          {report.link_scan_age_hours !== null && report.link_scan_age_hours !== undefined && (
            <FreshnessBadge
              label="Link scan"
              hours={report.link_scan_age_hours}
              warnAfter={24}
            />
          )}
        </div>
        {/* Task creation feedback */}
        {taskMessage && (
          <div
            className={`flex items-center gap-2 text-xs px-2 py-1.5 rounded ${
              taskMessage.type === 'success'
                ? 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-300'
                : 'bg-red-500/10 text-red-700 dark:text-red-300'
            }`}
          >
            {taskMessage.type === 'success' ? (
              <CheckCircle2 className="w-3.5 h-3.5 shrink-0" />
            ) : (
              <AlertCircle className="w-3.5 h-3.5 shrink-0" />
            )}
            {taskMessage.text}
          </div>
        )}
      </div>

      <ScrollArea className="flex-1 min-h-0">
        <div className="p-4 space-y-4">
          {/* Summary cards */}
          <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
            <SummaryCard
              title="Indexed"
              value={report.indexed_count}
              sub={`of ${report.sitemap_total} in sitemap`}
              variant="success"
            />
            <SummaryCard
              title="Not Indexed"
              value={report.not_indexed_count}
              sub="known issues"
              variant="warning"
            />
            <SummaryCard
              title="Missing from GSC"
              value={report.in_sitemap_not_in_gsc.length}
              sub="in sitemap only"
              variant="danger"
            />
            <SummaryCard
              title="Missing from Sitemap"
              value={report.in_gsc_not_in_sitemap.length}
              sub="in GSC only"
              variant="neutral"
            />
          </div>

          {/* Recovery campaign stats */}
          {recoveryStats && recoveryStats.total_attempts > 0 && (
            <RecoveryStatsCard stats={recoveryStats} />
          )}

          {/* Link-fix CTA */}
          {report && (
            <LinkFixCard
              candidates={report.resubmit_priority}
              onCreateTask={handleCreateRecoveryTask}
              creating={creatingTask}
            />
          )}

          {/* Instructions */}
          <InstructionsCard />

          <Tabs defaultValue="priority" className="w-full">
            <TabsList className="bg-card border border-border">
              <TabsTrigger value="priority" className="text-xs">
                Priority Resubmit ({report.resubmit_priority.length})
              </TabsTrigger>
              <TabsTrigger value="not_indexed" className="text-xs">
                Not Indexed ({report.not_indexed.length})
              </TabsTrigger>
              <TabsTrigger value="missing_gsc" className="text-xs">
                Missing from GSC ({report.in_sitemap_not_in_gsc.length})
              </TabsTrigger>
              <TabsTrigger value="missing_sitemap" className="text-xs">
                Missing from Sitemap ({report.in_gsc_not_in_sitemap.length})
              </TabsTrigger>
            </TabsList>

            <TabsContent value="priority" className="mt-3 h-[calc(100vh-380px)] min-h-0">
              {report.resubmit_priority.length === 0 ? (
                <EmptyState message="No resubmission candidates found." />
              ) : (
                <CandidateTable candidates={report.resubmit_priority} projectId={projectId} />
              )}
            </TabsContent>

            <TabsContent value="not_indexed" className="mt-3 h-[calc(100vh-380px)] min-h-0">
              {report.not_indexed.length === 0 ? (
                <EmptyState message="All indexed URLs are healthy." />
              ) : (
                <DriftUrlTable urls={report.not_indexed} projectId={projectId} />
              )}
            </TabsContent>

            <TabsContent value="missing_gsc" className="mt-3 h-[calc(100vh-380px)] min-h-0">
              {report.in_sitemap_not_in_gsc.length === 0 ? (
                <EmptyState message="All sitemap URLs are known to GSC." />
              ) : (
                <div className="space-y-3">
                  <div className="text-xs text-muted-foreground bg-muted/50 rounded px-3 py-2">
                    <Info className="w-3.5 h-3.5 inline-block mr-1 -mt-0.5" />
                    These URLs are in your sitemap but were not found in the cached GSC data.
                    {report.gsc_data_age_hours !== null && report.gsc_data_age_hours !== undefined && report.gsc_data_age_hours > 24
                      ? ` GSC data is ${report.gsc_data_age_hours}h old — run a fresh GSC collection to update.`
                      : ' They may be recently discovered by Google but not yet indexed, or the URL format may differ from what GSC reports.'}
                  </div>
                  <DriftUrlTable urls={report.in_sitemap_not_in_gsc} projectId={projectId} />
                </div>
              )}
            </TabsContent>

            <TabsContent value="missing_sitemap" className="mt-3 h-[calc(100vh-380px)] min-h-0">
              {report.in_gsc_not_in_sitemap.length === 0 ? (
                <EmptyState message="No GSC URLs are missing from the sitemap." />
              ) : (
                <DriftUrlTable urls={report.in_gsc_not_in_sitemap} projectId={projectId} />
              )}
            </TabsContent>
          </Tabs>
        </div>
      </ScrollArea>
    </div>
  )
}

// ─── Sub-components ───────────────────────────────────────────────────────────

function FreshnessBadge({
  label,
  hours,
  warnAfter,
}: {
  label: string
  hours: number
  warnAfter: number
}) {
  const isStale = hours > warnAfter
  return (
    <span
      className={`inline-flex items-center gap-1 px-1.5 py-0.5 rounded ${
        isStale
          ? 'bg-amber-500/10 text-amber-700 dark:text-amber-300'
          : 'bg-emerald-500/10 text-emerald-700 dark:text-emerald-300'
      }`}
    >
      <span className="font-medium">{label}:</span>
      {hours < 1 ? '<1h old' : `${hours}h old`}
    </span>
  )
}

function SummaryCard({
  title,
  value,
  sub,
  variant,
}: {
  title: string
  value: number
  sub: string
  variant: 'success' | 'warning' | 'danger' | 'neutral'
}) {
  const variantStyles = {
    success: 'border-emerald-500/30 bg-emerald-500/5',
    warning: 'border-amber-500/30 bg-amber-500/5',
    danger: 'border-red-500/30 bg-red-500/5',
    neutral: 'border-border bg-card',
  }

  return (
    <Card className={`${variantStyles[variant]} border`}>
      <CardContent className="p-3">
        <div className="text-2xl font-semibold tabular-nums">{value}</div>
        <div className="text-xs font-medium mt-0.5">{title}</div>
        <div className="text-[11px] text-muted-foreground">{sub}</div>
      </CardContent>
    </Card>
  )
}

function LinkFixCard({
  candidates,
  onCreateTask,
  creating,
}: {
  candidates: ResubmitCandidate[]
  onCreateTask: () => void
  creating: boolean
}) {
  const orphans = candidates.filter((c) => !c.has_internal_links)
  const blocked = orphans.filter((c) =>
    c.recovery_status === 'linked' || c.recovery_status === 'pending' || c.recovery_status === 'resolved'
  )
  const eligible = orphans.filter((c) =>
    !c.recovery_status || c.recovery_status === 'failed'
  )

  if (orphans.length === 0) return null

  return (
    <Card className="border-amber-500/20 bg-amber-500/5">
      <CardContent className="p-3">
        <div className="flex items-start gap-3">
          <Link2 className="w-4 h-4 text-amber-600 mt-0.5 shrink-0" />
          <div className="flex-1 min-w-0">
            <div className="flex items-center justify-between gap-3">
              <div className="text-xs text-amber-900 dark:text-amber-200">
                {eligible.length > 0 ? (
                  <>
                    <span className="font-medium">{eligible.length} URL{eligible.length === 1 ? '' : 's'}</span> in the priority list have{' '}
                    <span className="font-medium">zero internal incoming links</span> and are eligible for recovery.
                  </>
                ) : blocked.length > 0 ? (
                  <>
                    <span className="font-medium">{blocked.length} URL{blocked.length === 1 ? '' : 's'}</span> have zero incoming links but{' '}
                    <span className="font-medium">were already processed</span> by a previous recovery run.
                  </>
                ) : (
                  <>All URLs in the priority list have internal links.</>
                )}
              </div>
              <Button
                variant="outline"
                size="sm"
                className="shrink-0 text-xs h-7 border-amber-500/30 hover:bg-amber-500/10"
                onClick={onCreateTask}
                disabled={creating || eligible.length === 0}
              >
                {creating ? 'Starting…' : eligible.length === 0 ? 'Nothing to recover' : 'Start link recovery'}
              </Button>
            </div>
            {eligible.length === 0 && blocked.length > 0 && (
              <p className="text-[11px] text-amber-700 dark:text-amber-400 mt-1">
                These URLs were already targeted by a previous recovery campaign. Recovery avoids duplicates to prevent link spam. If you want to retry, clear the recovery history from the database.
              </p>
            )}
            {eligible.length > 0 && (
              <p className="text-[11px] text-amber-700 dark:text-amber-400 mt-1">
                This starts a <strong>GSC indexing recovery</strong> campaign that refreshes data, plans targets, and spawns one focused link-fix task per eligible URL.
              </p>
            )}
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

function InstructionsCard() {
  return (
    <Card className="border-blue-500/20 bg-blue-500/5">
      <CardContent className="p-3">
        <div className="flex items-start gap-2">
          <Info className="w-4 h-4 text-blue-600 mt-0.5 shrink-0" />
          <div className="text-xs text-blue-900 dark:text-blue-200 space-y-1">
            <p className="font-medium">How to manually request indexing</p>
            <ol className="list-decimal list-inside space-y-0.5 text-[11px] text-blue-800 dark:text-blue-300">
              <li>Open <a href="https://search.google.com/search-console" target="_blank" rel="noopener noreferrer" className="underline">Google Search Console</a> → URL Inspection</li>
              <li>Copy a URL from the table below (click the copy button)</li>
              <li>Paste it into the GSC search bar and wait for inspection</li>
              <li>Click <strong>Request Indexing</strong></li>
            </ol>
            <p className="text-[11px] text-blue-700 dark:text-blue-400 mt-1">
              Rate limit: ~10–20 requests per day. Prioritise the highest-scored URLs first — they are the most valuable.
            </p>
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

function EmptyState({ message }: { message: string }) {
  return (
    <div className="flex items-center justify-center py-12 text-sm text-muted-foreground">
      {message}
    </div>
  )
}

function CopyUrl({ url, projectId }: { url: string; projectId: string }) {
  const storageKey = `pageseeds:gsc_drift:copied:${projectId}`

  const isCopied = useCallback(() => {
    try {
      const raw = localStorage.getItem(storageKey)
      const set = raw ? new Set<string>(JSON.parse(raw)) : new Set<string>()
      return set.has(url)
    } catch {
      return false
    }
  }, [storageKey, url])

  const [copied, setCopied] = useState(isCopied)

  const markCopied = useCallback(() => {
    try {
      const raw = localStorage.getItem(storageKey)
      const set = raw ? new Set<string>(JSON.parse(raw)) : new Set<string>()
      set.add(url)
      localStorage.setItem(storageKey, JSON.stringify([...set]))
    } catch {
      // ignore
    }
    setCopied(true)
  }, [storageKey, url])

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(url)
      markCopied()
    } catch {
      const textarea = document.createElement('textarea')
      textarea.value = url
      document.body.appendChild(textarea)
      textarea.select()
      document.execCommand('copy')
      document.body.removeChild(textarea)
      markCopied()
    }
  }

  return (
    <button
      onClick={handleCopy}
      className={`inline-flex items-center justify-center w-5 h-5 rounded transition-colors shrink-0 ${
        copied ? 'bg-emerald-500/10' : 'hover:bg-muted'
      }`}
      title={copied ? 'Copied — already submitted?' : 'Copy full URL'}
    >
      {copied ? (
        <Check className="w-3 h-3 text-emerald-600" />
      ) : (
        <Copy className="w-3 h-3 text-muted-foreground" />
      )}
    </button>
  )
}

function CandidateTable({ candidates, projectId }: { candidates: ResubmitCandidate[]; projectId: string }) {
  return (
    <ScrollArea className="h-full">
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="text-xs w-[60px]">Score</TableHead>
              <TableHead className="text-xs">URL</TableHead>
              <TableHead className="text-xs">Reason</TableHead>
              <TableHead className="text-xs">Links</TableHead>
              <TableHead className="text-xs">Recovery</TableHead>
              <TableHead className="text-xs">Keyword</TableHead>
              <TableHead className="text-xs">Published</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {candidates.map((c, i) => (
              <TableRow key={i} className="group">
                <TableCell className="text-xs font-medium tabular-nums">
                  {c.priority_score}
                </TableCell>
                <TableCell className="text-xs">
                  <div className="flex items-center gap-1.5">
                    <CopyUrl url={c.url} projectId={projectId} />
                    <div className="min-w-0">
                      <div className="max-w-[260px] truncate" title={c.url}>
                        {c.slug}
                      </div>
                      <div className="text-[11px] text-muted-foreground truncate max-w-[260px]">
                        {c.priority_reason}
                      </div>
                    </div>
                  </div>
                </TableCell>
                <TableCell className="text-xs">
                  <ReasonBadge reason={c.reason_code} />
                </TableCell>
                <TableCell className="text-xs">
                  {c.has_internal_links ? (
                    <span className="text-emerald-600">{c.incoming_link_count}</span>
                  ) : (
                    <span className="text-red-500 font-medium">0</span>
                  )}
                </TableCell>
                <TableCell className="text-xs">
                  {c.recovery_status ? (
                    <RecoveryBadge status={c.recovery_status} />
                  ) : (
                    <span className="text-muted-foreground">—</span>
                  )}
                </TableCell>
                <TableCell className="text-xs text-muted-foreground">
                  {c.target_keyword ?? '—'}
                </TableCell>
                <TableCell className="text-xs text-muted-foreground">
                  {c.published_date ?? '—'}
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </ScrollArea>
  )
}

function DriftUrlTable({ urls, projectId }: { urls: DriftUrl[]; projectId: string }) {
  return (
    <ScrollArea className="h-full">
      <div className="rounded-md border">
        <Table>
          <TableHeader>
            <TableRow className="hover:bg-transparent">
              <TableHead className="text-xs">URL</TableHead>
              <TableHead className="text-xs w-[90px]">Last mod</TableHead>
              <TableHead className="text-xs w-[70px]">Content</TableHead>
              <TableHead className="text-xs">Issues</TableHead>
              <TableHead className="text-xs w-[90px]">Reason</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {urls.map((u, i) => (
              <TableRow key={i} className="group">
                <TableCell className="text-xs">
                  <div className="flex items-center gap-1.5">
                    <CopyUrl url={u.url} projectId={projectId} />
                    <div className="max-w-[260px] truncate" title={u.url}>
                      {u.slug}
                    </div>
                  </div>
                </TableCell>
                <TableCell className="text-xs text-muted-foreground">
                  {u.lastmod
                    ? new Date(u.lastmod).toLocaleDateString()
                    : '—'}
                </TableCell>
                <TableCell className="text-xs">
                  {u.has_content_file ? (
                    <span className="text-emerald-600">✓</span>
                  ) : (
                    <span className="text-red-500" title="No MDX file matches this URL">✗</span>
                  )}
                </TableCell>
                <TableCell className="text-xs">
                  <IssueBadges issues={u.issues ?? []} />
                </TableCell>
                <TableCell className="text-xs">
                  <ReasonBadge reason={u.reason_code ?? 'unknown'} />
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </ScrollArea>
  )
}

function IssueBadges({ issues }: { issues: string[] }) {
  if (issues.length === 0) {
    return <span className="text-muted-foreground">—</span>
  }
  return (
    <div className="flex flex-wrap gap-1">
      {issues.map((issue, i) => (
        <Badge
          key={i}
          variant="outline"
          className={`text-[10px] ${
            issue === 'noindex'
              ? 'bg-purple-500/10 text-purple-700 border-purple-500/20'
              : issue.startsWith('canonical')
                ? 'bg-orange-500/10 text-orange-700 border-orange-500/20'
                : issue.startsWith('thin')
                  ? 'bg-amber-500/10 text-amber-700 border-amber-500/20'
                  : 'bg-muted text-muted-foreground'
          }`}
        >
          {issue}
        </Badge>
      ))}
    </div>
  )
}

function RecoveryStatsCard({ stats }: { stats: RecoveryStats }) {
  return (
    <Card className="border-emerald-500/20 bg-emerald-500/5">
      <CardContent className="p-3">
        <div className="flex items-start gap-3">
          <CheckCircle2 className="w-4 h-4 text-emerald-600 mt-0.5 shrink-0" />
          <div className="flex-1 min-w-0">
            <div className="text-xs font-medium text-emerald-900 dark:text-emerald-200">
              Recovery campaign progress
            </div>
            <div className="grid grid-cols-3 gap-2 mt-2">
              <div className="text-center">
                <div className="text-lg font-semibold tabular-nums text-emerald-700">
                  {stats.linked}
                </div>
                <div className="text-[10px] text-emerald-600">Linked</div>
              </div>
              <div className="text-center">
                <div className="text-lg font-semibold tabular-nums text-emerald-700">
                  {stats.total_links_added}
                </div>
                <div className="text-[10px] text-emerald-600">Links added</div>
              </div>
              <div className="text-center">
                <div className="text-lg font-semibold tabular-nums text-emerald-700">
                  {stats.resolved}
                </div>
                <div className="text-[10px] text-emerald-600">Resolved</div>
              </div>
            </div>
            {stats.failed > 0 && (
              <div className="text-[11px] text-amber-700 dark:text-amber-400 mt-1">
                {stats.failed} attempt{stats.failed === 1 ? '' : 's'} failed or still pending
              </div>
            )}
          </div>
        </div>
      </CardContent>
    </Card>
  )
}

function ReasonBadge({ reason }: { reason: string }) {
  const variantMap: Record<string, { label: string; className: string }> = {
    indexed_pass: { label: 'Indexed', className: 'bg-emerald-500/10 text-emerald-700 hover:bg-emerald-500/20' },
    not_indexed_other: { label: 'Unknown', className: 'bg-red-500/10 text-red-700 hover:bg-red-500/20' },
    not_indexed_crawled: { label: 'Crawled', className: 'bg-amber-500/10 text-amber-700 hover:bg-amber-500/20' },
    not_indexed_discovered: { label: 'Discovered', className: 'bg-blue-500/10 text-blue-700 hover:bg-blue-500/20' },
    robots_blocked: { label: 'Robots', className: 'bg-purple-500/10 text-purple-700 hover:bg-purple-500/20' },
    noindex: { label: 'Noindex', className: 'bg-purple-500/10 text-purple-700 hover:bg-purple-500/20' },
    fetch_error: { label: 'Fetch Error', className: 'bg-orange-500/10 text-orange-700 hover:bg-orange-500/20' },
    canonical_mismatch: { label: 'Canonical', className: 'bg-orange-500/10 text-orange-700 hover:bg-orange-500/20' },
    not_in_gsc: { label: 'Not in GSC', className: 'bg-red-500/10 text-red-700 hover:bg-red-500/20' },
  }

  const mapped = variantMap[reason] ?? {
    label: reason.replace(/_/g, ' '),
    className: 'bg-muted text-muted-foreground',
  }

  return (
    <Badge variant="outline" className={`text-[10px] capitalize ${mapped.className}`}>
      {mapped.label}
    </Badge>
  )
}

function RecoveryBadge({ status }: { status: string }) {
  const variantMap: Record<string, { label: string; className: string }> = {
    linked: { label: 'Linked', className: 'bg-emerald-500/10 text-emerald-700 hover:bg-emerald-500/20' },
    pending: { label: 'Pending', className: 'bg-amber-500/10 text-amber-700 hover:bg-amber-500/20' },
    resolved: { label: 'Resolved', className: 'bg-blue-500/10 text-blue-700 hover:bg-blue-500/20' },
    failed: { label: 'Failed', className: 'bg-red-500/10 text-red-700 hover:bg-red-500/20' },
  }

  const mapped = variantMap[status] ?? {
    label: status,
    className: 'bg-muted text-muted-foreground',
  }

  return (
    <Badge variant="outline" className={`text-[10px] capitalize ${mapped.className}`}>
      {mapped.label}
    </Badge>
  )
}
