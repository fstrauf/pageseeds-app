import { useEffect, useState } from 'react'
import { Check, ExternalLink, Plus } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { clarityGetSummary, createClarityTasksFromSelection } from '../../lib/tauri'
import type { ClarityFindingPayload, ClarityTaskCreationResult, Project, Task } from '../../lib/types'

interface Props {
  task: Task
  project?: Project
  onTasksCreated?: (tasks: Task[]) => void
  onClose?: () => void
}

export function ClarityFindingReview({ task, project, onTasksCreated, onClose }: Props) {
  const [findings, setFindings] = useState<ClarityFindingPayload[]>([])
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [loading, setLoading] = useState(false)
  const [creating, setCreating] = useState(false)
  const [createError, setCreateError] = useState<string | null>(null)
  const [result, setResult] = useState<ClarityTaskCreationResult | null>(null)

  useEffect(() => {
    if (!project) return
    setLoading(true)
    clarityGetSummary(project)
      .then(summary => {
        setFindings(summary?.top_findings ?? [])
      })
      .catch(console.error)
      .finally(() => setLoading(false))
  }, [project])

  const toggleFinding = (idx: number) => {
    setSelected(prev => {
      const next = new Set(prev)
      if (next.has(idx)) {
        next.delete(idx)
      } else {
        next.add(idx)
      }
      return next
    })
  }

  const handleCreateTasks = async () => {
    if (!project) return
    setCreating(true)
    setCreateError(null)
    try {
      const selectedFindings = Array.from(selected)
        .map(idx => findings[idx])
        .filter((f): f is ClarityFindingPayload => Boolean(f))

      const creationResult = await createClarityTasksFromSelection(task.id, selectedFindings)
      setResult(creationResult)
    } catch (e) {
      console.error('Failed to create tasks from Clarity findings', e)
      setCreateError(e instanceof Error ? e.message : String(e))
    } finally {
      setCreating(false)
    }
  }

  const handleDone = () => {
    onTasksCreated?.(result?.created_tasks ?? [])
    onClose?.()
  }

  if (loading) return <p className="text-sm text-muted-foreground">Loading findings…</p>
  if (findings.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        No findings available. The summary may not have been generated yet.
      </p>
    )
  }

  if (result) {
    return (
      <div className="space-y-3">
        <p className="text-sm">
          Created {result.created_tasks.length} fix task
          {result.created_tasks.length !== 1 ? 's' : ''}.
        </p>
        {result.skipped.length > 0 && (
          <div className="space-y-2">
            <p className="text-xs text-muted-foreground">
              {result.skipped.length} finding{result.skipped.length !== 1 ? 's were' : ' was'} skipped:
            </p>
            {result.skipped.map((skip, idx) => (
              <Card key={idx}>
                <CardHeader className="pb-2">
                  <CardTitle className="text-sm">{skip.issue_type}</CardTitle>
                  <CardDescription className="truncate">{skip.url}</CardDescription>
                </CardHeader>
                <CardContent>
                  <p className="text-xs text-muted-foreground">{skip.reason}</p>
                </CardContent>
              </Card>
            ))}
          </div>
        )}
        <Button onClick={handleDone} className="w-full">
          Done
        </Button>
      </div>
    )
  }

  return (
    <div className="space-y-3">
      <p className="text-xs text-muted-foreground">
        Select findings to create follow-up tasks, then click "Create Tasks".
      </p>

      {findings.map((finding, idx) => (
        <Card
          key={idx}
          className={`cursor-pointer transition-colors ${selected.has(idx) ? 'border-primary bg-primary/5' : 'hover:bg-secondary/30'}`}
          onClick={() => toggleFinding(idx)}
        >
          <CardHeader className="pb-2">
            <div className="flex items-start gap-2">
              <div
                className={`mt-0.5 flex h-4 w-4 shrink-0 items-center justify-center rounded border ${selected.has(idx) ? 'border-primary bg-primary text-primary-foreground' : 'border-border'}`}
              >
                {selected.has(idx) && <Check size={10} strokeWidth={3} />}
              </div>
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <CardTitle className="text-sm">{finding.issue_type}</CardTitle>
                  <Badge
                    variant={
                      finding.severity === 'high'
                        ? 'destructive'
                        : finding.severity === 'medium'
                        ? 'default'
                        : 'secondary'
                    }
                  >
                    {finding.severity}
                  </Badge>
                </div>
                <CardDescription className="truncate">{finding.url}</CardDescription>
              </div>
              <Button
                variant="ghost"
                size="sm"
                asChild
                onClick={e => e.stopPropagation()}
              >
                <a
                  href={finding.clarity_dashboard_url}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  <ExternalLink className="h-3 w-3" />
                </a>
              </Button>
            </div>
          </CardHeader>
          <CardContent className="space-y-1 pl-10">
            <p className="text-xs">
              <span className="font-medium">Evidence:</span> {finding.evidence}
            </p>
            <p className="text-xs">
              <span className="font-medium">Recommendation:</span> {finding.recommendation}
            </p>
          </CardContent>
        </Card>
      ))}

      {createError && (
        <p className="text-sm text-destructive" role="alert">
          {createError}
        </p>
      )}

      <Button
        onClick={handleCreateTasks}
        disabled={creating || selected.size === 0}
        className="w-full"
      >
        <Plus className="h-4 w-4 mr-1" />
        {creating ? 'Creating…' : `Create Tasks (${selected.size})`}
      </Button>
    </div>
  )
}
