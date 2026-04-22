import { useState, useEffect, useCallback } from 'react'
import { Eye, Copy, Check } from 'lucide-react'
import { listTasks, listSkills, buildPromptPreview } from '../../lib/tauri'
import type { PromptContext, Skill, Task } from '../../lib/types'
import { ScrollArea } from '@/components/ui/scroll-area'
import { cn } from '../../lib/utils'

interface PromptPreviewProps {
  projectId: string
}

export function PromptPreview({ projectId }: PromptPreviewProps) {
  const [tasks, setTasks] = useState<Task[]>([])
  const [skills, setSkills] = useState<Skill[]>([])
  const [selectedTask, setSelectedTask] = useState('')
  const [selectedSkill, setSelectedSkill] = useState('')
  const [loading, setLoading] = useState(false)
  const [loadingInit, setLoadingInit] = useState(false)
  const [context, setContext] = useState<PromptContext | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)
  const [activeSection, setActiveSection] = useState<number | null>(null)

  const init = useCallback(async () => {
    setLoadingInit(true)
    setError(null)
    try {
      const [t, s] = await Promise.all([listTasks(projectId), listSkills(projectId)])
      setTasks(t)
      setSkills(s)
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setLoadingInit(false)
    }
  }, [projectId])

  useEffect(() => {
    if (projectId) init()
  }, [projectId, init])

  async function build() {
    if (!selectedTask || !selectedSkill) return
    setLoading(true)
    setError(null)
    setContext(null)
    try {
      const ctx = await buildPromptPreview(selectedTask, selectedSkill)
      setContext(ctx)
      setActiveSection(null)
    } catch (e: unknown) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  async function copyPrompt() {
    if (!context) return
    await navigator.clipboard.writeText(context.prompt)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }

  return (
    <ScrollArea className="h-full">
      <div className="p-6 flex flex-col gap-6">
        {/* Header */}
        <div>
          <h2 className="text-base font-semibold text-foreground mb-1">Prompt Preview</h2>
          <p className="text-xs text-muted-foreground">
            Build the complete agent prompt for a task + skill combination before execution.
          </p>
        </div>

        {/* Controls */}
        <div className="flex flex-col gap-3">
          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-medium text-foreground">Task</label>
            <select
              className="rounded border border-border bg-background px-3 py-1.5 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-ring disabled:opacity-50"
              value={selectedTask}
              onChange={e => setSelectedTask(e.target.value)}
              disabled={loadingInit}
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

          <div className="flex flex-col gap-1.5">
            <label className="text-xs font-medium text-foreground">Skill</label>
            <select
              className="rounded border border-border bg-background px-3 py-1.5 text-sm text-foreground focus:outline-none focus:ring-1 focus:ring-ring disabled:opacity-50"
              value={selectedSkill}
              onChange={e => setSelectedSkill(e.target.value)}
              disabled={loadingInit}
            >
              <option value="">— choose a skill —</option>
              {skills.map(s => (
                <option key={s.name} value={s.name}>{s.name}</option>
              ))}
            </select>
          </div>

          <button
            className="self-start flex items-center gap-2 px-4 py-1.5 rounded bg-primary text-primary-foreground text-sm font-medium hover:bg-primary/90 disabled:opacity-50 transition-colors"
            onClick={build}
            disabled={!selectedTask || !selectedSkill || loading}
          >
            <Eye size={14} />
            {loading ? 'Building…' : 'Build Preview'}
          </button>
        </div>

        {error && (
          <div className="rounded border border-destructive/50 bg-destructive/5 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}

        {/* Result */}
        {context && (
          <div className="flex flex-col gap-4">
            {/* Meta row */}
            <div className="flex items-center gap-3 flex-wrap">
              <span className="text-xs text-muted-foreground">
                Task <code className="font-mono">{context.task_id}</code>
              </span>
              <span className="text-xs text-muted-foreground">·</span>
              <span className="text-xs text-muted-foreground">
                Skill <code className="font-mono">{context.skill_name}</code>
              </span>
              <span className="text-xs text-muted-foreground">·</span>
              <span className="text-xs text-muted-foreground">
                ~{context.word_count.toLocaleString()} words
              </span>
              <button
                className="ml-auto flex items-center gap-1.5 text-xs text-muted-foreground hover:text-foreground px-2 py-0.5 rounded border border-border hover:bg-muted/50 transition-colors"
                onClick={copyPrompt}
              >
                {copied ? <Check size={11} className="text-green-500" /> : <Copy size={11} />}
                {copied ? 'Copied' : 'Copy prompt'}
              </button>
            </div>

            {/* Sections */}
            <div className="flex flex-col gap-2">
              {context.sections.map((section, i) => (
                <div key={i} className="border border-border rounded overflow-hidden">
                  <button
                    className="w-full flex items-center gap-2 px-3 py-2 bg-muted/30 hover:bg-muted/50 transition-colors text-left border-b border-border"
                    onClick={() => setActiveSection(activeSection === i ? null : i)}
                  >
                    <span className="text-xs font-mono bg-primary/10 text-primary px-1.5 py-0.5 rounded">
                      {section.label}
                    </span>
                    <span className="text-xs text-muted-foreground ml-auto">
                      ~{section.content.split(/\s+/).length} words
                    </span>
                    <span className="text-muted-foreground text-xs">{activeSection === i ? '▲' : '▼'}</span>
                  </button>
                  {activeSection === i && (
                    <pre className="p-3 text-xs text-foreground/80 whitespace-pre-wrap font-mono max-h-64 overflow-y-auto leading-relaxed">
                      {section.content}
                    </pre>
                  )}
                </div>
              ))}
            </div>

            {/* Full prompt collapsed */}
            <details className="border border-border rounded overflow-hidden">
              <summary className="px-3 py-2 text-xs text-muted-foreground hover:text-foreground cursor-pointer bg-muted/20">
                View full assembled prompt
              </summary>
              <pre className={cn(
                'p-3 text-xs text-foreground/80 whitespace-pre-wrap font-mono overflow-y-auto leading-relaxed',
                'max-h-96'
              )}>
                {context.prompt}
              </pre>
            </details>
          </div>
        )}
      </div>
    </ScrollArea>
  )
}
