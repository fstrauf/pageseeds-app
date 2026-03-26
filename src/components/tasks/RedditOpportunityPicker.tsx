import { useEffect, useMemo, useState } from 'react'
import { CheckSquare, Square, Loader2, ExternalLink, MessageSquare } from 'lucide-react'
import { createRedditReplyTasks } from '../../lib/tauri'
import { useQueue } from '../../lib/queue-context'
import type { Task } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'
import { cn } from '../../lib/utils'

interface RedditOpportunity {
  post_id: string
  title?: string
  url?: string
  subreddit?: string
  severity?: string
  final_score?: number
  why_relevant?: string
  reply_text?: string
  author?: string
  upvotes?: number
  comment_count?: number
  posted_date?: string
}

interface RedditOpportunityPickerProps {
  task: Task
  onTasksCreated: (tasks: Task[]) => void
}

interface OpportunityRow {
  opportunity: RedditOpportunity
  selected: boolean
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

function severityColor(severity?: string): string {
  switch (severity?.toUpperCase()) {
    case 'CRITICAL':
      return 'bg-red-100 text-red-700 border-transparent'
    case 'HIGH':
      return 'bg-orange-100 text-orange-700 border-transparent'
    case 'MEDIUM':
      return 'bg-amber-100 text-amber-700 border-transparent'
    default:
      return 'bg-slate-100 text-slate-700 border-transparent'
  }
}

function formatScore(score?: number): string {
  if (score == null) return '—'
  return score.toFixed(1)
}

function truncate(str: string, maxLen: number): string {
  if (str.length <= maxLen) return str
  return str.slice(0, maxLen - 3) + '...'
}

function formatDate(dateStr?: string): string {
  if (!dateStr) return '—'
  try {
    const date = new Date(dateStr)
    return date.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
  } catch {
    return dateStr
  }
}

// ─── Component ───────────────────────────────────────────────────────────────

export function RedditOpportunityPicker({ task, onTasksCreated }: RedditOpportunityPickerProps) {
  const queue = useQueue()
  const [rows, setRows] = useState<OpportunityRow[]>([])
  const [creating, setCreating] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [expandedReply, setExpandedReply] = useState<string | null>(null)

  // Parse opportunities from task artifact
  useEffect(() => {
    const resultsArtifact = task.artifacts.find(a => a.key === 'reddit_results_stage')
    if (!resultsArtifact?.content) {
      setError('No results found. The search may have failed or returned no opportunities.')
      return
    }

    try {
      const opportunities: RedditOpportunity[] = JSON.parse(resultsArtifact.content)
      // Sort by final_score descending
      const sorted = opportunities.sort((a, b) => (b.final_score || 0) - (a.final_score || 0))
      setRows(sorted.map(opp => ({ opportunity: opp, selected: false })))
    } catch (e) {
      setError('Failed to parse search results. The data may be corrupted.')
    }
  }, [task])

  const selectedCount = useMemo(() => rows.filter(r => r.selected).length, [rows])
  const criticalCount = useMemo(() => rows.filter(r => r.opportunity.severity?.toUpperCase() === 'CRITICAL').length, [rows])
  const highCount = useMemo(() => rows.filter(r => r.opportunity.severity?.toUpperCase() === 'HIGH').length, [rows])

  function toggleAll(selected: boolean) {
    setRows(rows.map(r => ({ ...r, selected })))
  }

  function toggleSelectHighPriority() {
    setRows(rows.map(r => ({
      ...r,
      selected: ['CRITICAL', 'HIGH'].includes(r.opportunity.severity?.toUpperCase() || '')
    })))
  }

  function toggleRow(postId: string) {
    setRows(rows.map(r =>
      r.opportunity.post_id === postId ? { ...r, selected: !r.selected } : r
    ))
  }

  async function handleCreateTasks() {
    const selectedIds = rows.filter(r => r.selected).map(r => r.opportunity.post_id)
    if (selectedIds.length === 0) return

    setCreating(true)
    setError(null)

    try {
      const newTasks = await createRedditReplyTasks(task.id, selectedIds)
      
      // Auto-add created tasks to the queue (shopping cart pattern)
      if (newTasks.length > 0) {
        queue.enqueueNext(
          newTasks.map(t => ({
            taskId: t.id,
            projectId: t.projectId ?? task.project_id,
            title: t.title ?? 'Reply to Reddit post',
            taskType: t.type ?? 'reddit_reply',
            projectName: task.projectName,
          }))
        )
      }
      
      onTasksCreated(newTasks)
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setCreating(false)
    }
  }

  if (error) {
    return (
      <div className="rounded-md bg-destructive/10 text-destructive px-3 py-2.5 text-xs">
        {error}
      </div>
    )
  }

  if (rows.length === 0) {
    return (
      <div className="text-xs text-muted-foreground py-4">
        No opportunities found. Try running the search again with different keywords.
      </div>
    )
  }

  return (
    <div className="space-y-3">
      {/* Summary */}
      <div className="flex items-center justify-between text-xs">
        <div className="flex items-center gap-3">
          <span className="text-muted-foreground">
            {rows.length} opportunities found
          </span>
          {criticalCount > 0 && (
            <Badge className="bg-red-100 text-red-700 border-transparent text-[10px]">
              {criticalCount} Critical
            </Badge>
          )}
          {highCount > 0 && (
            <Badge className="bg-orange-100 text-orange-700 border-transparent text-[10px]">
              {highCount} High
            </Badge>
          )}
        </div>
        <div className="flex items-center gap-2">
          <Button
            variant="ghost"
            size="xs"
            onClick={() => toggleSelectHighPriority()}
            className="text-[10px] h-6"
          >
            Select High Priority
          </Button>
          <Button
            variant="ghost"
            size="xs"
            onClick={() => toggleAll(true)}
            className="text-[10px] h-6"
          >
            All
          </Button>
          <Button
            variant="ghost"
            size="xs"
            onClick={() => toggleAll(false)}
            className="text-[10px] h-6"
          >
            None
          </Button>
        </div>
      </div>

      {/* Opportunities list */}
      <div className="space-y-2 max-h-96 overflow-y-auto pr-1">
        {rows.map((row) => {
          const opp = row.opportunity
          const isExpanded = expandedReply === opp.post_id

          return (
            <div
              key={opp.post_id}
              className={cn(
                'rounded-lg border p-3 space-y-2 transition-colors',
                row.selected
                  ? 'border-primary bg-primary/5'
                  : 'border-border bg-background hover:bg-secondary/40'
              )}
            >
              {/* Header row */}
              <div className="flex items-start gap-3">
                <button
                  onClick={() => toggleRow(opp.post_id)}
                  className="mt-0.5 shrink-0 text-muted-foreground hover:text-foreground"
                >
                  {row.selected ? <CheckSquare size={16} /> : <Square size={16} />}
                </button>

                <div className="flex-1 min-w-0 space-y-1">
                  <div className="flex items-start justify-between gap-2">
                    <h4 className="text-sm font-medium text-foreground leading-tight">
                      {truncate(opp.title || 'Untitled Post', 100)}
                    </h4>
                    {opp.url && (
                      <a
                        href={opp.url}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="shrink-0 text-muted-foreground hover:text-foreground"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <ExternalLink size={12} />
                      </a>
                    )}
                  </div>

                  <div className="flex items-center gap-2 flex-wrap">
                    <Badge variant="outline" className="text-[10px] border-border">
                      r/{opp.subreddit || 'unknown'}
                    </Badge>
                    {opp.posted_date && (
                      <span className="text-[10px] text-muted-foreground">
                        {formatDate(opp.posted_date)}
                      </span>
                    )}
                    {opp.severity && (
                      <Badge className={cn('text-[10px]', severityColor(opp.severity))}>
                        {opp.severity} ({formatScore(opp.final_score)})
                      </Badge>
                    )}
                    {opp.upvotes != null && (
                      <span className="text-[10px] text-muted-foreground">
                        {opp.upvotes} upvotes
                      </span>
                    )}
                  </div>
                </div>
              </div>

              {/* Why relevant */}
              {opp.why_relevant && (
                <p className="text-xs text-muted-foreground pl-7">
                  {opp.why_relevant}
                </p>
              )}

              {/* Draft reply (expandable) */}
              {opp.reply_text && (
                <div className="pl-7">
                  <button
                    onClick={() => setExpandedReply(isExpanded ? null : opp.post_id)}
                    className="flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground"
                  >
                    <MessageSquare size={12} />
                    {isExpanded ? 'Hide draft reply' : 'Show draft reply'}
                  </button>

                  {isExpanded && (
                    <div className="mt-2 p-3 rounded-md bg-secondary/60 text-xs text-foreground whitespace-pre-wrap">
                      {opp.reply_text}
                    </div>
                  )}
                </div>
              )}
            </div>
          )
        })}
      </div>

      <Separator className="bg-border" />

      {/* Footer actions */}
      <div className="flex items-center justify-between">
        <div className="text-xs text-muted-foreground">
          {selectedCount === 0
            ? 'Select opportunities to create reply tasks'
            : `${selectedCount} selected`}
        </div>

        <Button
          size="sm"
          onClick={handleCreateTasks}
          disabled={selectedCount === 0 || creating}
        >
          {creating ? (
            <>
              <Loader2 size={14} className="mr-1.5 animate-spin" />
              Creating...
            </>
          ) : (
            <>Create Reply Tasks</>
          )}
        </Button>
      </div>
    </div>
  )
}
