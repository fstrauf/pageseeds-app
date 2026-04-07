import React, { useCallback, useEffect, useState } from 'react'
import { PieChart, RefreshCw, Target, FolderOpen, AlertCircle, CheckCircle2 } from 'lucide-react'
import { getKeywordCoverage, createTask } from '../../lib/tauri'
import type { Project, KeywordCoverage, KeywordCoverageCluster } from '../../lib/types'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'
import { cn } from '../../lib/utils'

const AUTHORITY_BADGE: Record<string, string> = {
  Strong: 'bg-emerald-100 text-emerald-700 border-transparent',
  Moderate: 'bg-sky-100 text-sky-700 border-transparent',
  Weak: 'bg-amber-100 text-amber-700 border-transparent',
  Minimal: 'bg-red-100 text-red-700 border-transparent',
}

interface KeywordCoverageProps {
  project: Project | null
  onRunTasks?: (tasks: { id: string; title?: string; type: string }[]) => void
}

export function KeywordCoveragePanel({ project, onRunTasks }: KeywordCoverageProps) {
  const [coverage, setCoverage] = useState<KeywordCoverage | null>(null)
  const [lastAnalyzed, setLastAnalyzed] = useState<string>('Never analyzed')
  const [exists, setExists] = useState(false)
  const [loading, setLoading] = useState(false)
  const [creating, setCreating] = useState(false)

  const load = useCallback(async () => {
    if (!project) return
    setLoading(true)
    try {
      const status = await getKeywordCoverage(project.id)
      setExists(status.exists)
      setLastAnalyzed(status.last_analyzed)
      if (status.coverage) {
        setCoverage(status.coverage)
      }
    } catch (e) {
      console.error('Failed to load keyword coverage:', e)
    } finally {
      setLoading(false)
    }
  }, [project])

  useEffect(() => {
    load()
  }, [load])

  async function handleAnalyze() {
    if (!project || creating) return
    setCreating(true)
    try {
      const task = await createTask(
        project.id,
        'analyze_keyword_coverage',
        `Analyze keyword coverage — ${new Date().toLocaleDateString()}`,
        undefined,
        'medium',
      )
      onRunTasks?.([{ id: task.id, title: task.title ?? undefined, type: task.type }])
      // Poll for completion
      setTimeout(() => load(), 2000)
    } catch (e) {
      console.error('Failed to create coverage task:', e)
    } finally {
      setCreating(false)
    }
  }

  if (!project) {
    return (
      <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
        Select a project to see keyword coverage.
      </div>
    )
  }

  const hasCoverage = exists && coverage && coverage.clusters.length > 0

  return (
    <div className="flex flex-col h-full overflow-y-auto bg-background">
      <div className="p-6 space-y-6 pb-20">
        {/* Header */}
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-base font-semibold text-foreground flex items-center gap-2">
              <PieChart size={18} className="text-muted-foreground" />
              Keyword Coverage
            </h1>
            <div className="text-xs text-muted-foreground mt-0.5">
              Semantic clustering of your content portfolio
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Button variant="ghost" size="icon-sm" onClick={load} disabled={loading} className="text-muted-foreground">
              <RefreshCw size={13} className={loading ? 'animate-spin' : ''} />
            </Button>
          </div>
        </div>

        {/* Status Card */}
        <Card className="bg-card border-border">
          <CardContent className="pt-4 pb-3 px-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-xs text-muted-foreground mb-1">Last Analyzed</div>
                <div className="text-lg font-semibold text-foreground flex items-center gap-2">
                  {exists ? (
                    <>
                      <CheckCircle2 size={16} className="text-emerald-600" />
                      {lastAnalyzed}
                    </>
                  ) : (
                    <>
                      <AlertCircle size={16} className="text-amber-600" />
                      {lastAnalyzed}
                    </>
                  )}
                </div>
              </div>
              <Button
                onClick={handleAnalyze}
                disabled={creating}
                size="sm"
                className="gap-1.5"
              >
                {creating ? (
                  <RefreshCw size={13} className="animate-spin" />
                ) : (
                  <Target size={13} />
                )}
                {exists ? 'Re-analyze' : 'Analyze Now'}
              </Button>
            </div>
          </CardContent>
        </Card>

        {/* Coverage Summary */}
        {hasCoverage && (
          <Card className="bg-card border-border">
            <CardHeader className="pb-2">
              <CardTitle className="text-sm font-semibold text-foreground flex items-center gap-1.5">
                <FolderOpen size={13} className="text-muted-foreground" />
                Coverage Summary
              </CardTitle>
            </CardHeader>
            <CardContent className="pb-4">
              <div className="grid grid-cols-3 gap-4 mb-4">
                <div className="text-center">
                  <div className="text-2xl font-bold text-foreground">{coverage.article_count}</div>
                  <div className="text-xs text-muted-foreground">Total Articles</div>
                </div>
                <div className="text-center">
                  <div className="text-2xl font-bold text-foreground">{coverage.clusters.length}</div>
                  <div className="text-xs text-muted-foreground">Topic Clusters</div>
                </div>
                <div className="text-center">
                  <div className="text-2xl font-bold text-foreground">
                    {Math.round(coverage.article_count / coverage.clusters.length)}
                  </div>
                  <div className="text-xs text-muted-foreground">Avg per Cluster</div>
                </div>
              </div>
              
              {/* Authority Level Distribution */}
              {coverage.clusters.some(c => c.authority_level) && (
                <>
                  <Separator className="my-3" />
                  <div className="text-xs text-muted-foreground mb-2">Authority Distribution</div>
                  <div className="flex flex-wrap gap-2">
                    {['Strong', 'Moderate', 'Weak', 'Minimal'].map(level => {
                      const count = coverage.clusters.filter(c => c.authority_level === level).length
                      if (count === 0) return null
                      return (
                        <Badge key={level} className={cn('text-xs', AUTHORITY_BADGE[level])}>
                          {level}: {count}
                        </Badge>
                      )
                    })}
                  </div>
                </>
              )}
            </CardContent>
          </Card>
        )}

        {/* Clusters */}
        {hasCoverage ? (
          <div className="space-y-3">
            <h3 className="text-sm font-medium text-foreground">Topic Clusters</h3>
            {coverage.clusters.map((cluster) => (
              <ClusterCard key={cluster.cluster_id} cluster={cluster} />
            ))}
          </div>
        ) : exists ? (
          <Card className="bg-card border-border">
            <CardContent className="py-8 text-center">
              <AlertCircle size={32} className="text-muted-foreground mx-auto mb-3 opacity-50" />
              <p className="text-sm text-muted-foreground">
                No coverage data available. The analysis may have failed or found no articles.
              </p>
              <Button onClick={handleAnalyze} disabled={creating} variant="outline" size="sm" className="mt-4">
                {creating ? <RefreshCw size={13} className="animate-spin mr-1.5" /> : <RefreshCw size={13} className="mr-1.5" />}
                Try Again
              </Button>
            </CardContent>
          </Card>
        ) : (
          <Card className="bg-card border-border">
            <CardContent className="py-8 text-center">
              <PieChart size={32} className="text-muted-foreground mx-auto mb-3 opacity-50" />
              <p className="text-sm text-muted-foreground mb-1">
                No coverage analysis yet.
              </p>
              <p className="text-xs text-muted-foreground mb-4">
                Run an analysis to see how your articles are clustered by topic.
              </p>
              <Button onClick={handleAnalyze} disabled={creating} size="sm">
                {creating ? <RefreshCw size={13} className="animate-spin mr-1.5" /> : <Target size={13} className="mr-1.5" />}
                Analyze Coverage
              </Button>
            </CardContent>
          </Card>
        )}
      </div>
    </div>
  )
}

