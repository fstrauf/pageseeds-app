import { useState, useMemo } from 'react'
import { CheckSquare, Square, Loader2, Sparkles } from 'lucide-react'
import { createArticleTasksFromKeywords } from '../../lib/tauri'
import type { KeywordDifficultyEntry, KeywordResearchResult, Task } from '../../lib/types'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'
import { cn } from '../../lib/utils'

interface KeywordRow {
  keyword: string
  difficulty: number | null
  volume: number | null
  traffic: number | null
  serp_count?: number
}

interface KeywordPickerProps {
  task: Task
  onTasksCreated: (tasks: Task[]) => void
}

// ─── KD helpers ───────────────────────────────────────────────────────────────

function kdValue(raw: number | string | null | undefined): number | null {
  if (raw == null) return null
  const n = typeof raw === 'number' ? raw : parseInt(String(raw), 10)
  return isNaN(n) ? null : n
}

function kdLabel(kd: number | null): string {
  if (kd == null) return '—'
  if (kd < 10) return 'Very Easy'
  if (kd < 30) return 'Easy'
  if (kd < 50) return 'Medium'
  if (kd < 70) return 'Hard'
  return 'Very Hard'
}

function kdColor(kd: number | null): string {
  if (kd == null) return 'bg-secondary text-secondary-foreground border-transparent'
  if (kd < 10) return 'bg-emerald-100 text-emerald-700 border-transparent'
  if (kd < 30) return 'bg-green-100 text-green-700 border-transparent'
  if (kd < 50) return 'bg-amber-100 text-amber-700 border-transparent'
  if (kd < 70) return 'bg-orange-100 text-orange-700 border-transparent'
  return 'bg-red-100 text-red-700 border-transparent'
}

function parseMetric(raw: number | string | null | undefined): number | null {
  if (raw == null) return null
  if (typeof raw === 'number') return Number.isFinite(raw) ? raw : null
  const cleaned = String(raw).replace(/,/g, '').trim()
  if (!cleaned) return null
  const n = Number.parseInt(cleaned, 10)
  return Number.isNaN(n) ? null : n
}

function formatMetric(n: number | null): string {
  if (n == null) return '—'
  return n.toLocaleString('en-US')
}

function parseRangeMidpoint(raw: string): number | null {
  const nums = (raw.match(/\d[\d,]*/g) ?? [])
    .map(s => Number.parseInt(s.replace(/,/g, ''), 10))
    .filter(n => Number.isFinite(n))
  if (nums.length === 0) return null
  if (nums.length === 1) return nums[0]
  return Math.round((nums[0] + nums[1]) / 2)
}

function buildFromMarkdownTable(content: string): KeywordResearchResult | null {
  const rows: Array<{ keyword: string; volume: number | null; difficulty: number | null }> = []
  const lines = content.split(/\r?\n/)

  for (const line of lines) {
    if (!line.includes('|')) continue
    if (/\|\s*[-:]+\s*\|/.test(line)) continue

    const cols = line
      .split('|')
      .map(c => c.trim())
      .filter(Boolean)

    // Expected markdown summary row shape:
    // Priority | Keyword | Vol | KD
    if (cols.length < 4) continue

    const keyword = cols[1]
    const volume = parseRangeMidpoint(cols[2])
    const difficulty = parseRangeMidpoint(cols[3])

    // Skip headers and malformed rows.
    if (!keyword || /^keyword$/i.test(keyword) || /^priority$/i.test(cols[0])) continue

    rows.push({ keyword, volume, difficulty })
  }

  if (rows.length === 0) return null

  return {
    new_keywords: rows.map(r => r.keyword),
    total_candidates: rows.length,
    filtered_out: 0,
    difficulty: {
      total: rows.length,
      successful: rows.length,
      results: rows.map(r => ({
        keyword: r.keyword,
        difficulty: r.difficulty,
        volume: r.volume,
      })),
    },
  }
}

// ─── Parse artifact ───────────────────────────────────────────────────────────

function parseArtifact(content: string): KeywordResearchResult | null {
  try {
    return JSON.parse(content) as KeywordResearchResult
  } catch {
    // Agentic output can be markdown tables instead of JSON. Build a compatible
    // synthetic result so the picker still works.
    return buildFromMarkdownTable(content)
  }
}

