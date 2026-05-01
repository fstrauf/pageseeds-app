import { useMemo, useCallback, useEffect, useState, useRef } from 'react'
import {
  Check,
  X,
  AlertCircle,
  GitMerge,
  BookOpen,
  MapPin,
  Calculator,
  Play,
  Loader2,
  RotateCcw,
  Eye,
  EyeOff,
  Info,
} from 'lucide-react'
import { cn } from '@/lib/utils'
import { useQuery, useMutation } from '@/hooks/useQuery'
import { useErrorHandler } from '@/lib/toast-context'
import {
  getCannibalizationStrategy,
  setRecommendationApproval,
  createTasksFromApprovedRecommendations,
  enqueueTasks,
} from '@/lib/tauri'
import type {
  MergeRecommendation,
  HubRecommendation,
  TerritoryRecommendation,
  CalculatorRecommendation,
  StrategyReview,
  RecommendationTaskStatus,
} from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from '@/components/ui/card'
import { Tabs, TabsContent, TabsList, TabsTrigger } from '@/components/ui/tabs'
import { Separator } from '@/components/ui/separator'
import { ScrollArea } from '@/components/ui/scroll-area'

interface Props {
  projectId: string
}

type RecType = 'merge' | 'hub' | 'territory' | 'calculator'
type ApprovalStatus = 'approved' | 'rejected' | 'needs_review' | 'pending'

function statusBadgeClass(status: string) {
  switch (status) {
    case 'approved':
      return 'bg-emerald-100 text-emerald-700 border-transparent'
    case 'rejected':
      return 'bg-red-100 text-red-700 border-transparent'
    case 'needs_review':
      return 'bg-amber-100 text-amber-700 border-transparent'
    default:
      return 'bg-secondary text-muted-foreground border-transparent'
  }
}

function statusLabel(status: string) {
  switch (status) {
    case 'approved':
      return 'Approved'
    case 'rejected':
      return 'Rejected'
    case 'needs_review':
      return 'Needs Review'
    default:
      return 'Pending'
  }
}

function confidenceBadgeClass(confidence: string) {
  switch (confidence) {
    case 'high':
      return 'bg-emerald-50 text-emerald-700 border-emerald-200'
    case 'medium':
      return 'bg-amber-50 text-amber-700 border-amber-200'
    default:
      return 'bg-secondary text-muted-foreground border-transparent'
  }
}

function useReviewLookup(reviews: StrategyReview[]) {
  return useMemo(() => {
    const map = new Map<string, StrategyReview>()
    for (const r of reviews) {
      map.set(`${r.recommendation_type}:${r.recommendation_id}`, r)
    }
    return map
  }, [reviews])
}

function useTaskStatusLookup(taskStatuses: RecommendationTaskStatus[]) {
  return useMemo(() => {
    const map = new Map<string, RecommendationTaskStatus>()
    for (const t of taskStatuses) {
      map.set(`${t.recommendation_type}:${t.recommendation_id}`, t)
    }
    return map
  }, [taskStatuses])
}

