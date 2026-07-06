import { useEffect, useState } from 'react'
import { Check, ExternalLink, Plus } from 'lucide-react'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { clarityGetSummary, createTask } from '../../lib/tauri'
import type { ClarityFindingPayload, Project, Task } from '../../lib/types'

interface Props {
  task: Task
  project?: Project
  onTasksCreated?: (tasks: Task[]) => void
  onClose?: () => void
}

export function ClarityFindingReview({ project, onTasksCreated, onClose }: Props) {
  const [findings, setFindings] = useState<ClarityFindingPayload[]>([])
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [loading, setLoading] = useState(false)
  const [creating, setCreating] = useState(false)

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
    try {
      const newTasks: Task[] = []
      for (const idx of Array.from(selected)) {
        const finding = findings[idx]
        if (!finding) continue

        const taskType =
          finding.issue_type === 'Quickback bounces' || finding.issue_type === 'Low engagement'
            ? 'fix_content_article'
            : finding.issue_type === 'Rage clicks' || finding.issue_type === 'Dead clicks'
            ? 'create_landing_page'
            : 'write_article'

        const newTask = await createTask(
          project.id,
          taskType,
          `${finding.issue_type}: ${finding.url}`,
          `From Clarity investigation: ${finding.evidence}\n\nRecommendation: ${finding.recommendation}\n\nDashboard: ${finding.clarity_dashboard_url}`,
          finding.severity === 'high' ? 'high' : 'medium',
        )
        newTasks.push(newTask)
      }
      onTasksCreated?.(newTasks)
      onClose?.()
    } catch (e) {
      console.error('Failed to create tasks from Clarity findings', e)
    } finally {
      setCreating(false)
    }
  }

  if (loading) return <p className="text-sm text-muted-foreground">Loading findings…</p>
  if (findings.length === 0) {
    return (
      <p className="text-sm text-muted-foreground">
        No findings available. The summary may not have been generated yet.
      </p>
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
