import React, { useCallback, useEffect, useRef, useState } from 'react'
import {
  Zap, RefreshCw, CheckCircle2, Clock, AlertCircle,
  BarChart2, FileText, Search, Globe, BookOpen, Cpu, ChevronRight,
  PlayCircle, TrendingUp, Users, ArrowRight,
} from 'lucide-react'
import { getProjectOverview, quickRunWorkflow } from '../../lib/tauri'
import type { Project, ProjectOverview, WorkflowActivity } from '../../lib/types'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { ActionDrawer } from '@/components/ui/action-drawer'
import { useActionRun } from '../../hooks/useActionRun'
import { cn } from '../../lib/utils'
import { SetupWarnings } from './SetupWarnings'

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
    description: 'Find new long-tail keyword opportunities and append drafts to articles.json',
    icon: <Search size={16} />,
    phase: 'research',
    nextView: 'articles',
    nextLabel: 'Review new draft articles',
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

// ─── Main component ───────────────────────────────────────────────────────────

interface OverviewProps {
  project: Project | null
  onViewChange: (view: import('../../lib/types').View, taskId?: string) => void
}

export function Overview({ project, onViewChange }: OverviewProps) {
  const [overview, setOverview] = useState<ProjectOverview | null>(null)
  const [loading, setLoading] = useState(false)
  const { state: actionState, run: runAction, dismiss: dismissAction } = useActionRun()
  const pollRef = useRef<ReturnType<typeof setTimeout> | null>(null)

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
    return () => { if (pollRef.current) clearTimeout(pollRef.current) }
  }, [load])

  async function handleQuickAction(action: ActionDef) {
    if (!project || actionState.status === 'running') return
    await runAction(
      action.label,
      async () => {
        const result = await quickRunWorkflow(
          project.id,
          action.task_type,
          `${action.label} — ${new Date().toLocaleDateString()}`,
        )
        await load()
        return { kind: 'execution' as const, data: result }
      },
      { view: action.nextView, label: action.nextLabel },
    )
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

  return (
    <div className="flex flex-col h-full overflow-y-auto bg-background">
      <div className="p-6 space-y-6 pb-20">

        {/* Header */}
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-base font-semibold text-foreground">{project.name}</h1>
            {project.site_url && (
              <div className="text-xs text-muted-foreground mt-0.5">{project.site_url}</div>
            )}
          </div>
          <Button variant="ghost" size="icon-sm" onClick={load} disabled={loading} className="text-muted-foreground">
            <RefreshCw size={13} className={loading ? 'animate-spin' : ''} />
          </Button>
        </div>

        {/* Setup warnings — shown when workspace config is missing or content dir is guessed */}
        <SetupWarnings projectId={project.id} />

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
              <div className="text-xs text-muted-foreground mb-1">Articles</div>
              <div className="text-2xl font-bold text-foreground">{articles?.total ?? '—'}</div>
              <div className="text-xs text-muted-foreground mt-1">
                {articles ? `${articles.published} published · ${articles.draft} draft` : ''}
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
              {QUICK_ACTIONS.map(action => (
                <button
                  key={action.task_type}
                  onClick={() => handleQuickAction(action)}
                  disabled={actionState.status === 'running'}
                  className={cn(
                    'w-full flex items-center gap-3 px-3 py-2.5 rounded-md text-left transition-colors',
                    'hover:bg-secondary disabled:opacity-50 disabled:cursor-not-allowed',
                    actionState.status === 'running' && actionState.label === action.label && 'bg-secondary ring-1 ring-blue-700/50',
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
                  {actionState.status === 'running' && actionState.label === action.label
                    ? <RefreshCw size={13} className="shrink-0 animate-spin text-blue-600" />
                    : <PlayCircle size={13} className="shrink-0 text-muted-foreground opacity-0 group-hover:opacity-100" />
                  }
                </button>
              ))}
            </CardContent>
          </Card>

          {/* Right column: workflow activity + recent tasks + jump nav */}
          <div className="space-y-4">

            {/* Workflow activity timeline */}
            <WorkflowActivityCard
              items={overview?.workflow_activity ?? []}
              lastPublishedDate={overview?.articles.last_published_date}
            />

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

      {/* Slide-up execution drawer */}
      <ActionDrawer
        state={actionState}
        onDismiss={dismissAction}
        onNavigate={(view, taskId) => onViewChange(view as import('../../lib/types').View, taskId)}
      />
    </div>
  )
}