export function CannibalizationReview({ projectId }: Props) {
  const {
    data: strategyWithReviews,
    isLoading,
    error: queryError,
    refetch,
  } = useQuery(
    `cannibalization-strategy-${projectId}`,
    () => getCannibalizationStrategy(projectId),
    { enabled: !!projectId },
  )

  const { showError, showSuccess } = useErrorHandler()

  useEffect(() => {
    if (queryError) {
      console.error('[cannibalization] strategy fetch failed:', queryError)
      showError(queryError.message || 'Failed to load strategy')
    }
  }, [queryError, showError])

  const approveMutation = useMutation(
    (args: {
      strategyId: string
      projectId: string
      recommendationType: RecType
      recommendationId: string
      status: ApprovalStatus
    }) =>
      setRecommendationApproval({
        strategyId: args.strategyId,
        projectId: args.projectId,
        recommendationType: args.recommendationType,
        recommendationId: args.recommendationId,
        status: args.status,
      }),
    {
      onSuccess: () => refetch(),
      onError: (err: Error) => {
        console.error('[cannibalization] approve failed:', err)
        showError(err.message)
      },
    },
  )

  const createTasksMutation = useMutation(
    (args: { strategyId: string; projectId: string }) =>
      createTasksFromApprovedRecommendations(args.strategyId, args.projectId),
    {
      onSuccess: async (ids: string[], variables) => {
        creatingRef.current = false
        refetch()
        if (ids.length > 0) {
          const items = ids.map(id => ({
            task_id: id,
            project_id: variables.projectId,
            title: null as string | null,
            task_type: null as string | null,
            project_name: null as string | null,
          }))
          try {
            await enqueueTasks(items, 'append')
            showSuccess(`Created and enqueued ${ids.length} task${ids.length === 1 ? '' : 's'}`)
          } catch (enqueueErr) {
            console.error('[cannibalization] enqueue failed:', enqueueErr)
            showError(`Created ${ids.length} tasks but failed to enqueue them`)
          }
        } else {
          showError('No approved recommendations found to create tasks from')
        }
      },
      onError: (err: Error) => {
        creatingRef.current = false
        console.error('[cannibalization] create tasks failed:', err)
        showError(err.message)
      },
    },
  )

  const handleApprove = useCallback(
    (type: RecType, id: string, status: ApprovalStatus) => {
      if (!strategyWithReviews) return
      approveMutation.mutate({
        strategyId: strategyWithReviews.strategy_id,
        projectId: strategyWithReviews.project_id,
        recommendationType: type,
        recommendationId: id,
        status,
      })
    },
    [approveMutation, strategyWithReviews],
  )

  const creatingRef = useRef(false)

  const handleCreateTasks = useCallback(() => {
    if (!strategyWithReviews || createTasksMutation.isPending || creatingRef.current) return
    creatingRef.current = true
    createTasksMutation.mutate({
      strategyId: strategyWithReviews.strategy_id,
      projectId: strategyWithReviews.project_id,
    })
  }, [createTasksMutation, strategyWithReviews])

  const reviewLookup = useReviewLookup(strategyWithReviews?.reviews ?? [])
  const taskStatusLookup = useTaskStatusLookup(strategyWithReviews?.task_statuses ?? [])

  const approvedCount = useMemo(() => {
    return strategyWithReviews?.reviews.filter(r => r.approval_status === 'approved').length ?? 0
  }, [strategyWithReviews])

  const approvedWithoutTaskCount = useMemo(() => {
    if (!strategyWithReviews) return 0
    return strategyWithReviews.reviews.filter(r => {
      if (r.approval_status !== 'approved') return false
      const status = taskStatusLookup.get(`${r.recommendation_type}:${r.recommendation_id}`)
      // Count as "needs task" only if no task exists at all (regardless of status).
      // Once a task has been created for a recommendation, it should not be
      // recreated from this UI — the user manages retries from the task board.
      return !status?.task_status
    }).length
  }, [strategyWithReviews, taskStatusLookup])

  // Filter to hide approved recommendations that already have tasks.
  const [hideCompleted, setHideCompleted] = useState(true)

  const isCompleted = useCallback(
    (type: RecType, id: string) => {
      const review = reviewLookup.get(`${type}:${id}`)
      if (review?.approval_status !== 'approved') return false
      const status = taskStatusLookup.get(`${type}:${id}`)
      return !!status?.task_status
    },
    [reviewLookup, taskStatusLookup]
  )

  // Defensive dedup: the backend reducer deduplicates by cluster_id, but stale
  // strategy files or old audit runs may still contain duplicates. Deduplicating
  // here prevents "click one, all ticked" UI bugs where multiple cards share
  // the same review lookup key.
  const strategy = useMemo(() => {
    const rawStrategy = strategyWithReviews?.strategy
    if (!rawStrategy) return null
    const seen = new Set<string>()
    return {
      ...rawStrategy,
      merge_recommendations: rawStrategy.merge_recommendations.filter(rec => {
        if (seen.has(rec.cluster_id)) return false
        seen.add(rec.cluster_id)
        return true
      }),
    }
  }, [strategyWithReviews])

  const totalRecs =
    (strategy?.merge_recommendations.length ?? 0) +
    (strategy?.hub_recommendations.length ?? 0) +
    (strategy?.territory_recommendations.length ?? 0) +
    (strategy?.calculator_recommendations.length ?? 0)

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="animate-spin text-muted-foreground" size={24} />
      </div>
    )
  }

  if (!strategyWithReviews || !strategy) {
    return (
      <div className="flex flex-col items-center justify-center h-full gap-4 text-muted-foreground">
        <AlertCircle size={32} />
        <p className="text-sm">No cannibalization strategy found.</p>
        <p className="text-xs">Run a cannibalization audit to generate a strategy.</p>
      </div>
    )
  }

  const { strategy_id } = strategyWithReviews

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="px-6 py-4 border-b border-border flex items-center justify-between shrink-0">
        <div>
          <h2 className="text-base font-semibold">Cannibalization Review</h2>
          <p className="text-xs text-muted-foreground mt-0.5">
            Strategy {strategy_id} · {approvedCount} / {totalRecs} approved
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            onClick={() => setHideCompleted(v => !v)}
            title={hideCompleted ? 'Show completed recommendations' : 'Hide completed recommendations'}
          >
            {hideCompleted ? (
              <Eye size={14} className="mr-1.5" />
            ) : (
              <EyeOff size={14} className="mr-1.5" />
            )}
            {hideCompleted ? 'Show Completed' : 'Hide Completed'}
          </Button>
          <Button
            size="sm"
            variant="outline"
            onClick={handleCreateTasks}
            disabled={createTasksMutation.isPending || approvedWithoutTaskCount === 0}
            title={
              approvedWithoutTaskCount === 0 && approvedCount > 0
                ? 'All approved recommendations already have tasks'
                : 'Legacy flow — prefer the task-drawer picker'
            }
          >
            {createTasksMutation.isPending ? (
              <Loader2 className="animate-spin mr-1.5" size={14} />
            ) : (
              <Play className="mr-1.5" size={14} />
            )}
            {approvedWithoutTaskCount === 0 && approvedCount > 0
              ? 'All Tasks Created'
              : `Create from Approved (${approvedWithoutTaskCount})`}
          </Button>
        </div>
      </div>

      {/* New-primary-flow banner */}
      <div className="mx-6 mt-4 shrink-0">
        <div className="flex items-start gap-2.5 rounded-md border border-border bg-secondary/40 px-3 py-2.5 text-xs text-muted-foreground">
          <Info size={14} className="mt-0.5 shrink-0 text-foreground" />
          <div>
            <span className="font-medium text-foreground">New workflow:</span>{' '}
            Open the latest <span className="font-medium">cannibalization_audit</span> task in the task board,
            review recommendations in the drawer, and create follow-up tasks directly from there.
            This page is now a read-only strategy browser.
          </div>
        </div>
      </div>

      <div className="flex-1 min-h-0">
        <ScrollArea className="h-full">
          <div className="p-6">
            <Tabs defaultValue="merge">
            <TabsList className="mb-4">
              <TabsTrigger value="merge">
                <GitMerge size={14} className="mr-1.5" />
                Merges ({strategy?.merge_recommendations.length ?? 0})
              </TabsTrigger>
              <TabsTrigger value="hub">
                <BookOpen size={14} className="mr-1.5" />
                Hubs ({strategy.hub_recommendations.length})
              </TabsTrigger>
              <TabsTrigger value="territory">
                <MapPin size={14} className="mr-1.5" />
                Territories ({strategy.territory_recommendations.length})
              </TabsTrigger>
              <TabsTrigger value="calculator">
                <Calculator size={14} className="mr-1.5" />
                Calculators ({strategy.calculator_recommendations.length})
              </TabsTrigger>
            </TabsList>

            <TabsContent value="merge">
              <div className="space-y-3">
                {strategy.merge_recommendations
                  .filter(rec => !hideCompleted || !isCompleted('merge', rec.cluster_id))
                  .map((rec, i) => (
                    <MergeCard
                      key={`${rec.cluster_id}-${i}`}
                      rec={rec}
                      review={reviewLookup.get(`merge:${rec.cluster_id}`)}
                      taskStatus={taskStatusLookup.get(`merge:${rec.cluster_id}`)}
                      onApprove={status => handleApprove('merge', rec.cluster_id, status)}
                      isPending={approveMutation.isPending}
                    />
                  ))}
                {strategy.merge_recommendations.filter(rec => !hideCompleted || !isCompleted('merge', rec.cluster_id)).length === 0 && (
                  <p className="text-sm text-muted-foreground">
                    {hideCompleted
                      ? 'No pending merge recommendations. Toggle "Show Completed" to see finished ones.'
                      : 'No merge recommendations.'}
                  </p>
                )}
              </div>
            </TabsContent>

            <TabsContent value="hub">
              <div className="space-y-3">
                {strategy.hub_recommendations
                  .filter(rec => !hideCompleted || !isCompleted('hub', rec.topic))
                  .map(rec => (
                    <HubCard
                      key={rec.topic}
                      rec={rec}
                      review={reviewLookup.get(`hub:${rec.topic}`)}
                      taskStatus={taskStatusLookup.get(`hub:${rec.topic}`)}
                      onApprove={status => handleApprove('hub', rec.topic, status)}
                      isPending={approveMutation.isPending}
                    />
                  ))}
                {strategy.hub_recommendations.filter(rec => !hideCompleted || !isCompleted('hub', rec.topic)).length === 0 && (
                  <p className="text-sm text-muted-foreground">
                    {hideCompleted
                      ? 'No pending hub recommendations. Toggle "Show Completed" to see finished ones.'
                      : 'No hub recommendations.'}
                  </p>
                )}
              </div>
            </TabsContent>

            <TabsContent value="territory">
              <div className="space-y-3">
                {strategy.territory_recommendations
                  .filter(rec => !hideCompleted || !isCompleted('territory', rec.theme))
                  .map(rec => (
                    <TerritoryCard
                      key={rec.theme}
                      rec={rec}
                      review={reviewLookup.get(`territory:${rec.theme}`)}
                      taskStatus={taskStatusLookup.get(`territory:${rec.theme}`)}
                      onApprove={status => handleApprove('territory', rec.theme, status)}
                      isPending={approveMutation.isPending}
                    />
                  ))}
                {strategy.territory_recommendations.filter(rec => !hideCompleted || !isCompleted('territory', rec.theme)).length === 0 && (
                  <p className="text-sm text-muted-foreground">
                    {hideCompleted
                      ? 'No pending territory recommendations. Toggle "Show Completed" to see finished ones.'
                      : 'No territory recommendations.'}
                  </p>
                )}
              </div>
            </TabsContent>

            <TabsContent value="calculator">
              <div className="space-y-3">
                {strategy.calculator_recommendations
                  .filter(rec => !hideCompleted || !isCompleted('calculator', rec.strategy))
                  .map(rec => (
                    <CalculatorCard
                      key={rec.strategy}
                      rec={rec}
                      review={reviewLookup.get(`calculator:${rec.strategy}`)}
                      taskStatus={taskStatusLookup.get(`calculator:${rec.strategy}`)}
                      onApprove={status => handleApprove('calculator', rec.strategy, status)}
                      isPending={approveMutation.isPending}
                    />
                  ))}
                {strategy.calculator_recommendations.filter(rec => !hideCompleted || !isCompleted('calculator', rec.strategy)).length === 0 && (
                  <p className="text-sm text-muted-foreground">
                    {hideCompleted
                      ? 'No pending calculator recommendations. Toggle "Show Completed" to see finished ones.'
                      : 'No calculator recommendations.'}
                  </p>
                )}
              </div>
            </TabsContent>
          </Tabs>
        </div>
      </ScrollArea>
    </div>
  </div>
  )
}

