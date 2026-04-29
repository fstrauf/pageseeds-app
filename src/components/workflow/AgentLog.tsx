import { useState, useEffect, useCallback } from 'react'
import { RefreshCw, Wand2, Copy, Check } from 'lucide-react'
import { useErrorHandler } from '../../lib/toast-context'
import { listTaskArtifacts, listTasks } from '../../lib/tauri'
import { extractJsonArtifact } from '../../lib/artifacts'
import type { NormalizedArtifact, Task, TaskArtifact } from '../../lib/types'
import { ScrollArea } from '@/components/ui/scroll-area'
import { cn } from '../../lib/utils'

interface AgentLogProps {
  projectId: string
}

function ArtifactRow({ artifact, onNormalize }: {
  artifact: TaskArtifact
  onNormalize: (raw: string) => void
}) {
  const [copied, setCopied] = useState(false)

  async function copy() {
    if (!artifact.content) return
    await navigator.clipboard.writeText(artifact.content)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }

  const isRaw = artifact.source === 'agentic' || artifact.source === 'raw'
  const hasContent = !!artifact.content

  return (
    <div className="border border-border rounded overflow-hidden">
      <div className="flex items-center gap-2 px-3 py-2 bg-muted/30 border-b border-border">
        <span className="text-sm font-mono font-medium text-foreground">{artifact.key}</span>
        {artifact.type && (
          <span className="text-xs text-muted-foreground bg-muted px-1.5 py-0.5 rounded">{artifact.type}</span>
        )}
        {artifact.source && (
          <span className={cn(
            'text-xs px-1.5 py-0.5 rounded',
            artifact.source === 'agentic' ? 'bg-amber-100 text-amber-700' :
            artifact.source === 'deterministic' ? 'bg-blue-100 text-blue-700' :
            'bg-muted text-muted-foreground'
          )}>
            {artifact.source}
          </span>
        )}
        {artifact.path && (
          <span className="text-xs text-muted-foreground truncate ml-1 font-mono">{artifact.path}</span>
        )}
        <div className="ml-auto flex gap-1">
          {hasContent && isRaw && (
            <button
              className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground px-2 py-0.5 rounded border border-border hover:bg-muted/50 transition-colors"
              onClick={() => onNormalize(artifact.content!)}
              title="Normalize this output to JSON"
            >
              <Wand2 size={11} /> Normalize
            </button>
          )}
          {hasContent && (
            <button
              className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground px-2 py-0.5 rounded border border-border hover:bg-muted/50 transition-colors"
              onClick={copy}
            >
              {copied ? <Check size={11} className="text-green-500" /> : <Copy size={11} />}
              {copied ? 'Copied' : 'Copy'}
            </button>
          )}
        </div>
      </div>
      {hasContent && (
        <pre className="p-3 text-xs text-foreground/80 whitespace-pre-wrap font-mono max-h-48 overflow-y-auto leading-relaxed">
          {artifact.content}
        </pre>
      )}
      {!hasContent && (
        <p className="px-3 py-2 text-xs text-muted-foreground italic">No inline content — stored at path only.</p>
      )}
    </div>
  )
}

function NormalizeResult({ result, onDismiss }: { result: NormalizedArtifact; onDismiss: () => void }) {
  const [copied, setCopied] = useState(false)

  async function copy() {
    const text = result.json_artifact ? JSON.stringify(result.json_artifact, null, 2) : ''
    await navigator.clipboard.writeText(text)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }

  return (
    <div className="rounded border border-border overflow-hidden">
      <div className="flex items-center gap-2 px-3 py-2 bg-muted/30 border-b border-border">
        <span className="text-sm font-semibold text-foreground">Normalized Result</span>
        <span className={cn(
          'text-xs px-1.5 py-0.5 rounded',
          result.success ? 'bg-green-100 text-green-700' : 'bg-destructive/10 text-destructive',
        )}>
          {result.success ? result.extraction_method : 'none'}
        </span>
        {result.success && (
          <button
            className="ml-auto flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground px-2 py-0.5 rounded border border-border hover:bg-muted/50 transition-colors"
            onClick={copy}
          >
            {copied ? <Check size={11} className="text-green-500" /> : <Copy size={11} />}
            {copied ? 'Copied' : 'Copy JSON'}
          </button>
        )}
        <button
          className="text-xs text-muted-foreground hover:text-foreground px-1.5"
          onClick={onDismiss}
        >
          ✕
        </button>
      </div>
      {result.success && result.json_artifact !== null ? (
        <pre className="p-3 text-xs text-foreground/80 whitespace-pre-wrap font-mono max-h-64 overflow-y-auto leading-relaxed">
          {JSON.stringify(result.json_artifact, null, 2)}
        </pre>
      ) : (
        <p className="px-3 py-2 text-xs text-muted-foreground italic">No JSON structure found in the output.</p>
      )}
    </div>
  )
}

