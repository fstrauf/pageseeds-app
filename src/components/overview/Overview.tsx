import React, { useCallback, useEffect, useRef, useState } from 'react'
import {
  Zap, RefreshCw, CheckCircle2, Clock, AlertCircle,
  BarChart2, FileText, Search, Globe, BookOpen, Cpu, ChevronRight,
  PlayCircle, TrendingUp, Users, ArrowRight, Send, PieChart, Target,
  Activity, Wrench,
} from 'lucide-react'
import { createTask, getCtrHealthSummary, getProjectOverview, importLiveSite, listArticles, listLiveSitePages, repairArticlePaths } from '../../lib/tauri'
import { useQueueStore } from '@/stores/queueStore'
import type { Article, LandingPageResearchPending, Project, ProjectOverview, Task, WorkflowActivity } from '../../lib/types'
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
  nextView: import('../../lib/types').View
  nextLabel: string
}

const QUICK_ACTIONS: ActionDef[] = [
  {
    task_type: 'research_keywords',
    label: 'Keyword Research',
    description: 'Find new long-tail keyword opportunities for your site, then select which to write about',
    icon: <Search size={16} />,
    phase: 'research',
    nextView: 'tasks',
    nextLabel: 'Select keywords & create articles',
  },
  {
    task_type: 'research_landing_pages',
    label: 'Landing Page Research',
    description: 'Research high-intent keywords for conversion-focused landing pages with strategic context',
    icon: <Target size={16} />,
    phase: 'research',
    nextView: 'tasks',
    nextLabel: 'Select keywords & create landing pages',
  },
  {
    task_type: 'content_review',
    label: 'Content Review',
    description: 'Sync GSC data and generate recommendations for the highest-priority article',
    icon: <TrendingUp size={16} />,
    phase: 'investigation',
    nextView: 'tasks',
    nextLabel: 'See optimization tasks',
  },
  {
    task_type: 'collect_gsc',
    label: 'GSC Collection',
    description: 'Pull the latest Search Console analytics into the project workspace',
    icon: <Globe size={16} />,
    phase: 'collection',
    nextView: 'gsc',
    nextLabel: 'View Search Console data',
  },
  {
    task_type: 'reddit_opportunity_search',
    label: 'Reddit Search',
    description: 'Search subreddits for posts to engage with and save pending opportunities',
    icon: <Users size={16} />,
    phase: 'research',
    nextView: 'reddit',
    nextLabel: 'Review pending opportunities',
  },
  {
    task_type: 'content_cleanup',
    label: 'Content Cleanup',
    description: 'Scan MDX files for structural issues — heading duplicates, broken frontmatter',
    icon: <FileText size={16} />,
    phase: 'implementation',
    nextView: 'tasks',
    nextLabel: 'See cleanup tasks',
  },
  {
    task_type: 'indexing_diagnostics',
    label: 'Indexing Diagnostics',
    description: 'Inspect sitemap URLs in Search Console and find non-indexed pages',
    icon: <BookOpen size={16} />,
    phase: 'investigation',
    nextView: 'tasks',
    nextLabel: 'See indexing tasks',
  },
  {
    task_type: 'analyze_keyword_coverage',
    label: 'Analyze Coverage',
    description: 'Analyze your content portfolio and identify topic clusters + coverage gaps',
    icon: <PieChart size={16} />,
    phase: 'research',
    nextView: 'tasks',
    nextLabel: 'View coverage results',
  },
  {
    task_type: 'ctr_audit',
    label: 'CTR Audit',
    description: 'Analyze titles, meta descriptions, and snippet readiness to fix low CTR',
    icon: <BarChart2 size={16} />,
    phase: 'investigation',
    nextView: 'tasks',
    nextLabel: 'See CTR fix tasks',
  },
  {
    task_type: 'cannibalization_audit',
    label: 'Cannibalization Audit',
    description: 'Detect overlapping content, find merge candidates, and identify hub gaps',
    icon: <Target size={16} />,
    phase: 'investigation',
    nextView: 'tasks',
    nextLabel: 'See merge & hub tasks',
  },
  {
    task_type: 'sanitize_content',
    label: 'Sanitize Content',
    description: 'Normalize frontmatter field names (metaDescription → description) across all MDX files',
    icon: <Wrench size={16} />,
    phase: 'implementation',
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

// ─── Workflow activity helpers ───────────────────────────────────────────────

const WORKFLOW_ICONS: Record<string, React.ReactNode> = {
  research_keywords:        <Search size={13} />,
  content_review:           <TrendingUp size={13} />,
  reddit_opportunity_search:<Users size={13} />,
  collect_gsc:              <Globe size={13} />,
  ctr_audit:                <BarChart2 size={13} />,
  sanitize_content:         <Wrench size={13} />,
}

function relativeDate(iso: string): string {
  const diffMs = Date.now() - new Date(iso).getTime()
  if (diffMs < 0) return 'just now'
  const days = Math.floor(diffMs / (1000 * 60 * 60 * 24))
  if (days === 0) return 'today'
  if (days === 1) return '1d ago'
  return `${days}d ago`
}

function nextDueLabel(iso: string): { text: string; overdue: boolean } {
  const diffMs = new Date(iso).getTime() - Date.now()
  if (diffMs <= 0) return { text: 'overdue', overdue: true }
  const days = Math.ceil(diffMs / (1000 * 60 * 60 * 24))
  if (days === 0) return { text: 'due today', overdue: true }
  if (days === 1) return { text: 'in 1d', overdue: false }
  return { text: `in ${days}d`, overdue: false }
}

function WorkflowActivityCard({ items, lastPublishedDate }: { items: WorkflowActivity[]; lastPublishedDate?: string }) {
  if ((!items || items.length === 0) && !lastPublishedDate) return null
  return (
    <Card className="bg-card border-border">
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-semibold text-foreground flex items-center gap-1.5">
          <Clock size={13} className="text-muted-foreground" />
          Workflow Activity
        </CardTitle>
      </CardHeader>
      <CardContent className="pb-3">
        <div className="space-y-0">
          {lastPublishedDate && (
            <div className="flex items-center gap-2.5 py-1.5 px-1">
              <span className="shrink-0 text-muted-foreground"><FileText size={13} /></span>
              <span className="flex-1 min-w-0 text-xs text-foreground">Last article published</span>
              <span className="text-xs text-muted-foreground shrink-0">
                {relativeDate(lastPublishedDate + 'T12:00:00Z')}
              </span>
              <span className="text-xs text-muted-foreground shrink-0">{lastPublishedDate}</span>
            </div>
          )}
          {items.map(item => (
            <div key={item.task_type} className="flex items-center gap-2.5 py-1.5 px-1">
              <span className="shrink-0 text-muted-foreground">
                {WORKFLOW_ICONS[item.task_type] ?? <Clock size={13} />}
              </span>
              <span className="flex-1 min-w-0 text-xs text-foreground">{item.label}</span>
              <span className="text-xs text-muted-foreground shrink-0">
                {item.last_run_at ? relativeDate(item.last_run_at) : 'never'}
              </span>
              {item.next_due_at && (() => {
                const due = nextDueLabel(item.next_due_at)
                return (
                  <span className={cn(
                    'text-xs shrink-0 px-1.5 py-0.5 rounded',
                    due.overdue
                      ? 'bg-amber-100 text-amber-700'
                      : 'bg-secondary text-muted-foreground',
                  )}>
                    {due.text}
                  </span>
                )
              })()}
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  )
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

// ─── Main component ───────────────────────────────────────────────────────────

interface OverviewProps {
  project: Project | null
  onViewChange: (view: import('../../lib/types').View, taskId?: string) => void
  onRunTasks?: (tasks: Task[]) => void
  runCompletedTick?: number
}

export function Overview({
  project,
  onViewChange,
  onRunTasks,
  runCompletedTick = 0,
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
  }, [project, runCompletedTick, load])

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
          <Button variant="ghost" size="icon-sm" onClick={load} disabled={loading} className="text-muted-foreground">
            <RefreshCw size={13} className={loading ? 'animate-spin' : ''} />
          </Button>
        </div>

        {!isLiveSiteProject && (
          <SetupWarnings projectId={project.id} onViewChange={onViewChange} />
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
              {(isLiveSiteProject
                ? QUICK_ACTIONS.filter(a =>
                    ['research_keywords', 'research_landing_pages', 'collect_gsc',
                     'reddit_opportunity_search', 'analyze_keyword_coverage'].includes(a.task_type)
                  )
                : QUICK_ACTIONS
              ).map(action => (
                <button
                  key={action.task_type}
                  onClick={() => handleQuickAction(action)}
                  disabled={runningActionLabel === action.label}
                  className={cn(
                    'w-full flex items-center gap-3 px-3 py-2.5 rounded-md text-left transition-colors',
                    'hover:bg-secondary disabled:opacity-50 disabled:cursor-not-allowed',
                    runningActionLabel === action.label && 'bg-secondary ring-1 ring-blue-700/50',
                  )}
                >
                  <span className="shrink-0 text-muted-foreground">{action.icon}</span>
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="text-sm text-foreground font-medium">{action.label}</span>
                      <span className={cn(
                        'text-xs px-1.5 py-0.5 rounded border-transparent',
                        PHASE_BADGE[action.phase] ?? 'bg-secondary text-muted-foreground',
                      )}>
                        {action.phase}
                      </span>
                    </div>
                    <span className="text-xs text-muted-foreground leading-snug">{action.description}</span>
                  </div>
                  {runningActionLabel === action.label
                    ? <RefreshCw size={13} className="shrink-0 animate-spin text-blue-600" />
                    : <PlayCircle size={13} className="shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100" />
                  }
                </button>
              ))}
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

            {/* Workflow activity timeline */}
            <WorkflowActivityCard
              items={overview?.workflow_activity ?? []}
              lastPublishedDate={overview?.articles.last_published_date}
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
                                taskId: task.id,
                                projectId: project.id,
                                title: task.title ?? 'CTR Audit',
                                taskType: 'ctr_audit',
                                projectName: project.name,
                                status: 'pending',
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
                      <div key={task.id} className="flex items-center gap-2.5 py-1.5 px-1">
                        <span className={cn('shrink-0', STATUS_COLORS[task.status] ?? 'text-muted-foreground')}>
                          {STATUS_ICONS[task.status] ?? <Clock size={13} />}
                        </span>
                        <span className="flex-1 min-w-0 text-xs text-foreground truncate">
                          {task.title ?? task.task_type}
                        </span>
                        <span className="text-xs text-muted-foreground shrink-0">{timeAgo(task.updated_at)}</span>
                      </div>
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
