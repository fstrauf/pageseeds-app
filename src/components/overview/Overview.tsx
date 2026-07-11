import React, { useCallback, useEffect, useRef, useState } from 'react'
import {
  Zap, RefreshCw, CheckCircle2, Clock, AlertCircle,
  BarChart2, FileText, Search, Globe, BookOpen, Cpu, ChevronRight,
  PlayCircle, TrendingUp, Users, ArrowRight, Send, Target,
  Activity, Wrench, HeartPulse,
} from 'lucide-react'
import { createTask, getCtrHealthSummary, getProjectOverview, importLiveSite, listArticles, listLiveSitePages, openFeatureSpecInVSCode, repairArticlePaths, runSeoOrchestrator, updateTaskStatus } from '../../lib/tauri'
import { useQueueStore } from '@/stores/queueStore'
import type { Article, LandingPageResearchPending, PendingFeatureSpec, Project, ProjectOverview, Task, WorkflowActivity } from '../../lib/types'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { cn } from '../../lib/utils'
import { SetupWarnings } from './SetupWarnings'
import { PublishPanel } from '../articles/PublishPanel'
import { useQuery } from '../../hooks/useQuery'

// ─── Quick actions definition ─────────────────────────────────────────────────

interface ActionDef {
  task_type: string
  label: string
  description: string
  icon: React.ReactNode
  phase: string
  frequency: string
  nextView: import('../../lib/types').View
  nextLabel: string
}

const QUICK_ACTIONS: ActionDef[] = [
  // ── Research (monthly / as needed) ─────────────────────────────────────────
  {
    task_type: 'research_keywords',
    label: 'Keyword Research',
    description: 'Find new long-tail keyword opportunities for your site, then select which to write about',
    icon: <Search size={16} />,
    phase: 'research',
    frequency: 'monthly',
    nextView: 'tasks',
    nextLabel: 'Select keywords & create articles',
  },
  {
    task_type: 'research_landing_pages',
    label: 'Landing Page Research',
    description: 'Research high-intent keywords for conversion-focused landing pages with strategic context',
    icon: <Target size={16} />,
    phase: 'research',
    frequency: 'monthly',
    nextView: 'tasks',
    nextLabel: 'Select keywords & create landing pages',
  },
  {
    task_type: 'reddit_opportunity_search',
    label: 'Reddit Search',
    description: 'Search subreddits for posts to engage with and save pending opportunities',
    icon: <Users size={16} />,
    phase: 'research',
    frequency: 'weekly',
    nextView: 'reddit',
    nextLabel: 'Review pending opportunities',
  },
  // ── Investigation (bi-weekly / after collection) ───────────────────────────
  {
    task_type: 'content_review',
    label: 'Content Review',
    description: 'Sync GSC data and generate recommendations for the highest-priority article',
    icon: <TrendingUp size={16} />,
    phase: 'investigation',
    frequency: 'bi-weekly',
    nextView: 'tasks',
    nextLabel: 'See optimization tasks',
  },
  {
    task_type: 'cannibalization_audit',
    label: 'Cannibalization Audit',
    description: 'Detect overlapping content, find merge candidates, and identify hub gaps',
    icon: <Target size={16} />,
    phase: 'investigation',
    frequency: 'monthly',
    nextView: 'tasks',
    nextLabel: 'See merge & hub tasks',
  },
  {
    task_type: 'ctr_audit',
    label: 'CTR Audit',
    description: 'Analyze titles, meta descriptions, and snippet readiness to fix low CTR',
    icon: <BarChart2 size={16} />,
    phase: 'investigation',
    frequency: 'monthly',
    nextView: 'tasks',
    nextLabel: 'See CTR fix tasks',
  },
  {
    task_type: 'indexing_health_campaign',
    label: 'Indexing Health Campaign',
    description: 'Unified workflow: checks prerequisites, reviews distinctiveness against cluster siblings, and spawns targeted fixes for non-indexed pages',
    icon: <HeartPulse size={16} />,
    phase: 'investigation',
    frequency: 'bi-weekly',
    nextView: 'tasks',
    nextLabel: 'See campaign results',
  },
  {
    task_type: 'seo_health_scan',
    label: 'SEO Health Scan',
    description: 'Fuses content audit, CTR, indexing, cannibalization, and Clarity signals into a ranked opportunity backlog',
    icon: <Globe size={16} />,
    phase: 'investigation',
    frequency: 'monthly',
    nextView: 'tasks',
    nextLabel: 'Review opportunities',
  },
  {
    task_type: 'clarity_analytics',
    label: 'Clarity Analytics',
    description: 'Collect Microsoft Clarity behavioral data, score pages for UX anomalies, and surface ranked findings',
    icon: <Activity size={16} />,
    phase: 'investigation',
    frequency: 'weekly',
    nextView: 'tasks',
    nextLabel: 'Review findings',
  },
  {
    task_type: 'generate_feature_spec',
    label: 'Generate Feature Spec',
    description: 'Agentic investigation of the project to produce a prioritized developer feature specification',
    icon: <BookOpen size={16} />,
    phase: 'investigation',
    frequency: 'as needed',
    nextView: 'overview',
    nextLabel: 'Review generated spec',
  },
  // ── Implementation (as needed) ─────────────────────────────────────────────
  {
    task_type: 'content_cleanup',
    label: 'Content Cleanup',
    description: 'Scan MDX files for structural issues — heading duplicates, broken frontmatter',
    icon: <FileText size={16} />,
    phase: 'implementation',
    frequency: 'as needed',
    nextView: 'tasks',
    nextLabel: 'See cleanup tasks',
  },
  {
    task_type: 'sanitize_content',
    label: 'Sanitize Content',
    description: 'Normalize frontmatter field names (metaDescription → description) across all MDX files',
    icon: <Wrench size={16} />,
    phase: 'implementation',
    frequency: 'as needed',
    nextView: 'tasks',
    nextLabel: 'See sanitize results',
  },
]

// ─── Helpers ─────────────────────────────────────────────────────────────────

function timeAgo(iso: string): string {
  const diffMs = Date.now() - new Date(iso).getTime()
  const secs = Math.floor(diffMs / 1000)
  if (secs < 60) return `${secs}s ago`
  const mins = Math.floor(secs / 60)
  if (mins < 60) return `${mins}m ago`
  const hrs = Math.floor(mins / 60)
  if (hrs < 24) return `${hrs}h ago`
  const days = Math.floor(hrs / 24)
  return `${days}d ago`
}

function relativeDate(iso: string): string {
  const diffMs = Date.now() - new Date(iso).getTime()
  if (diffMs < 0) return 'just now'
  const days = Math.floor(diffMs / (1000 * 60 * 60 * 24))
  if (days === 0) return 'today'
  if (days === 1) return '1d ago'
  return `${days}d ago`
}