// ─── Merge Card ───────────────────────────────────────────────────────────────

function taskStatusBadgeClass(status: string | null | undefined) {
  if (!status) return ''
  switch (status) {
    case 'todo':
    case 'queued':
      return 'bg-blue-100 text-blue-700 border-transparent'
    case 'in_progress':
      return 'bg-purple-100 text-purple-700 border-transparent'
    case 'review':
      return 'bg-amber-100 text-amber-700 border-transparent'
    case 'done':
      return 'bg-emerald-100 text-emerald-700 border-transparent'
    case 'failed':
      return 'bg-red-100 text-red-700 border-transparent'
    case 'cancelled':
      return 'bg-gray-100 text-gray-700 border-transparent'
    default:
      return 'bg-secondary text-muted-foreground border-transparent'
  }
}

function taskStatusLabel(status: string | null | undefined) {
  if (!status) return ''
  switch (status) {
    case 'todo':
    case 'queued':
      return 'Task queued'
    case 'in_progress':
      return 'In progress'
    case 'review':
      return 'Under review'
    case 'done':
      return 'Completed'
    case 'failed':
      return 'Failed'
    case 'cancelled':
      return 'Cancelled'
    default:
      return status
  }
}

function MergeCard({
  rec,
  review,
  taskStatus,
  onApprove,
  isPending,
}: {
  rec: MergeRecommendation
  review?: StrategyReview
  taskStatus?: RecommendationTaskStatus
  onApprove: (status: ApprovalStatus) => void
  isPending: boolean
}) {
  const status = review?.approval_status ?? 'pending'

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="space-y-1">
            <CardTitle className="text-sm">{rec.cluster_id.replace(/_/g, ' ')}</CardTitle>
            <CardDescription className="text-xs">
              Keeper: <span className="font-medium text-foreground">{rec.keep_url}</span>
            </CardDescription>
          </div>
          <div className="flex items-center gap-2 flex-wrap">
            <Badge variant="outline" className={cn('text-xs', confidenceBadgeClass(rec.confidence))}>
              {rec.confidence} confidence
            </Badge>
            <Badge variant="outline" className={cn('text-xs', statusBadgeClass(status))}>
              {statusLabel(status)}
            </Badge>
            {taskStatus?.task_status && (
              <Badge variant="outline" className={cn('text-xs', taskStatusBadgeClass(taskStatus.task_status))}>
                {taskStatusLabel(taskStatus.task_status)}
              </Badge>
            )}
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="text-xs space-y-1">
          <div className="text-muted-foreground">
            Redirect to:{' '}
            {rec.redirect_urls.map((url, i) => (
              <span key={url} className="text-foreground">
                {url}
                {i < rec.redirect_urls.length - 1 ? ', ' : ''}
              </span>
            ))}
          </div>
          {rec.merge_instructions.length > 0 && (
            <ul className="list-disc list-inside text-muted-foreground mt-1">
              {rec.merge_instructions.map((inst, i) => (
                <li key={i}>{inst}</li>
              ))}
            </ul>
          )}
          <p className="text-muted-foreground mt-1">{rec.reason}</p>
        </div>
        <Separator />
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant={status === 'approved' ? 'default' : 'outline'}
            onClick={() => onApprove('approved')}
            disabled={isPending}
          >
            <Check size={14} className="mr-1" />
            Approve
          </Button>
          <Button
            size="sm"
            variant={status === 'rejected' ? 'default' : 'outline'}
            onClick={() => onApprove('rejected')}
            disabled={isPending}
          >
            <X size={14} className="mr-1" />
            Reject
          </Button>
          <Button
            size="sm"
            variant={status === 'needs_review' ? 'default' : 'outline'}
            onClick={() => onApprove('needs_review')}
            disabled={isPending}
          >
            <AlertCircle size={14} className="mr-1" />
            Needs Review
          </Button>
          {status !== 'pending' && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => onApprove('pending')}
              disabled={isPending}
              className="text-muted-foreground hover:text-foreground"
            >
              <RotateCcw size={14} className="mr-1" />
              Clear
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  )
}