function ClusterCard({ cluster }: { cluster: KeywordCoverageCluster }) {
  const hasAuthority = cluster.authority_score !== undefined

  return (
    <Card className="bg-card border-border">
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="text-sm font-semibold text-foreground">
            {cluster.cluster_name}
          </CardTitle>
          <div className="flex items-center gap-2">
            {hasAuthority && (
              <Badge 
                className={cn('text-xs', AUTHORITY_BADGE[cluster.authority_level ?? ''] ?? 'bg-secondary text-secondary-foreground')}
                title={cluster.authority_description}
              >
                {cluster.authority_level} ({cluster.authority_score})
              </Badge>
            )}
            <Badge variant="secondary" className="text-xs">
              {cluster.article_count} articles
            </Badge>
          </div>
        </div>
      </CardHeader>
      <CardContent className="pb-4">
        {hasAuthority && (
          <div className="mb-3 p-2 bg-secondary/40 rounded-md">
            <div className="flex items-center justify-between text-xs">
              <span className="text-muted-foreground">Recommended Action:</span>
              <span className="font-medium text-foreground">{cluster.recommended_action}</span>
            </div>
            {cluster.avg_position !== undefined && (
              <div className="flex items-center justify-between text-xs mt-1">
                <span className="text-muted-foreground">Avg Position:</span>
                <span className="font-medium text-foreground">
                  {cluster.avg_position.toFixed(1)}
                  {cluster.total_impressions !== undefined && (
                    <span className="text-muted-foreground ml-2">
                      ({cluster.total_impressions.toLocaleString()} impressions)
                    </span>
                  )}
                </span>
              </div>
            )}
          </div>
        )}
        <div className="flex flex-wrap gap-1.5 mb-3">
          {cluster.primary_keywords.map((keyword) => (
            <Badge key={keyword} variant="outline" className="text-xs font-normal">
              {keyword}
            </Badge>
          ))}
        </div>
        <Separator className="my-2" />
        <div className="text-xs text-muted-foreground">
          Article IDs: {cluster.article_ids.join(', ')}
        </div>
      </CardContent>
    </Card>
  )
}