function extractRows(result: KeywordResearchResult): KeywordRow[] {
  // Normalise difficulty data into a flat map: keyword → difficulty entry
  const diffMap = new Map<string, KeywordDifficultyEntry>()

  if (result.difficulty) {
    if (Array.isArray(result.difficulty)) {
      // Rare case where difficulty is already a list
      for (const entry of result.difficulty) {
        diffMap.set(entry.keyword.toLowerCase(), entry)
      }
    } else if (result.difficulty.results) {
      for (const entry of result.difficulty.results) {
        diffMap.set(entry.keyword.toLowerCase(), entry)
      }
    }
  }

  // CLI parity: when difficulty results exist, they are the selectable set.
  // Otherwise fall back to first 10 new keywords.
  const analyzedKeywords = Array.from(diffMap.values()).map(entry => entry.keyword)
  const selectedKeywords =
    analyzedKeywords.length > 0
      ? analyzedKeywords.slice(0, 10)
      : result.new_keywords.slice(0, 10)

  return selectedKeywords.map(kw => {
    const entry = diffMap.get(kw.toLowerCase())
    const volume = parseMetric(entry?.volume ?? entry?.topVolume)
    const traffic = parseMetric(entry?.traffic)
    return {
      keyword: kw,
      difficulty: entry ? kdValue(entry.difficulty) : null,
      volume,
      traffic,
      serp_count: entry?.serp_count,
    }
  })
}

// ─── Component ────────────────────────────────────────────────────────────────

