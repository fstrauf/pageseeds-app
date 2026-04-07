import React, { useState } from 'react'
import { X } from 'lucide-react'
import { createTask } from '../../lib/tauri'
import type { Task } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Textarea } from '@/components/ui/textarea'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'

const TASK_TYPES = [
  'write_article',
  'create_landing_page',
  'collect_gsc',
  'collect_posthog',
  'research_keywords',
  'research_landing_pages',
  'reddit_search',
  'reddit_reply',
  'analyse_gsc',
  'optimise_article',
  'fix_indexing',
  'fix_redirects',
  'implementation',
]

const KEYWORD_RESEARCH_TYPES = new Set(['research_keywords', 'custom_keyword_research', 'research_landing_pages'])

interface TaskCreateProps {
  projectId: string
  onClose: () => void
  onCreated: (task: Task) => void
}

export function TaskCreate({ projectId, onClose, onCreated }: TaskCreateProps) {
  const [taskType, setTaskType] = useState('write_article')
  const [customType, setCustomType] = useState('')
  const [title, setTitle] = useState('')
  const [themes, setThemes] = useState('')
  const [landingPageContext, setLandingPageContext] = useState('')
  const [priority, setPriority] = useState('medium')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const resolvedType = taskType === '__custom__' ? customType.trim() : taskType
  const isKeywordResearch = KEYWORD_RESEARCH_TYPES.has(resolvedType)
  const isLandingPageResearch = resolvedType === 'research_landing_pages'

  async function handleCreate(e: React.FormEvent) {
    e.preventDefault()
    if (!resolvedType) return
    setSaving(true)
    setError(null)
    try {
      // Build description based on task type
      let description: string | undefined
      if (isLandingPageResearch) {
        // Landing page research: JSON format with context and optional themes
        const themesList = themes.trim()
          ? themes.split('\n').map(t => t.trim()).filter(Boolean)
          : undefined
        if (landingPageContext.trim() || themesList) {
          description = JSON.stringify({
            context: landingPageContext.trim(),
            themes: themesList,
          })
        }
      } else if (isKeywordResearch && themes.trim()) {
        // Regular keyword research: plain text themes
        description = themes.trim()
      }
      
      const task = await createTask(projectId, resolvedType, title || undefined, description, priority)
      onCreated(task)
    } catch (e: unknown) {
      setError(String(e))
      setSaving(false)
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      style={{ background: 'rgba(0,0,0,0.5)' }}
      onClick={e => { if (e.target === e.currentTarget) onClose() }}
    >
      <div className="bg-card border border-border rounded-lg shadow-xl w-[400px]">
        {/* Header */}
        <div className="flex items-center justify-between px-5 py-4 border-b border-border">
          <h2 className="text-sm font-semibold text-foreground">New Task</h2>
          <Button variant="ghost" size="icon-sm" onClick={onClose} className="text-muted-foreground">
            <X size={15} />
          </Button>
        </div>

        <form onSubmit={handleCreate}>
          <div className="px-5 py-5 space-y-4">
            {error && (
              <div className="px-3 py-2 rounded-md text-sm bg-destructive/15 text-destructive">
                {error}
              </div>
            )}

            {/* Task type */}
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">Type</Label>
              <Select value={taskType} onValueChange={v => { setTaskType(v); setThemes(''); setLandingPageContext('') }}>
                <SelectTrigger className="bg-background border-border text-foreground text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent className="bg-popover border-border text-popover-foreground">
                  {TASK_TYPES.map(t => (
                    <SelectItem key={t} value={t} className="text-sm font-mono">{t}</SelectItem>
                  ))}
                  <SelectItem value="__custom__" className="text-sm">Custom…</SelectItem>
                </SelectContent>
              </Select>
              {taskType === '__custom__' && (
                <Input
                  value={customType}
                  onChange={e => setCustomType(e.target.value)}
                  placeholder="e.g. fix_canonicals"
                  className="mt-1.5 bg-background border-border text-foreground text-sm font-mono"
                  autoFocus
                />
              )}
            </div>

            {/* Landing page strategy context — shown only for landing page research */}
            {isLandingPageResearch && (
              <div className="space-y-1.5">
                <Label className="text-xs text-muted-foreground">
                  Landing Page Strategy Context <span className="text-muted-foreground/50">(optional)</span>
                </Label>
                <Textarea
                  value={landingPageContext}
                  onChange={e => setLandingPageContext(e.target.value)}
                  placeholder={'Describe your landing page goals, target audience, and what makes your offering unique.\n\nExamples:\n• "Enterprise CRM for real estate agents"\n• "Looking for high-intent comparison terms"\n• "Target: CTOs at Series A startups"'}
                  rows={5}
                  className="bg-background border-border text-foreground text-sm resize-none"
                />
                <p className="text-[11px] text-muted-foreground leading-relaxed">
                  This context helps guide keyword selection for conversion-focused landing pages.
                </p>
              </div>
            )}

            {/* Keyword themes — shown for research task types */}
            {isKeywordResearch && (
              <div className="space-y-1.5">
                <Label className="text-xs text-muted-foreground">
                  Keyword Themes <span className="text-muted-foreground/50">(optional — auto-derived if blank)</span>
                </Label>
                <Textarea
                  value={themes}
                  onChange={e => setThemes(e.target.value)}
                  placeholder={'Enter topics, one per line\nExample:\ncoffee brewing methods\nespresso guides\nhome barista tips'}
                  rows={4}
                  className="bg-background border-border text-foreground text-sm resize-none"
                />
                <p className="text-[11px] text-muted-foreground leading-relaxed">
                  If left blank, themes are auto-derived from your content brief or articles.json.
                </p>
              </div>
            )}

            {/* Title */}
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">Title <span className="text-muted-foreground/50">(optional)</span></Label>
              <Input
                value={title}
                onChange={e => setTitle(e.target.value)}
                placeholder="Brief description…"
                className="bg-background border-border text-foreground text-sm"
              />
            </div>

            {/* Priority */}
            <div className="space-y-1.5">
              <Label className="text-xs text-muted-foreground">Priority</Label>
              <Select value={priority} onValueChange={setPriority}>
                <SelectTrigger className="bg-background border-border text-foreground text-sm">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent className="bg-popover border-border text-popover-foreground">
                  {['high', 'medium', 'low'].map(p => (
                    <SelectItem key={p} value={p} className="text-sm">{p}</SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>

          {/* Footer */}
          <div className="px-5 pb-5 flex items-center justify-end gap-2">
            <Button type="button" variant="ghost" size="sm" onClick={onClose} className="text-muted-foreground">
              Cancel
            </Button>
            <Button
              type="submit"
              size="sm"
              disabled={saving || !resolvedType}
            >
              {saving ? 'Creating…' : 'Create task'}
            </Button>
          </div>
        </form>
      </div>
    </div>
  )
}