const STATUS_COLORS: Record<string, string> = {
  todo: 'text-muted-foreground',
  in_progress: 'text-blue-600',
  review: 'text-amber-600',
  done: 'text-emerald-600',
  failed: 'text-destructive',
}

const STATUS_ICONS: Record<string, React.ReactNode> = {
  todo: <Clock size={13} />,
  in_progress: <RefreshCw size={13} className="animate-spin" />,
  review: <AlertCircle size={13} />,
  done: <CheckCircle2 size={13} />,
  failed: <AlertCircle size={13} />,
}

const PHASE_BADGE: Record<string, string> = {
  collection: 'bg-blue-100 text-blue-700',
  investigation: 'bg-violet-100 text-violet-700',
  research: 'bg-amber-100 text-amber-700',
  implementation: 'bg-emerald-100 text-emerald-700',
  verification: 'bg-pink-100 text-pink-700',
}

const FREQUENCY_BADGE: Record<string, string> = {
  weekly: 'bg-sky-100 text-sky-700',
  'bi-weekly': 'bg-indigo-100 text-indigo-700',
  monthly: 'bg-slate-100 text-slate-700',
  'as needed': 'bg-stone-100 text-stone-600',
}

const PHASE_ORDER = ['collection', 'research', 'investigation', 'implementation']

function getActionStatus(
  action: ActionDef,
  activityItems: WorkflowActivity[],
): { lastRun: string | null; overdue: boolean; dueText: string } {
  const item = activityItems.find(i => i.task_type === action.task_type)
  const lastRun = item?.last_run_at ?? null

  if (!lastRun) {
    return { lastRun: null, overdue: true, dueText: 'never run' }
  }

  const lastRunMs = new Date(lastRun).getTime()
  const nowMs = Date.now()
  const hoursSince = (nowMs - lastRunMs) / (1000 * 60 * 60)

  const intervalHours: Record<string, number> = {
    weekly: 24 * 7,
    'bi-weekly': 24 * 14,
    monthly: 24 * 30,
  }

  const threshold = intervalHours[action.frequency] ?? Infinity
  const overdue = hoursSince > threshold

  let dueText: string
  if (overdue) {
    const daysOver = Math.floor((hoursSince - threshold) / 24)
    dueText = daysOver <= 0 ? 'due now' : `${daysOver}d overdue`
  } else {
    const daysUntil = Math.ceil((threshold - hoursSince) / 24)
    dueText = daysUntil <= 0 ? 'due soon' : `in ${daysUntil}d`
  }

  return { lastRun, overdue, dueText }
}

// ─── Pending Landing Page Research Card ───────────────────────────────────────