export function KeywordPicker({ task, onTasksCreated }: KeywordPickerProps) {
  const artifact =
    task.artifacts.find(a => a.key === 'research_normalize_stage') ??
    task.artifacts.find(a => a.key === 'research_keywords_cli') ??
    task.artifacts.find(a => a.key === 'research_agent_stage')
  const result = useMemo(
    () => (artifact?.content ? parseArtifact(artifact.content) : null),
    [artifact?.content],
  )

  const rows = useMemo(() => (result ? extractRows(result) : []), [result])

  // Pre-select keywords with KD < 30 (Easy or better) — matches CLI reference scoring.
  // Unknown KD (null) is included because we don't have enough data to exclude it.
  const [selected, setSelected] = useState<Set<string>>(
    () => new Set(rows.filter(r => r.difficulty == null || r.difficulty < 30).map(r => r.keyword)),
  )
  const [creating, setCreating] = useState(false)
  const [error, setError] = useState<string | null>(null)

  if (!artifact?.content) {
    return (
      <div className="text-xs text-muted-foreground px-3 py-2.5 bg-secondary/40 rounded-md">
        No keyword research artifact found. Run the task first.
      </div>
    )
  }

  if (!result) {
    return (
      <div className="text-xs text-destructive px-3 py-2.5 bg-destructive/10 rounded-md">
        Could not parse keyword research output.
      </div>
    )
  }

  const analyzedCount = result.difficulty
    ? Array.isArray(result.difficulty)
      ? result.difficulty.length
      : (result.difficulty.results?.length ?? 0)
    : 0

  const difficultyBuckets = rows.reduce(
    (acc, row) => {
      const kd = row.difficulty
      if (kd == null) {
        acc.unknown += 1
      } else if (kd < 10) {
        acc.veryEasy += 1
      } else if (kd < 30) {
        acc.easy += 1
      } else if (kd < 50) {
        acc.medium += 1
      } else {
        acc.hardPlus += 1
      }
      return acc
    },
    { veryEasy: 0, easy: 0, medium: 0, hardPlus: 0, unknown: 0 },
  )

  function toggle(keyword: string) {
    setSelected(prev => {
      const next = new Set(prev)
      next.has(keyword) ? next.delete(keyword) : next.add(keyword)
      return next
    })
  }

  function selectAll() {
    setSelected(new Set(rows.map(r => r.keyword)))
  }

  function selectNone() {
    setSelected(new Set())
  }

  async function handleCreate() {
    if (selected.size === 0) return
    setCreating(true)
    setError(null)
    try {
      const tasks = await createArticleTasksFromKeywords(
        task.project_id,
        task.id,
        Array.from(selected),
      )
      onTasksCreated(tasks)
    } catch (e) {
      setError(String(e))
      setCreating(false)
    }
  }

  return (
    <div className="space-y-3 min-w-0 overflow-x-hidden">
      {/* Summary */}
      <div className="flex items-start justify-between gap-2 text-xs text-muted-foreground min-w-0">
        <span className="min-w-0">
          Showing top {rows.length} keyword{rows.length !== 1 ? 's' : ''} for selection
          {result.filtered_out ? ` · ${result.filtered_out} already covered` : ''}
          {analyzedCount > 0 ? ` · ${analyzedCount} analyzed` : ''}
        </span>
        <div className="flex gap-1 shrink-0">
          <Button variant="ghost" size="xs" onClick={selectAll} className="h-auto py-0 text-xs text-muted-foreground hover:text-foreground">
            All
          </Button>
          <Button variant="ghost" size="xs" onClick={selectNone} className="h-auto py-0 text-xs text-muted-foreground hover:text-foreground">
            None
          </Button>
        </div>
      </div>

      {/* Keyword rows */}
      {rows.length === 0 ? (
        <div className="text-xs text-muted-foreground italic">
          No new keywords found. All themes may already be covered in articles.json.
        </div>
      ) : (
        <>
        <div className="flex flex-wrap items-center gap-1.5 text-[11px]">
          <Badge variant="outline" className="border-border text-muted-foreground">Very Easy {difficultyBuckets.veryEasy}</Badge>
          <Badge variant="outline" className="border-border text-muted-foreground">Easy {difficultyBuckets.easy}</Badge>
          <Badge variant="outline" className="border-border text-muted-foreground">Medium {difficultyBuckets.medium}</Badge>
          <Badge variant="outline" className="border-border text-muted-foreground">Hard+ {difficultyBuckets.hardPlus}</Badge>
          {difficultyBuckets.unknown > 0 && (
            <Badge variant="outline" className="border-border text-muted-foreground">Unknown {difficultyBuckets.unknown}</Badge>
          )}
        </div>
        <div className="max-h-[min(44vh,22rem)] rounded-md border border-border min-w-0 overflow-y-auto overflow-x-hidden">
          <div className="space-y-0.5 p-1">
          {rows.map(row => {
            const isSelected = selected.has(row.keyword)
            const kd = row.difficulty
            const trafficLabel = formatMetric(row.traffic ?? row.volume)
            return (
              <button
                key={row.keyword}
                onClick={() => toggle(row.keyword)}
                className={cn(
                  'w-full min-w-0 flex items-center gap-2.5 px-2.5 py-2 rounded-md text-left text-xs transition-colors',
                  isSelected
                    ? 'bg-primary/8 border border-primary/20'
                    : 'bg-secondary/40 border border-transparent hover:bg-secondary/70',
                )}
              >
                {isSelected
                  ? <CheckSquare size={13} className="text-primary shrink-0" />
                  : <Square size={13} className="text-muted-foreground shrink-0" />
                }
                <span className="flex-1 min-w-0 truncate text-foreground">{row.keyword}</span>
                <div className="flex items-center gap-1.5 shrink-0 max-w-[48%]">
                  <Badge variant="outline" className="text-[10px] border-border text-muted-foreground">
                    Traffic {trafficLabel}
                  </Badge>
                  {kd != null && (
                    <Badge variant="outline" className={cn('text-[10px] px-1.5 py-0', kdColor(kd))}>
                      KD {kd}
                    </Badge>
                  )}
                  <span className={cn('text-[10px] truncate', kd == null ? 'text-muted-foreground' : kdColor(kd).split(' ')[1])}>
                    {kdLabel(kd)}
                  </span>
                </div>
              </button>
            )
          })}
          </div>
        </div>
        </>
      )}

      {/* Skipped keywords (no difficulty data) note */}
      {result.difficulty_skipped_keywords && result.difficulty_skipped_keywords.length > 0 && (
        <p className="text-[11px] text-muted-foreground">
          {result.difficulty_skipped_keywords.length} additional keywords are available but hidden to keep this review focused to top-10.
        </p>
      )}

      {error && (
        <div className="text-xs text-destructive bg-destructive/10 px-3 py-2 rounded-md">{error}</div>
      )}

      {rows.length > 0 && (
        <>
          <Separator className="bg-border" />
          <Button
            size="sm"
            className="w-full"
            onClick={handleCreate}
            disabled={selected.size === 0 || creating}
          >
            {creating ? (
              <><Loader2 size={13} className="mr-1.5 animate-spin" />Creating…</>
            ) : (
              <><Sparkles size={13} className="mr-1.5" />Create {selected.size} Article Task{selected.size !== 1 ? 's' : ''}</>
            )}
          </Button>
        </>
      )}
    </div>
  )
}