// ─── Hub Card ─────────────────────────────────────────────────────────────────

function HubCard({
  rec,
  review,
  taskStatus,
  onApprove,
  isPending,
}: {
  rec: HubRecommendation
  review?: StrategyReview
  taskStatus?: RecommendationTaskStatus
  onApprove: (status: ApprovalStatus) => void
  isPending: boolean
}) {
  const status = review?.approval_status ?? 'pending'

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="space-y-1">
            <CardTitle className="text-sm">{rec.suggested_title}</CardTitle>
            <CardDescription className="text-xs">
              {rec.suggested_url} · {rec.spoke_pages.length} spokes
            </CardDescription>
          </div>
          <div className="flex items-center gap-2">
            <Badge variant="outline" className={cn('text-xs', statusBadgeClass(status))}>
              {statusLabel(status)}
            </Badge>
            {taskStatus?.task_status && (
              <Badge variant="outline" className={cn('text-xs', taskStatusBadgeClass(taskStatus.task_status))}>
                {taskStatusLabel(taskStatus.task_status)}
              </Badge>
            )}
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="text-xs text-muted-foreground">
          <p>Topic: {rec.topic}</p>
          <p>Intent: {rec.intent}</p>
          {rec.outline.length > 0 && (
            <p className="mt-1">Outline: {rec.outline.join(' → ')}</p>
          )}
        </div>
        <Separator />
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant={status === 'approved' ? 'default' : 'outline'}
            onClick={() => onApprove('approved')}
            disabled={isPending}
          >
            <Check size={14} className="mr-1" />
            Approve
          </Button>
          <Button
            size="sm"
            variant={status === 'rejected' ? 'default' : 'outline'}
            onClick={() => onApprove('rejected')}
            disabled={isPending}
          >
            <X size={14} className="mr-1" />
            Reject
          </Button>
          <Button
            size="sm"
            variant={status === 'needs_review' ? 'default' : 'outline'}
            onClick={() => onApprove('needs_review')}
            disabled={isPending}
          >
            <AlertCircle size={14} className="mr-1" />
            Needs Review
          </Button>
          {status !== 'pending' && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => onApprove('pending')}
              disabled={isPending}
              className="text-muted-foreground hover:text-foreground"
            >
              <RotateCcw size={14} className="mr-1" />
              Clear
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  )
}