export function AgentLog({ projectId }: AgentLogProps) {
  const [tasks, setTasks] = useState<Task[]>([])
  const [selectedTask, setSelectedTask] = useState<string>('')
  const [artifacts, setArtifacts] = useState<TaskArtifact[]>([])
  const [loadingTasks, setLoadingTasks] = useState(false)
  const [loadingArtifacts, setLoadingArtifacts] = useState(false)
  const [normalizeResult, setNormalizeResult] = useState<NormalizedArtifact | null>(null)
  const { showError } = useErrorHandler()

  const loadTasks = useCallback(async () => {
    setLoadingTasks(true)
    try {
      const data = await listTasks(projectId)
      setTasks(data)
    } catch (e: unknown) {
      showError(String(e))
    } finally {
      setLoadingTasks(false)
    }
  }, [projectId, showError])

  useEffect(() => {
    if (projectId) loadTasks()
  }, [projectId, loadTasks])

  const loadArtifacts = useCallback(async (taskId: string) => {
    setLoadingArtifacts(true)
    setNormalizeResult(null)
    try {
      const data = await listTaskArtifacts(taskId)
      setArtifacts(data)
    } catch (e: unknown) {
      showError(String(e))
    } finally {
      setLoadingArtifacts(false)
    }
  }, [showError])

  useEffect(() => {
    if (selectedTask) loadArtifacts(selectedTask)
  }, [selectedTask, loadArtifacts])

  function handleNormalize(raw: string) {
    const result = extractJsonArtifact(raw)
    setNormalizeResult(result)
  }

  return (
    <ScrollArea className="h-full">
      <div className="p-6 flex flex-col gap-6">
        {/* Header */}
        <div className="flex items-start justify-between">
          <div>
            <h2 className="text-base font-semibold text-foreground mb-1">Agent Log</h2>
            <p className="text-xs text-muted-foreground">
              View raw agent output and normalize to structured JSON artifacts.
            </p>
          </div>
          <button
            onClick={loadTasks}
            className="text-muted-foreground hover:text-foreground transition-colors shrink-0"
            title="Refresh task list"
          >
            <RefreshCw size={14} className={cn(loadingTasks && 'animate-spin')} />
          </button>
        </div>

        {/* Task selector */}
        <div className="flex flex-col gap-1.5">
          <label className="text-xs font-medium text-foreground">Select Task</label>
          <select
            className="rounded border border-border bg-background px-3 py-1.5 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            value={selectedTask}
            onChange={e => setSelectedTask(e.target.value)}
            disabled={loadingTasks}
          >
            <option value="">— choose a task —</option>
            {tasks.map(t => (
              <option key={t.id} value={t.id}>
                [{t.status}] {t.type}
                {t.title ? ` — ${t.title}` : ''}
              </option>
            ))}
          </select>
        </div>

        {/* Artifacts */}
        {selectedTask && (
          <div className="flex flex-col gap-3">
            <div className="flex items-center justify-between">
              <p className="text-xs font-medium text-foreground">
                Artifacts {loadingArtifacts ? '(loading…)' : `(${artifacts.length})`}
              </p>
            </div>

            {!loadingArtifacts && artifacts.length === 0 && (
              <p className="text-sm text-muted-foreground">No artifacts stored for this task yet.</p>
            )}

            {artifacts.map((a, i) => (
              <ArtifactRow key={i} artifact={a} onNormalize={handleNormalize} />
            ))}

            {normalizeResult && (
              <NormalizeResult result={normalizeResult} onDismiss={() => setNormalizeResult(null)} />
            )}
          </div>
        )}
      </div>
    </ScrollArea>
  )
}
