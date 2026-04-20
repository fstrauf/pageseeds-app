import { useState, useEffect, useCallback } from 'react'
import { Search, RefreshCw, ChevronDown, ChevronUp } from 'lucide-react'
import { listSkills } from '../../lib/tauri'
import { useErrorHandler } from '../../lib/toast-context'
import type { Skill } from '../../lib/types'
import { ScrollArea } from '@/components/ui/scroll-area'
import { cn } from '../../lib/utils'

interface SkillBrowserProps {
  projectId: string
}

function SkillCard({ skill }: { skill: Skill }) {
  const [expanded, setExpanded] = useState(false)

  return (
    <div className="border border-border rounded overflow-hidden">
      <button
        className="w-full flex items-start gap-3 px-4 py-3 text-left hover:bg-muted/40 transition-colors"
        onClick={() => setExpanded(v => !v)}
      >
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium text-foreground font-mono">{skill.name}</p>
          <p className="text-xs text-muted-foreground mt-0.5 line-clamp-2">{skill.description}</p>
          <p className="text-xs text-muted-foreground/60 mt-1">{skill.skill_dir}</p>
        </div>
        <span className="shrink-0 text-muted-foreground mt-0.5">
          {expanded ? <ChevronUp size={14} /> : <ChevronDown size={14} />}
        </span>
      </button>

      {expanded && (
        <div className="border-t border-border bg-muted/20">
          <pre className="p-4 text-xs text-foreground/80 whitespace-pre-wrap font-mono overflow-x-auto max-h-96 overflow-y-auto leading-relaxed">
            {skill.content}
          </pre>
        </div>
      )}
    </div>
  )
}

export function SkillBrowser({ projectId }: SkillBrowserProps) {
  const [skills, setSkills] = useState<Skill[]>([])
  const [loading, setLoading] = useState(false)
  const [query, setQuery] = useState('')
  const { showError } = useErrorHandler()

  const load = useCallback(async () => {
    setLoading(true)
    try {
      const data = await listSkills(projectId)
      setSkills(data)
    } catch (e: unknown) {
      showError(String(e))
    } finally {
      setLoading(false)
    }
  }, [projectId, showError])

  useEffect(() => {
    if (projectId) load()
  }, [projectId, load])

  const filtered = query.trim()
    ? skills.filter(s =>
        s.name.toLowerCase().includes(query.toLowerCase()) ||
        s.description.toLowerCase().includes(query.toLowerCase()),
      )
    : skills

  return (
    <ScrollArea className="h-full">
      <div className="p-6 flex flex-col gap-6">
        {/* Header */}
        <div className="flex items-start justify-between">
          <div>
            <h2 className="text-base font-semibold text-foreground mb-1">Skill Browser</h2>
            <p className="text-xs text-muted-foreground">
              Skills loaded from{' '}
              <code className="font-mono text-xs">.github/skills/*/SKILL.md</code>
            </p>
          </div>
          <button
            onClick={load}
            className="text-muted-foreground hover:text-foreground transition-colors shrink-0"
            title="Refresh"
          >
            <RefreshCw size={14} className={cn(loading && 'animate-spin')} />
          </button>
        </div>

        {/* Search */}
        <div className="relative">
          <Search size={13} className="absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground" />
          <input
            className="w-full rounded border border-border bg-background pl-8 pr-3 py-1.5 text-sm placeholder:text-muted-foreground focus:outline-none focus:ring-1 focus:ring-ring"
            placeholder="Filter skills…"
            value={query}
            onChange={e => setQuery(e.target.value)}
          />
        </div>

        {!loading && filtered.length === 0 && (
          <p className="text-sm text-muted-foreground">
            {skills.length === 0
              ? 'No skills found. Make sure the project repo contains .github/skills/.'
              : 'No skills match your filter.'}
          </p>
        )}

        {filtered.length > 0 && (
          <div className="flex flex-col gap-2">
            <p className="text-xs text-muted-foreground">
              {filtered.length} skill{filtered.length !== 1 ? 's' : ''}
              {query ? ` matching "${query}"` : ''}
            </p>
            {filtered.map(skill => (
              <SkillCard key={skill.name} skill={skill} />
            ))}
          </div>
        )}
      </div>
    </ScrollArea>
  )
}