// ─── Calculator Card ──────────────────────────────────────────────────────────

function CalculatorCard({
  rec,
  review,
  taskStatus,
  onApprove,
  isPending,
}: {
  rec: CalculatorRecommendation
  review?: StrategyReview
  taskStatus?: RecommendationTaskStatus
  onApprove: (status: ApprovalStatus) => void
  isPending: boolean
}) {
  const status = review?.approval_status ?? 'pending'

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="space-y-1">
            <CardTitle className="text-sm">{rec.strategy}</CardTitle>
            <CardDescription className="text-xs">Universe: {rec.ticker_universe}</CardDescription>
          </div>
          <div className="flex items-center gap-2">
            <Badge variant="outline" className={cn('text-xs', statusBadgeClass(status))}>
              {statusLabel(status)}
            </Badge>
            {taskStatus?.task_status && (
              <Badge variant="outline" className={cn('text-xs', taskStatusBadgeClass(taskStatus.task_status))}>
                {taskStatusLabel(taskStatus.task_status)}
              </Badge>
            )}
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="text-xs text-muted-foreground">
          <p>Tickers: {rec.priority_tickers.join(', ')}</p>
          <p className="mt-1">{rec.reason}</p>
        </div>
        <Separator />
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant={status === 'approved' ? 'default' : 'outline'}
            onClick={() => onApprove('approved')}
            disabled={isPending}
          >
            <Check size={14} className="mr-1" />
            Approve
          </Button>
          <Button
            size="sm"
            variant={status === 'rejected' ? 'default' : 'outline'}
            onClick={() => onApprove('rejected')}
            disabled={isPending}
          >
            <X size={14} className="mr-1" />
            Reject
          </Button>
          <Button
            size="sm"
            variant={status === 'needs_review' ? 'default' : 'outline'}
            onClick={() => onApprove('needs_review')}
            disabled={isPending}
          >
            <AlertCircle size={14} className="mr-1" />
            Needs Review
          </Button>
          {status !== 'pending' && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => onApprove('pending')}
              disabled={isPending}
              className="text-muted-foreground hover:text-foreground"
            >
              <RotateCcw size={14} className="mr-1" />
              Clear
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  )
}

