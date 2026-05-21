import { useState, useCallback } from 'react'
import { Loader2, Send, Sparkles, ChevronDown, ChevronRight } from 'lucide-react'
import { investigate } from '@/lib/tauri'
import type { InvestigationResult } from '@/lib/types'
import { Button } from '@/components/ui/button'
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card'
import { Badge } from '@/components/ui/badge'
import { useErrorHandler } from '@/lib/toast-context'

interface Props {
  projectId: string
}

export function InvestigationPanel({ projectId }: Props) {
  const handleError = useErrorHandler()
  const [question, setQuestion] = useState('')
  const [running, setRunning] = useState(false)
  const [results, setResults] = useState<InvestigationResult[]>([])
  const [expandedId, setExpandedId] = useState<string | null>(null)

  const handleInvestigate = useCallback(async () => {
    if (!question.trim() || running || !projectId) return
    setRunning(true)
    try {
      const result = await investigate(projectId, question.trim())
      setResults((prev) => [result, ...prev])
      setQuestion('')
    } catch (e: unknown) {
      handleError.showError(String(e))
    } finally {
      setRunning(false)
    }
  }, [question, running, projectId, handleError])

  return (
    <Card className="border-border">
      <CardHeader className="pb-2">
        <CardTitle className="text-sm font-semibold flex items-center gap-2">
          <Sparkles size={15} className="text-primary" />
          Ask AI about your site
        </CardTitle>
      </CardHeader>
      <CardContent className="pt-0">
        {/* Input */}
        <div className="flex gap-2 mb-3">
          <textarea
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            placeholder="e.g. Why are my impressions plateauing?"
            className="flex-1 min-h-[60px] resize-none rounded-md border border-input bg-background px-3 py-2 text-sm placeholder:text-muted-foreground focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
            onKeyDown={(e) => {
              if (e.key === 'Enter' && !e.shiftKey) {
                e.preventDefault()
                handleInvestigate()
              }
            }}
          />
          <Button
            size="sm"
            onClick={handleInvestigate}
            disabled={running || !question.trim()}
            className="shrink-0 self-end gap-1.5"
          >
            {running ? <Loader2 size={14} className="animate-spin" /> : <Send size={14} />}
            Ask
          </Button>
        </div>

        {/* Results */}
        {results.map((r) => (
          <InvestigationResultCard
            key={r.id}
            result={r}
            expanded={expandedId === r.id}
            onToggle={() => setExpandedId(expandedId === r.id ? null : r.id)}
          />
        ))}

        {results.length === 0 && !running && (
          <p className="text-xs text-muted-foreground text-center py-4">
            Ask a question about your site's performance, content, or SEO health.
            The AI will explore your data and return specific findings.
          </p>
        )}
      </CardContent>
    </Card>
  )
}

function InvestigationResultCard({
  result,
  expanded,
  onToggle,
}: {
  result: InvestigationResult
  expanded: boolean
  onToggle: () => void
}) {
  const severityColor = (s: string) =>
    s === 'critical' ? 'destructive' : s === 'warning' ? 'default' : 'secondary'

  return (
    <div className="mb-2 border border-border rounded-md overflow-hidden">
      <button
        onClick={onToggle}
        className="w-full flex items-center justify-between px-3 py-2 text-left hover:bg-secondary/50 transition-colors"
      >
        <div className="min-w-0 flex-1">
          <p className="text-xs font-medium text-foreground truncate">
            Q: {result.question}
          </p>
          <p className="text-[10px] text-muted-foreground mt-0.5 line-clamp-1">
            {result.summary || result.answer.slice(0, 100)}
          </p>
        </div>
        <div className="flex items-center gap-2 ml-2 shrink-0">
          {result.findings.length > 0 && (
            <Badge variant="outline" className="text-[10px] px-1 py-0 h-auto">
              {result.findings.length} findings
            </Badge>
          )}
          {expanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
        </div>
      </button>

      {expanded && (
        <div className="px-3 pb-3 border-t border-border">
          <p className="text-xs text-foreground whitespace-pre-wrap mt-3 mb-3 leading-relaxed">
            {result.answer}
          </p>

          {result.findings.map((f, i) => (
            <div
              key={i}
              className="mb-2 p-2 rounded-md bg-secondary/40 border border-border"
            >
              <div className="flex items-center gap-2 mb-1">
                <Badge variant={severityColor(f.severity)} className="text-[10px] px-1 py-0 h-auto capitalize">
                  {f.severity}
                </Badge>
                <span className="text-xs font-medium">{f.title}</span>
              </div>
              <p className="text-[10px] text-muted-foreground">{f.description}</p>
              {f.fix_type && (
                <Badge variant="secondary" className="text-[10px] px-1 py-0 h-auto mt-1 capitalize">
                  {f.fix_type.replace(/_/g, ' ')}
                </Badge>
              )}
            </div>
          ))}

          <p className="text-[10px] text-muted-foreground mt-2">
            {new Date(result.created_at).toLocaleString()}
          </p>
        </div>
      )}
    </div>
  )
}