function PendingLandingPageCard({ 
  items, 
  onViewTask 
}: { 
  items: LandingPageResearchPending[]
  onViewTask: (taskId: string) => void 
}) {
  if (!items || items.length === 0) return null
  
  return (
    <Card className="bg-card border-amber-200">
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-semibold text-foreground flex items-center gap-1.5">
          <Target size={13} className="text-amber-600" />
          Landing Page Research Awaiting Review
        </CardTitle>
      </CardHeader>
      <CardContent className="pb-3">
        <div className="space-y-3">
          {items.map(item => (
            <div key={item.id} className="space-y-2">
              <button
                onClick={() => onViewTask(item.id)}
                className="w-full text-left group"
              >
                <div className="flex items-center gap-2">
                  <span className="text-xs font-medium text-foreground group-hover:text-amber-600 transition-colors">
                    {item.title || 'Landing Page Research'}
                  </span>
                  <span className="text-[10px] px-1.5 py-0.5 rounded bg-amber-100 text-amber-700">
                    review
                  </span>
                  <ArrowRight size={11} className="ml-auto text-muted-foreground opacity-0 group-hover:opacity-100 transition-opacity" />
                </div>
              </button>
              
              {item.context && (
                <p className="text-xs text-muted-foreground pl-2 border-l-2 border-amber-200">
                  {item.context.length > 120 
                    ? item.context.slice(0, 120) + '...' 
                    : item.context}
                </p>
              )}
              
              {item.themes.length > 0 && (
                <div className="flex flex-wrap gap-1 pl-2">
                  {item.themes.slice(0, 4).map((theme, idx) => (
                    <span 
                      key={idx}
                      className="text-[10px] px-1.5 py-0.5 rounded bg-secondary text-muted-foreground"
                    >
                      {theme}
                    </span>
                  ))}
                  {item.themes.length > 4 && (
                    <span className="text-[10px] px-1.5 py-0.5 rounded bg-secondary text-muted-foreground">
                      +{item.themes.length - 4} more
                    </span>
                  )}
                </div>
              )}
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  )
}

// ─── Pending Feature Spec Card ────────────────────────────────────────────────

function PendingFeatureSpecCard({
  items,
  onMarkDone,
}: {
  items: PendingFeatureSpec[]
  onMarkDone: (taskId: string) => void
}) {
  if (!items || items.length === 0) return null

  return (
    <Card className="bg-card border-amber-200">
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-semibold text-foreground flex items-center gap-1.5">
          <FileText size={13} className="text-amber-600" />
          Feature Specs Awaiting Implementation
        </CardTitle>
      </CardHeader>
      <CardContent className="pb-3 space-y-2">
        <p className="text-xs text-muted-foreground">
          Implement these in the target repo, then mark as done.
        </p>
        {items.map(item => (
          <div key={item.id} className="flex items-center gap-2">
            <div className="flex-1 min-w-0">
              <p className="text-xs font-medium text-foreground truncate">
                {item.title || 'Feature Spec'}
              </p>
              <p className="text-[10px] text-muted-foreground">
                {relativeDate(item.updated_at)}
              </p>
            </div>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 text-[11px] px-2 py-0 text-muted-foreground hover:text-foreground"
              onClick={async () => {
                try {
                  await openFeatureSpecInVSCode(item.id)
                } catch {
                  // ignore — user will see if VS Code doesn't open
                }
              }}
            >
              Open
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="h-7 text-[11px] px-2 py-0"
              onClick={() => onMarkDone(item.id)}
            >
              <CheckCircle2 size={11} className="mr-1" />
              Done
            </Button>
          </div>
        ))}
      </CardContent>
    </Card>
  )
}

// ─── Main component ───────────────────────────────────────────────────────────

interface OverviewProps {
  project: Project | null
  onViewChange: (view: import('../../lib/types').View, taskId?: string) => void
  onRunTasks?: (tasks: Task[]) => void
  runCompletedTick?: number
  onEditProject?: () => void
}

export function Overview({
  project,
  onViewChange,
  onRunTasks,
  runCompletedTick = 0,
  onEditProject,
}: OverviewProps) {
  const isLiveSiteProject = project?.project_mode === 'live_site'
  const [overview, setOverview] = useState<ProjectOverview | null>(null)
  const [loading, setLoading] = useState(false)
  const [runningActionLabel, setRunningActionLabel] = useState<string | null>(null)
  const [quickActionError, setQuickActionError] = useState<string | null>(null)
  const pollRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const [publishOpen, setPublishOpen] = useState(false)
  const [publishCandidates, setPublishCandidates] = useState<Article[]>([])
  const [loadingPublish, setLoadingPublish] = useState(false)
  const [liveSiteImporting, setLiveSiteImporting] = useState(false)
  const [liveSiteMsg, setLiveSiteMsg] = useState<string | null>(null)
  
  // Landing page research dialog state
  const [lpDialogOpen, setLpDialogOpen] = useState(false)
  const [lpContext, setLpContext] = useState('')
  const [lpThemes, setLpThemes] = useState('')
  const [lpCreating, setLpCreating] = useState(false)
  const [repairingPaths, setRepairingPaths] = useState(false)
  const [repairResult, setRepairResult] = useState<import('../../lib/types').RepairPathResult | null>(null)
  const [runningCtr, setRunningCtr] = useState(false)
  const [runningOrchestrator, setRunningOrchestrator] = useState(false)
  const [orchestratorMsg, setOrchestratorMsg] = useState<string | null>(null)

  const {
    data: liveSitePages = [],
    isLoading: loadingLiveSitePages,
    refetch: refetchLiveSitePages,
  } = useQuery(
    `overview-live-site-pages-${project?.id ?? 'none'}`,
    () => listLiveSitePages(project?.id ?? ''),
    { enabled: !!project?.id && isLiveSiteProject, staleTime: 0 },
  )

  const {
    data: ctrHealth,
    isLoading: loadingCtrHealth,
    refetch: refetchCtrHealth,
  } = useQuery(
    `overview-ctr-health-${project?.id ?? 'none'}`,
    () => getCtrHealthSummary(project?.id ?? ''),
    { enabled: !!project?.id && !isLiveSiteProject, staleTime: 30_000 },
  )

  const load = useCallback(async () => {
    if (!project) return
    setLoading(true)
    try {
      const data = await getProjectOverview(project.id)
      console.log('[Overview] load() received:', {
        tasks: data.tasks,
        articles: data.articles,
        ready: data.ready_task_count,
        health: data.health_snapshot,
        fixSummary: data.fix_summary,
      })
      setOverview(data)
    } catch {
      // ignore
    } finally {
      setLoading(false)
    }
  }, [project])

  useEffect(() => {
    load()
    const pollTimer = pollRef.current
    return () => {
      if (pollTimer) clearTimeout(pollTimer)
    }
  }, [load])

  useEffect(() => {
    if (!project || runCompletedTick === 0) return
    load()
    if (!isLiveSiteProject) {
      refetchCtrHealth()
    } else {
      refetchLiveSitePages()
    }
  }, [project, runCompletedTick, load, isLiveSiteProject, refetchCtrHealth, refetchLiveSitePages])

  async function handleOpenPublish() {
    if (!project || loadingPublish) return
    setLoadingPublish(true)
    try {
      const all = await listArticles(project.id)
      setPublishCandidates(all.filter(a => a.status === 'ready_to_publish' || a.status === 'draft'))
      setPublishOpen(true)
    } catch (e: unknown) {
      setQuickActionError(String(e))
    } finally {
      setLoadingPublish(false)
    }
  }

  async function handleQuickAction(action: ActionDef) {
    if (!project || runningActionLabel !== null) return
    
    // Landing page research needs a dialog for context input
    if (action.task_type === 'research_landing_pages') {
      setLpDialogOpen(true)
      return
    }
    
    // Feature spec requires a live site URL to crawl
    if (action.task_type === 'generate_feature_spec') {
      const hasUrl = project.site_url || project.sitemap_url
      if (!hasUrl) {
        setQuickActionError(
          'Feature Spec requires a Site URL or Sitemap URL. \
          Set it in Project Settings first.'
        )
        return
      }
    }

    // Clarity analytics requires a Clarity project ID
    if (action.task_type === 'clarity_analytics') {
      const clarityId = project.clarity_project_id?.trim()
      if (!clarityId) {
        setQuickActionError(
          'Clarity Analytics requires a Clarity Project ID. Set it in Project Settings or Clarity → Settings first.'
        )
        return
      }
    }
    
    setRunningActionLabel(action.label)
    setQuickActionError(null)
    try {
      const task = await createTask(
        project.id,
        action.task_type,
        `${action.label} — ${new Date().toLocaleDateString()}`,
        undefined,
        'medium',
      )
      onRunTasks?.([task])
      await load()
    } catch (e: unknown) {
      setQuickActionError(String(e))
    } finally {
      setRunningActionLabel(null)
    }
  }

  async function handleImportLiveSite() {
    if (!project || !isLiveSiteProject || liveSiteImporting) return

    setLiveSiteImporting(true)
    setQuickActionError(null)
    setLiveSiteMsg(null)
    try {
      const result = await importLiveSite(project.id, 50)
      setLiveSiteMsg(
        `Imported ${result.pages_imported} page${result.pages_imported !== 1 ? 's' : ''} from ${result.discovered_urls} sitemap URL${result.discovered_urls !== 1 ? 's' : ''}${result.pages_failed > 0 ? `, with ${result.pages_failed} crawl failure${result.pages_failed !== 1 ? 's' : ''}` : ''}.`,
      )
      await refetchLiveSitePages()
      await load()
    } catch (e: unknown) {
      setQuickActionError(String(e))
    } finally {
      setLiveSiteImporting(false)
    }
  }

  async function handleRunSeoOrchestrator() {
    if (!project || runningOrchestrator) return

    setRunningOrchestrator(true)
    setQuickActionError(null)
    setOrchestratorMsg(null)
    try {
      const result = await runSeoOrchestrator(project.id)
      const taskCount = (result.findings as unknown[])?.length ?? 0
      setOrchestratorMsg(
        `Orchestrator launched ${taskCount} task${taskCount !== 1 ? 's' : ''}. ${result.summary}`,
      )
      await load()
    } catch (e: unknown) {
      setQuickActionError(String(e))
    } finally {
      setRunningOrchestrator(false)
    }
  }

  if (!project) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        Select a project to see the overview.
      </div>
    )
  }

  const tasks = overview?.tasks
  const articles = overview?.articles
  const pct = tasks && tasks.total > 0 ? Math.round((tasks.done / tasks.total) * 100) : 0
  const liveSitePageCount = liveSitePages.length

  return (
    <>
    <div className="flex flex-col h-full overflow-y-auto bg-background">
      <div className="p-6 space-y-6 pb-20">

        {/* Header */}
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-base font-semibold text-foreground">{project.name}</h1>
            <div className="mt-1 flex items-center gap-2">
              <Badge variant={isLiveSiteProject ? 'secondary' : 'outline'} className="text-[10px] uppercase tracking-wide">
                {isLiveSiteProject ? 'Live Site' : 'Workspace'}
              </Badge>
            </div>
            {project.site_url && (
              <div className="text-xs text-muted-foreground mt-0.5">{project.site_url}</div>
            )}
          </div>
          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={handleRunSeoOrchestrator}
              disabled={runningOrchestrator}
              className="text-xs"
            >
              {runningOrchestrator ? (
                <RefreshCw size={13} className="mr-1.5 animate-spin" />
              ) : (
                <Cpu size={13} className="mr-1.5" />
              )}
              Run SEO Orchestrator
            </Button>
            <Button variant="ghost" size="icon-sm" onClick={load} disabled={loading} className="text-muted-foreground">
              <RefreshCw size={13} className={loading ? 'animate-spin' : ''} />
            </Button>
          </div>
        </div>

        {orchestratorMsg && (
          <div className="flex items-start gap-3 rounded-md border px-3 py-2.5 text-sm bg-emerald-50 border-emerald-200 text-emerald-900 dark:bg-emerald-950/30 dark:border-emerald-800 dark:text-emerald-100">
            <CheckCircle2 size={16} className="mt-0.5 shrink-0" />
            <div className="flex-1 min-w-0">{orchestratorMsg}</div>
          </div>
        )}

        {!isLiveSiteProject && (
          <SetupWarnings projectId={project.id} onViewChange={onViewChange} />
        )}

        {/* Site URL required for live crawling features */}
        {!project.site_url && !project.sitemap_url && (
          <div className="flex items-start gap-3 rounded-md border px-3 py-2.5 text-sm bg-amber-50 border-amber-200 text-amber-900 dark:bg-amber-950/30 dark:border-amber-800 dark:text-amber-100">
            <span className="mt-0.5 shrink-0 font-semibold">⚠</span>
            <div className="flex-1 min-w-0">
              <span className="font-medium">Site URL not configured</span>
              <span className="ml-2 opacity-75">
                Features like Feature Spec and GSC require a public URL to crawl.
              </span>
            </div>
            {onEditProject && (
              <button
                onClick={onEditProject}
                className="shrink-0 text-xs font-medium underline opacity-80 hover:opacity-100"
              >
                Edit Project
              </button>
            )}
          </div>
        )}

        {/* Stat cards row */}
        <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
          {/* Tasks total */}
          <Card className="bg-card border-border">
            <CardContent className="pt-4 pb-3 px-4">
              <div className="text-xs text-muted-foreground mb-1">Tasks</div>
              <div className="text-2xl font-bold text-foreground">{tasks?.total ?? '—'}</div>
              <div className="text-xs text-muted-foreground mt-1">
                {tasks ? `${tasks.done} done · ${tasks.todo} todo` : ''}
              </div>
            </CardContent>
          </Card>

          {/* Progress */}
          <Card className="bg-card border-border">
            <CardContent className="pt-4 pb-3 px-4">
              <div className="text-xs text-muted-foreground mb-1">Progress</div>
              <div className="text-2xl font-bold text-foreground">{pct}%</div>
              <div className="w-full h-1 bg-secondary rounded-full mt-2 overflow-hidden">
                <div
                  className="h-full bg-primary rounded-full transition-all"
                  style={{ width: `${pct}%` }}
                />
              </div>
            </CardContent>
          </Card>

          {/* Ready tasks */}
          <Card className="bg-card border-border">
            <CardContent className="pt-4 pb-3 px-4">
              <div className="text-xs text-muted-foreground mb-1">Ready</div>
              <div className="text-2xl font-bold text-foreground">{overview?.ready_task_count ?? '—'}</div>
              <div className="text-xs text-muted-foreground mt-1">tasks ready to run</div>
            </CardContent>
          </Card>

          {/* Articles */}
          <Card className="bg-card border-border">
            <CardContent className="pt-4 pb-3 px-4">
              <div className="text-xs text-muted-foreground mb-1">{isLiveSiteProject ? 'Pages' : 'Articles'}</div>
              <div className="text-2xl font-bold text-foreground">{isLiveSiteProject ? liveSitePageCount : articles?.total ?? '—'}</div>
              <div className="text-xs text-muted-foreground mt-1">
                {isLiveSiteProject
                  ? (loadingLiveSitePages
                    ? 'Loading page inventory…'
                    : liveSitePageCount > 0
                      ? 'Imported from live sitemap'
                      : 'No pages imported yet')
                  : (articles ? `${articles.published} published · ${articles.draft} draft` : '')}
              </div>
            </CardContent>
          </Card>
        </div>

        {/* Last published & in-progress callouts */}
        {(articles?.last_published_date || (tasks?.in_progress ?? 0) > 0) && (
          <div className="flex flex-wrap gap-2">
            {articles?.last_published_date && (
              <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-md border border-border bg-card text-xs text-foreground">
                <FileText size={12} className="text-muted-foreground" />
                Last published: {articles.last_published_date}
              </div>
            )}
            {(tasks?.in_progress ?? 0) > 0 && (
              <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-md border border-blue-200 bg-blue-50 text-xs text-blue-700">
                <RefreshCw size={12} className="animate-spin" />
                {tasks!.in_progress} task{tasks!.in_progress !== 1 ? 's' : ''} in progress
              </div>
            )}
            {(tasks?.review ?? 0) > 0 && (
              <div className="flex items-center gap-1.5 px-3 py-1.5 rounded-md border border-yellow-200 bg-yellow-50 text-xs text-yellow-700">
                <AlertCircle size={12} />
                {tasks!.review} awaiting review
              </div>
            )}
          </div>
        )}

        <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">

          {/* Quick Actions */}
          <Card className="bg-card border-border">
            <CardHeader className="pb-3">
              <CardTitle className="text-sm font-semibold text-foreground flex items-center gap-1.5">
                <Zap size={13} className="text-amber-600" />
                Run Workflow
              </CardTitle>
            </CardHeader>
            <CardContent className="space-y-1 pb-4">
              {/* Health snapshot — shows what's still outstanding */}
              {(() => {
                const snap = overview?.health_snapshot
                const hasSnap = snap && (
                  snap.content_poor > 0 ||
                  snap.content_needs_improvement > 0 ||
                  snap.indexing_not_indexed > 0 ||
                  snap.ctr_issue_count > 0 ||
                  snap.cannibalization_clusters > 0
                )
                // Use net outstanding counts for content (after fixes applied since last audit)
                const netPoor = snap ? (snap.content_poor_outstanding > 0 ? snap.content_poor_outstanding : snap.content_poor) : 0
                const netNeedsWork = snap ? (snap.content_needs_work_outstanding > 0 ? snap.content_needs_work_outstanding : snap.content_needs_improvement) : 0
                const totalOutstanding = snap
                  ? netPoor + netNeedsWork + snap.indexing_not_indexed + snap.ctr_issue_count + snap.cannibalization_clusters
                  : 0
                const daysSince = snap && snap.last_audit_days >= 0 ? snap.last_audit_days : null
                const auditOverdue = daysSince !== null && daysSince > 14
                const hasEverRun = daysSince !== null

                return (
                  <div className="px-3 py-2 rounded-md bg-secondary/40 space-y-1.5">
                    {/* Runs-estimate indicator */}
                    {snap && hasEverRun && (() => {
                      const perRun = (snap.content_next_run_yield + snap.indexing_next_run_yield) || 20
                      const total = (snap.content_poor || 0) + (snap.content_needs_improvement || 0) + (snap.indexing_not_indexed || 0)
                      const runsNeeded = total > 0 && perRun > 0 ? Math.ceil(total / perRun) : 0
                      if (runsNeeded === 0) return null
                      return (
                        <div className="flex items-center gap-1.5">
                          <span className="text-[10px] text-muted-foreground">
                            ~{runsNeeded} more run{runsNeeded !== 1 ? 's' : ''}
                          </span>
                          <span className="text-[10px] text-muted-foreground">·</span>
                          <span className="text-[10px] text-muted-foreground">
                            ~{perRun} fix{perRun !== 1 ? 'es' : ''}/run
                          </span>
                        </div>
                      )
                    })()}

                    {/* Primary status: what's outstanding */}
                    <div className="flex items-center gap-2 flex-wrap">
                      <span className="text-[10px] text-muted-foreground uppercase tracking-wider font-medium">Outstanding</span>
                      {!hasEverRun ? (
                        <span className="text-[10px] text-amber-600 font-medium">Never audited</span>
                      ) : auditOverdue ? (
                        <span className="text-[10px] text-amber-600 font-medium">{daysSince}d since last audit</span>
                      ) : hasSnap ? (
                        <span className="text-[10px] text-amber-600 font-medium">{totalOutstanding} issue{totalOutstanding !== 1 ? 's' : ''} need attention</span>
                      ) : (
                        <span className="text-[10px] text-emerald-600 font-medium">All clear</span>
                      )}
                    </div>

                    {/* Breakdown badges: what exactly needs work */}
                    {hasEverRun && (
                      <div className="flex flex-wrap gap-1">
                        {daysSince !== null && (
                          <Badge variant="outline" className={cn(
                            'text-[10px] px-1.5 py-0.5 h-auto font-normal',
                            auditOverdue
                              ? 'bg-amber-50 text-amber-700 border-amber-200'
                              : 'bg-emerald-50 text-emerald-700 border-emerald-200'
                          )}>
                            {daysSince}d ago
                          </Badge>
                        )}
                        {snap && snap.content_poor > 0 && (
                          <Badge variant="outline" className="text-[10px] px-1.5 py-0.5 h-auto font-normal bg-rose-50 text-rose-700 border-rose-200">
                            {netPoor} poor
                            {snap.content_poor_outstanding > 0 && snap.content_poor_outstanding < snap.content_poor && (
                              <span className="text-rose-400">/{snap.content_poor}</span>
                            )}
                          </Badge>
                        )}
                        {snap && snap.content_needs_improvement > 0 && (
                          <Badge variant="outline" className="text-[10px] px-1.5 py-0.5 h-auto font-normal bg-amber-50 text-amber-700 border-amber-200">
                            {netNeedsWork} needs work
                            {snap.content_needs_work_outstanding > 0 && snap.content_needs_work_outstanding < snap.content_needs_improvement && (
                              <span className="text-amber-400">/{snap.content_needs_improvement}</span>
                            )}
                          </Badge>
                        )}
                        {snap && snap.indexing_not_indexed > 0 && (
                          <Badge variant="outline" className="text-[10px] px-1.5 py-0.5 h-auto font-normal bg-slate-50 text-slate-700 border-slate-200">
                            {snap.indexing_not_indexed} not indexed
                          </Badge>
                        )}
                        {snap && snap.ctr_issue_count > 0 && (
                          <Badge variant="outline" className="text-[10px] px-1.5 py-0.5 h-auto font-normal bg-violet-50 text-violet-700 border-violet-200">
                            {snap.ctr_issue_count} CTR issues
                          </Badge>
                        )}
                        {snap && snap.cannibalization_clusters > 0 && (
                          <Badge variant="outline" className="text-[10px] px-1.5 py-0.5 h-auto font-normal bg-pink-50 text-pink-700 border-pink-200">
                            {snap.cannibalization_clusters} clusters
                          </Badge>
                        )}
                        {!hasSnap && (
                          <Badge variant="outline" className="text-[10px] px-1.5 py-0.5 h-auto font-normal bg-emerald-50 text-emerald-700 border-emerald-200">
                            Nothing outstanding
                          </Badge>
                        )}
                      </div>
                    )}

                    {/* Next-run yield estimate */}
                    {hasEverRun && snap && (snap.content_next_run_yield > 0 || snap.indexing_next_run_yield > 0 || snap.fix_on_cooldown > 0) && (
                      <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 pt-0.5">
                        <span className="text-[10px] text-muted-foreground">Next run:</span>
                        {snap.content_next_run_yield > 0 || snap.indexing_next_run_yield > 0 ? (
                          <span className="text-[10px] text-foreground font-medium">
                            ~{snap.content_next_run_yield + snap.indexing_next_run_yield} new fixes
                            {(snap.content_next_run_yield > 0 || snap.indexing_next_run_yield > 0) && (
                              <span className="text-[10px] text-muted-foreground font-normal">
                                {' '}({[
                                  snap.content_next_run_yield > 0 && `content: ${snap.content_next_run_yield}`,
                                  snap.indexing_next_run_yield > 0 && `indexing: ${snap.indexing_next_run_yield}`,
                                ].filter(Boolean).join(' · ')})
                              </span>
                            )}
                          </span>
                        ) : (
                          <span className="text-[10px] text-muted-foreground">0 new fixes</span>
                        )}
                        {snap.fix_on_cooldown > 0 && (
                          <span className="text-[10px] text-muted-foreground">
                            · {snap.fix_on_cooldown} on 30-day cooldown
                          </span>
                        )}
                      </div>
                    )}

                    {/* Fix progress: secondary, shown only when there's fix activity */}
                    {snap && (snap.fix_completed > 0 || snap.fix_failed > 0 || snap.fix_pending > 0 || snap.fix_needs_review > 0) && (
                      <div className="flex items-center gap-1.5 pt-0.5">
                        <span className="text-[10px] text-muted-foreground">Fixes:</span>
                        {snap.fix_completed > 0 && (
                          <span className="text-[10px] text-emerald-600">{snap.fix_completed} done</span>
                        )}
                        {snap.fix_needs_review > 0 && (
                          <button
                            onClick={() => onViewChange('tasks')}
                            className="text-[10px] text-amber-600 font-medium hover:text-amber-700 cursor-pointer bg-amber-50 px-1 py-0.5 rounded"
                          >
                            {snap.fix_needs_review} need review
                          </button>
                        )}
                        {snap.fix_failed > 0 && (
                          <span className="text-[10px] text-rose-600">{snap.fix_failed} failed</span>
                        )}
                        {snap.fix_pending > 0 && (
                          <span className="text-[10px] text-blue-600">{snap.fix_pending} pending</span>
                        )}
                      </div>
                    )}
                  </div>
                )
              })()}

              <div className="h-px bg-border my-2" />
              {quickActionError && (
                <div className="mb-2 px-2.5 py-2 rounded-md text-xs bg-destructive/10 text-destructive">
                  {quickActionError}
                </div>
              )}
              {isLiveSiteProject && liveSiteMsg && (
                <div className="mb-2 px-2.5 py-2 rounded-md text-xs bg-emerald-100 text-emerald-700">
                  {liveSiteMsg}
                </div>
              )}
              {(() => {
                const visibleActions = isLiveSiteProject
                  ? QUICK_ACTIONS.filter(a =>
                      ['research_keywords', 'research_landing_pages',
                       'reddit_opportunity_search'].includes(a.task_type)
                    )
                  : QUICK_ACTIONS
                const grouped = PHASE_ORDER.map(phase => ({
                  phase,
                  actions: visibleActions.filter(a => a.phase === phase),
                })).filter(g => g.actions.length > 0)
                return grouped.map(({ phase, actions }) => (
                  <div key={phase} className="space-y-1">
                    <div className="flex items-center gap-2 px-1 pt-2 pb-1">
                      <span className={cn(
                        'text-[10px] font-semibold uppercase tracking-wider px-1.5 py-0.5 rounded',
                        PHASE_BADGE[phase] ?? 'bg-secondary text-muted-foreground',
                      )}>
                        {phase}
                      </span>
                      <span className="text-[10px] text-muted-foreground">
                        {phase === 'collection' && 'Run first — weekly'}
                        {phase === 'research' && 'Monthly or as needed'}
                        {phase === 'investigation' && 'After collection — bi-weekly'}
                        {phase === 'implementation' && 'Spawned by investigations'}
                      </span>
                    </div>
                    {actions.map(action => {
                      const status = getActionStatus(action, overview?.workflow_activity ?? [])
                      return (
                        <button
                          key={action.task_type}
                          onClick={() => handleQuickAction(action)}
                          disabled={runningActionLabel === action.label}
                          className={cn(
                            'w-full flex items-center gap-3 px-3 py-2.5 rounded-md text-left transition-colors group',
                            'hover:bg-secondary disabled:opacity-50 disabled:cursor-not-allowed',
                            runningActionLabel === action.label && 'bg-secondary ring-1 ring-blue-700/50',
                          )}
                        >
                          <span className="shrink-0 text-muted-foreground">{action.icon}</span>
                          <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-2">
                              <span className="text-sm text-foreground font-medium">{action.label}</span>
                              <span className={cn(
                                'text-[10px] px-1.5 py-0.5 rounded',
                                FREQUENCY_BADGE[action.frequency] ?? 'bg-secondary text-muted-foreground',
                              )}>
                                {action.frequency}
                              </span>
                            </div>
                            <span className="text-xs text-muted-foreground leading-snug">{action.description}</span>
                          </div>
                          <div className="shrink-0 flex flex-col items-end gap-0.5">
                            {status.lastRun ? (
                              <span className={cn(
                                'text-[10px] shrink-0',
                                status.overdue ? 'text-amber-600 font-medium' : 'text-muted-foreground',
                              )}>
                                {relativeDate(status.lastRun)}
                              </span>
                            ) : (
                              <span className="text-[10px] text-destructive shrink-0 font-medium">never</span>
                            )}
                            {action.frequency !== 'as needed' && (
                              <span className={cn(
                                'text-[10px] px-1 py-0.5 rounded shrink-0',
                                status.overdue
                                  ? 'bg-amber-100 text-amber-700'
                                  : 'bg-emerald-100 text-emerald-700',
                              )}>
                                {status.dueText}
                              </span>
                            )}
                          </div>
                          {runningActionLabel === action.label
                            ? <RefreshCw size={13} className="shrink-0 animate-spin text-blue-600" />
                            : <PlayCircle size={13} className="shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100" />
                          }
                        </button>
                      )
                    })}
                  </div>
                ))
              })()}
              {isLiveSiteProject ? (
                <button
                  onClick={handleImportLiveSite}
                  disabled={liveSiteImporting}
                  className={cn(
                    'w-full flex items-center gap-3 px-3 py-2.5 rounded-md text-left transition-colors',
                    'hover:bg-secondary disabled:opacity-50 disabled:cursor-not-allowed',
                  )}
                >
                  <span className="shrink-0 text-muted-foreground"><Globe size={16} /></span>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="text-sm text-foreground font-medium">Import Site</span>
                      <span className={cn('text-xs px-1.5 py-0.5 rounded border-transparent', PHASE_BADGE['collection'])}>
                        collection
                      </span>
                    </div>
                    <span className="text-xs text-muted-foreground leading-snug">Re-crawl sitemap and refresh the live page inventory</span>
                  </div>
                  {liveSiteImporting
                    ? <RefreshCw size={13} className="shrink-0 animate-spin text-blue-600" />
                    : <PlayCircle size={13} className="shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100" />
                  }
                </button>
              ) : (
                <button
                  onClick={handleOpenPublish}
                  disabled={loadingPublish}
                  className={cn(
                    'w-full flex items-center gap-3 px-3 py-2.5 rounded-md text-left transition-colors',
                    'hover:bg-secondary disabled:opacity-50 disabled:cursor-not-allowed',
                  )}
                >
                  <span className="shrink-0 text-muted-foreground"><Send size={16} /></span>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="text-sm text-foreground font-medium">Publish Articles</span>
                      <span className={cn('text-xs px-1.5 py-0.5 rounded border-transparent', PHASE_BADGE['implementation'])}>
                        implementation
                      </span>
                    </div>
                    <span className="text-xs text-muted-foreground leading-snug">Fix dates, resolve mismatches, mark drafts as published</span>
                  </div>
                  {loadingPublish
                    ? <RefreshCw size={13} className="shrink-0 animate-spin text-blue-600" />
                    : <PlayCircle size={13} className="shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100" />
                  }
                </button>
              )}
            </CardContent>
          </Card>

          {/* Right column: workflow activity + recent tasks + jump nav */}
          <div className="space-y-4">

            {/* Pending landing page research tasks */}
            <PendingLandingPageCard 
              items={overview?.pending_landing_page_research ?? []}
              onViewTask={(taskId) => onViewChange('tasks', taskId)}
            />

            {/* Pending feature spec tasks */}
            <PendingFeatureSpecCard
              items={overview?.pending_feature_specs ?? []}
              onMarkDone={async (taskId) => {
                try {
                  await updateTaskStatus(taskId, 'done')
                  await load()
                } catch (e: unknown) {
                  setQuickActionError(String(e))
                }
              }}
            />

            {/* CTR Health Summary */}
            {!isLiveSiteProject && (
              <Card className="bg-card border-border">
                <CardHeader className="pb-2">
                  <CardTitle className="text-sm font-semibold text-foreground flex items-center gap-1.5">
                    <Activity size={13} className="text-muted-foreground" />
                    CTR Health
                  </CardTitle>
                </CardHeader>
                <CardContent className="pb-3">
                  {loadingCtrHealth ? (
                    <div className="flex items-center gap-2 py-2 text-xs text-muted-foreground">
                      <RefreshCw size={12} className="animate-spin" />
                      Loading CTR data…
                    </div>
                  ) : !ctrHealth || ctrHealth.total_articles === 0 ? (
                    <p className="text-xs text-muted-foreground py-2">No articles found. Run a CTR Audit to analyze your content.</p>
                  ) : (
                    <div className="space-y-2">
                      <div className="flex items-center justify-between">
                        <span className="text-xs text-muted-foreground">Last audit</span>
                        <span className="text-xs text-foreground">
                          {ctrHealth.last_audit_at ? relativeDate(ctrHealth.last_audit_at) : 'Never'}
                        </span>
                      </div>
                      <div className="flex items-center justify-between">
                        <span className="text-xs text-muted-foreground">Articles covered</span>
                        <span className="text-xs text-foreground">{ctrHealth.total_articles}</span>
                      </div>
                      <div className="flex items-center justify-between">
                        <span className="text-xs text-muted-foreground">Open CTR issues</span>
                        <span className={cn(
                          'text-xs font-medium px-1.5 py-0.5 rounded',
                          ctrHealth.open_issues_count > 0
                            ? 'bg-amber-100 text-amber-700'
                            : 'bg-emerald-100 text-emerald-700',
                        )}>
                          {ctrHealth.open_issues_count}
                        </span>
                      </div>
                      {ctrHealth.healthy_count > 0 && (
                        <div className="flex items-center justify-between">
                          <span className="text-xs text-muted-foreground">Healthy</span>
                          <span className="text-xs text-emerald-600">{ctrHealth.healthy_count}</span>
                        </div>
                      )}
                      {ctrHealth.improved_count > 0 && (
                        <div className="flex items-center justify-between">
                          <span className="text-xs text-muted-foreground">Improved since last audit</span>
                          <span className="text-xs text-emerald-600">{ctrHealth.improved_count}</span>
                        </div>
                      )}
                      {ctrHealth.regressed_count > 0 && (
                        <div className="flex items-center justify-between">
                          <span className="text-xs text-muted-foreground">Regressed</span>
                          <span className="text-xs text-destructive">{ctrHealth.regressed_count}</span>
                        </div>
                      )}
                      {ctrHealth.missing_files > 0 && (
                        <div className="flex items-center justify-between">
                          <span className="text-xs text-muted-foreground">Missing files</span>
                          <span className="text-xs text-destructive">{ctrHealth.missing_files}</span>
                        </div>
                      )}
                      {(ctrHealth.pending_fix_tasks > 0 || ctrHealth.completed_audits > 0) && (
                        <div className="flex items-center justify-between">
                          <span className="text-xs text-muted-foreground">Pipeline</span>
                          <span className="text-xs text-foreground">
                            {ctrHealth.pending_fix_tasks > 0
                              ? `Wave ${ctrHealth.completed_audits + 1} — ${ctrHealth.pending_fix_tasks} fix task${ctrHealth.pending_fix_tasks !== 1 ? 's' : ''} pending`
                              : `${ctrHealth.completed_audits} wave${ctrHealth.completed_audits !== 1 ? 's' : ''} completed`}
                          </span>
                        </div>
                      )}
                      <div className="pt-1 space-y-1.5">
                        <button
                          onClick={async () => {
                            if (!project || runningCtr) return
                            setRunningCtr(true)
                            setQuickActionError(null)
                            try {
                              const task = await createTask(
                                project.id,
                                'ctr_audit',
                                'CTR Audit',
                                'Full CTR audit run',
                                'medium',
                              )
                              const queue = useQueueStore.getState()
                              queue.enqueue([{
                                task_id: task.id,
                                project_id: project.id,
                                title: task.title ?? 'CTR Audit',
                                task_type: 'ctr_audit',
                                project_name: project.name,
                              }])
                              // Refresh CTR health after queue starts
                              await refetchCtrHealth?.()
                            } catch (e: unknown) {
                              setQuickActionError(String(e))
                            } finally {
                              setRunningCtr(false)
                            }
                          }}
                          disabled={runningCtr}
                          className={cn(
                            'w-full flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-md text-xs transition-colors',
                            'bg-primary hover:bg-primary/90 text-primary-foreground',
                            'disabled:opacity-50 disabled:cursor-not-allowed',
                          )}
                        >
                          {runningCtr ? (
                            <>
                              <RefreshCw size={11} className="animate-spin" />
                              Running CTR Audit…
                            </>
                          ) : (
                            <>
                              <PlayCircle size={11} />
                              Run CTR Audit
                            </>
                          )}
                        </button>
                        <button
                          onClick={async () => {
                            if (!project || repairingPaths) return
                            setRepairingPaths(true)
                            setRepairResult(null)
                            try {
                              const result = await repairArticlePaths(project.id)
                              setRepairResult(result)
                              await refetchCtrHealth?.()
                            } catch (e: unknown) {
                              setQuickActionError(String(e))
                            } finally {
                              setRepairingPaths(false)
                            }
                          }}
                          disabled={repairingPaths}
                          className={cn(
                            'w-full flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-md text-xs transition-colors',
                            'bg-secondary hover:bg-secondary/80 text-foreground',
                            'disabled:opacity-50 disabled:cursor-not-allowed',
                          )}
                        >
                          {repairingPaths ? (
                            <>
                              <RefreshCw size={11} className="animate-spin" />
                              Repairing paths…
                            </>
                          ) : (
                            <>
                              <FileText size={11} />
                              Repair article paths
                            </>
                          )}
                        </button>
                        {repairResult && (
                          <div className="mt-1.5 text-[11px] text-muted-foreground space-y-0.5">
                            <p>
                              Checked {repairResult.checked}, repaired {repairResult.repaired}, removed {repairResult.removed}.
                            </p>
                            {repairResult.not_found.length > 0 && (
                              <details>
                                <summary className="cursor-pointer text-destructive">
                                  {repairResult.not_found.length} stale article(s) removed (no MDX on disk)
                                </summary>
                                <ul className="pl-3 mt-1 space-y-0.5">
                                  {repairResult.not_found.map((f, i) => (
                                    <li key={i} className="truncate">{f}</li>
                                  ))}
                                </ul>
                              </details>
                            )}
                          </div>
                        )}
                      </div>
                    </div>
                  )}
                </CardContent>
              </Card>
            )}

            {/* Recent tasks */}
            <Card className="bg-card border-border">
              <CardHeader className="pb-2 flex flex-row items-center justify-between">
                <CardTitle className="text-sm font-semibold text-foreground">Recent Activity</CardTitle>
                <button
                  onClick={() => onViewChange('tasks')}
                  className="text-xs text-muted-foreground hover:text-foreground flex items-center gap-0.5 transition-colors"
                >
                  All tasks <ChevronRight size={12} />
                </button>
              </CardHeader>
              <CardContent className="pb-3">
                {!overview || overview.recent_tasks.length === 0 ? (
                  <p className="text-xs text-muted-foreground py-2">No tasks yet. Run a workflow to get started.</p>
                ) : (
                  <div className="space-y-0.5">
                    {overview.recent_tasks.map(task => (
                      <button
                        key={task.id}
                        onClick={() => onViewChange('tasks', task.id)}
                        className="w-full flex items-center gap-2.5 py-1.5 px-1 rounded-sm hover:bg-secondary/60 transition-colors text-left cursor-pointer"
                      >
                        <span className={cn('shrink-0', STATUS_COLORS[task.status] ?? 'text-muted-foreground')}>
                          {STATUS_ICONS[task.status] ?? <Clock size={13} />}
                        </span>
                        <span className="flex-1 min-w-0 text-xs text-foreground truncate">
                          {task.title ?? task.task_type}
                        </span>
                        <span className="text-xs text-muted-foreground shrink-0">{timeAgo(task.updated_at)}</span>
                      </button>
                    ))}
                  </div>
                )}
              </CardContent>
            </Card>

            {/* Navigation shortcuts */}
            <Card className="bg-card border-border">
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-semibold text-foreground flex items-center gap-1.5">
                  <BarChart2 size={13} className="text-muted-foreground" />
                  Jump To
                </CardTitle>
              </CardHeader>
              <CardContent className="pb-3 grid grid-cols-2 gap-1.5">
                {([
                  ['articles', 'Articles', <FileText size={13} />],
                  ['health', 'Health Audit', <HeartPulse size={13} />],
                  ['gsc', 'Search Console', <Globe size={13} />],
                  ['reddit', 'Reddit', <Users size={13} />],
                  ['scheduler', 'Scheduler', <Clock size={13} />],
                  ['history', 'Run History', <RefreshCw size={13} />],
                  ['settings', 'Settings', <Cpu size={13} />],
                ] as [import('../../lib/types').View, string, React.ReactNode][]).map(([view, label, icon]) => (
                  <button
                    key={view}
                    onClick={() => onViewChange(view)}
                    className="flex items-center gap-2 px-3 py-2 rounded-md bg-secondary hover:bg-secondary/80 text-xs text-foreground transition-colors text-left"
                  >
                    <span className="text-muted-foreground shrink-0">{icon}</span>
                    {label}
                    <ArrowRight size={11} className="ml-auto text-muted-foreground" />
                  </button>
                ))}
              </CardContent>
            </Card>

          </div>
        </div>
      </div>

    </div>
    {/* Landing Page Research Dialog */}
    {lpDialogOpen && (
      <div
        className="fixed inset-0 z-50 flex items-center justify-center"
        style={{ background: 'rgba(0,0,0,0.5)' }}
        onClick={e => { if (e.target === e.currentTarget) setLpDialogOpen(false) }}
      >
        <div className="bg-card border border-border rounded-lg shadow-xl w-112.5">
          <div className="flex items-center justify-between px-5 py-4 border-b border-border">
            <h2 className="text-sm font-semibold text-foreground">Landing Page Research</h2>
            <button 
              onClick={() => setLpDialogOpen(false)}
              className="text-muted-foreground hover:text-foreground"
            >
              ✕
            </button>
          </div>
          
          <div className="px-5 py-5 space-y-4">
            {quickActionError && (
              <div className="px-3 py-2 rounded-md text-sm bg-destructive/15 text-destructive">
                {quickActionError}
              </div>
            )}
            
            <div className="space-y-1.5">
              <label className="text-xs text-muted-foreground">
                Strategy Context <span className="text-muted-foreground/50">(optional)</span>
              </label>
              <textarea
                value={lpContext}
                onChange={e => setLpContext(e.target.value)}
                placeholder={'Describe your landing page goals, target audience, and what makes your offering unique.\n\nExamples:\n• "Enterprise CRM for real estate agents"\n• "Looking for high-intent comparison terms"\n• "Target: CTOs at Series A startups"'}
                rows={5}
                className="w-full bg-background border border-border text-foreground text-sm resize-none rounded-md px-3 py-2"
              />
              <p className="text-[11px] text-muted-foreground leading-relaxed">
                This context helps guide keyword selection for conversion-focused landing pages.
              </p>
            </div>
            
            <div className="space-y-1.5">
              <label className="text-xs text-muted-foreground">
                Keyword Themes <span className="text-muted-foreground/50">(optional — auto-derived if blank)</span>
              </label>
              <textarea
                value={lpThemes}
                onChange={e => setLpThemes(e.target.value)}
                placeholder={'Enter topics, one per line\nExample:\ncoffee brewing methods\nespresso guides'}
                rows={3}
                className="w-full bg-background border border-border text-foreground text-sm resize-none rounded-md px-3 py-2"
              />
            </div>
          </div>
          
          <div className="px-5 pb-5 flex items-center justify-end gap-2">
            <button
              onClick={() => setLpDialogOpen(false)}
              className="px-3 py-1.5 text-xs text-muted-foreground hover:text-foreground transition-colors"
            >
              Cancel
            </button>
            <button
              onClick={async () => {
                if (!project) return
                setLpCreating(true)
                setQuickActionError(null)
                try {
                  const themesList = lpThemes.trim()
                    ? lpThemes.split('\n').map(t => t.trim()).filter(Boolean)
                    : undefined
                  const description = JSON.stringify({
                    context: lpContext.trim(),
                    themes: themesList,
                  })
                  const task = await createTask(
                    project.id,
                    'research_landing_pages',
                    `Landing Page Research — ${new Date().toLocaleDateString()}`,
                    description,
                    'medium',
                  )
                  setLpDialogOpen(false)
                  setLpContext('')
                  setLpThemes('')
                  onRunTasks?.([task])
                  await load()
                } catch (e: unknown) {
                  setQuickActionError(String(e))
                } finally {
                  setLpCreating(false)
                }
              }}
              disabled={lpCreating}
              className="px-3 py-1.5 text-xs bg-primary text-primary-foreground rounded-md hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
            >
              {lpCreating ? 'Creating...' : 'Start Research'}
            </button>
          </div>
        </div>
      </div>
    )}

    <PublishPanel
      open={publishOpen}
      onOpenChange={setPublishOpen}
      projectId={project.id}
      candidates={publishCandidates}
      onPublished={() => load()}
    />
    </>
  )
}