// ─── Territory Card ───────────────────────────────────────────────────────────

function TerritoryCard({
  rec,
  review,
  taskStatus,
  onApprove,
  isPending,
}: {
  rec: TerritoryRecommendation
  review?: StrategyReview
  taskStatus?: RecommendationTaskStatus
  onApprove: (status: ApprovalStatus) => void
  isPending: boolean
}) {
  const status = review?.approval_status ?? 'pending'

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-start justify-between">
          <div className="space-y-1">
            <CardTitle className="text-sm">{rec.theme}</CardTitle>
            <CardDescription className="text-xs">Priority: {rec.priority}</CardDescription>
          </div>
          <div className="flex items-center gap-2">
            <Badge variant="outline" className={cn('text-xs', statusBadgeClass(status))}>
              {statusLabel(status)}
            </Badge>
            {taskStatus?.task_status && (
              <Badge variant="outline" className={cn('text-xs', taskStatusBadgeClass(taskStatus.task_status))}>
                {taskStatusLabel(taskStatus.task_status)}
              </Badge>
            )}
          </div>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="text-xs text-muted-foreground">
          {rec.demand_evidence.length > 0 && (
            <p>Evidence: {rec.demand_evidence.join('; ')}</p>
          )}
          {rec.suggested_tasks.length > 0 && (
            <p className="mt-1">Suggested: {rec.suggested_tasks.join(', ')}</p>
          )}
        </div>
        <Separator />
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant={status === 'approved' ? 'default' : 'outline'}
            onClick={() => onApprove('approved')}
            disabled={isPending}
          >
            <Check size={14} className="mr-1" />
            Approve
          </Button>
          <Button
            size="sm"
            variant={status === 'rejected' ? 'default' : 'outline'}
            onClick={() => onApprove('rejected')}
            disabled={isPending}
          >
            <X size={14} className="mr-1" />
            Reject
          </Button>
          <Button
            size="sm"
            variant={status === 'needs_review' ? 'default' : 'outline'}
            onClick={() => onApprove('needs_review')}
            disabled={isPending}
          >
            <AlertCircle size={14} className="mr-1" />
            Needs Review
          </Button>
          {status !== 'pending' && (
            <Button
              size="sm"
              variant="ghost"
              onClick={() => onApprove('pending')}
              disabled={isPending}
              className="text-muted-foreground hover:text-foreground"
            >
              <RotateCcw size={14} className="mr-1" />
              Clear
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  )
}
