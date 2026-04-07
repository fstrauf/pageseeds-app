import { useEffect, useMemo, useState } from 'react'
import { CheckSquare, Square, Loader2, Sparkles } from 'lucide-react'
import { createArticleTasksFromKeywords } from '../../lib/tauri'
import { useQueue } from '../../lib/queue-context'
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
  shortage: number | null
  has_data: boolean
  serp_count?: number
  intent?: string | null
  intent_confidence?: number | null
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

function opportunityScore(row: KeywordRow): number {
  const kd = row.difficulty
  const kdScore = kd == null ? 40 : Math.max(0, 100 - kd)
  // Use traffic or volume only — never use shortage as a traffic proxy.
  const trafficSignal = Math.max(0, row.traffic ?? row.volume ?? 0)
  const trafficScore = Math.min(100, Math.log10(trafficSignal + 1) * 25)
  return kdScore * 0.6 + trafficScore * 0.4
}

function opportunityTier(row: KeywordRow): 'High' | 'Medium' | 'Low' {
  const score = opportunityScore(row)
  if (score >= 70) return 'High'
  if (score >= 45) return 'Medium'
  return 'Low'
}

function opportunityTierClass(tier: 'High' | 'Medium' | 'Low'): string {
  if (tier === 'High') return 'bg-emerald-100 text-emerald-700 border-transparent'
  if (tier === 'Medium') return 'bg-amber-100 text-amber-700 border-transparent'
  return 'bg-slate-100 text-slate-700 border-transparent'
}

// ─── Intent helpers ───────────────────────────────────────────────────────────

function intentColor(intent: string | null | undefined): string {
  if (!intent) return 'bg-secondary text-secondary-foreground border-transparent'
  switch (intent.toLowerCase()) {
    case 'informational':
      return 'bg-blue-100 text-blue-700 border-transparent'
    case 'commercial':
      return 'bg-green-100 text-green-700 border-transparent'
    case 'transactional':
      return 'bg-orange-100 text-orange-700 border-transparent'
    case 'navigational':
      return 'bg-gray-100 text-gray-700 border-transparent'
    default:
      return 'bg-secondary text-secondary-foreground border-transparent'
  }
}

function intentLabel(intent: string | null | undefined): string {
  if (!intent) return '—'
  return intent.charAt(0).toUpperCase() + intent.slice(1).toLowerCase()
}

function intentDescription(intent: string | null | undefined): string {
  if (!intent) return ''
  switch (intent.toLowerCase()) {
    case 'informational':
      return 'Blog post, guide, or tutorial'
    case 'commercial':
      return 'Comparison or review page'
    case 'transactional':
      return 'Landing or product page'
    case 'navigational':
      return 'Brand/navigation query'
    default:
      return ''
  }
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

// ─── Parse artifact ─────────────────────────────────────────────────────────--

function extractJsonFromMarkdown(content: string): string {
  const trimmed = content.trim()
  const codeFenceMatch = trimmed.match(/^```(?:json)?\s*([\s\S]*?)\s*```$/)
  if (codeFenceMatch) {
    return codeFenceMatch[1].trim()
  }
  return trimmed
}

function parseArtifact(content: string): KeywordResearchResult | null {
  try {
    const cleanContent = extractJsonFromMarkdown(content)
    const parsed = JSON.parse(cleanContent)
    
    // Handle new unified format: landing_page_candidates (from research_final_selection step)
    if (parsed.landing_page_candidates && Array.isArray(parsed.landing_page_candidates)) {
      return {
        new_keywords: parsed.landing_page_candidates.map((c: any) => c.keyword),
        total_candidates: parsed.landing_page_candidates.length,
        filtered_out: 0,
        difficulty: {
          total: parsed.landing_page_candidates.length,
          successful: parsed.landing_page_candidates.length,
          results: parsed.landing_page_candidates.map((c: any) => ({
            keyword: c.keyword,
            difficulty: c.estimated_kd ?? c.difficulty ?? null,
            volume: c.estimated_volume ?? c.volume ?? null,
            traffic: null,
            has_data: true,
            // Include landing page specific fields for display
            landing_page_type: c.landing_page_type,
            opportunity_score: c.opportunity_score,
            opportunity_reason: c.opportunity_reason,
            proposed_title: c.proposed_title,
          })),
        },
      }
    }
    
    // Handle new unified format: difficulty.results (from research_final_selection step)
    if (parsed.difficulty && parsed.difficulty.results && Array.isArray(parsed.difficulty.results)) {
      return {
        new_keywords: parsed.difficulty.results.map((r: any) => r.keyword),
        total_candidates: parsed.difficulty.results.length,
        filtered_out: 0,
        difficulty: {
          total: parsed.difficulty.total ?? parsed.difficulty.results.length,
          successful: parsed.difficulty.successful ?? parsed.difficulty.results.length,
          results: parsed.difficulty.results.map((r: any) => ({
            keyword: r.keyword,
            difficulty: r.difficulty ?? null,
            volume: r.volume ?? null,
            traffic: r.traffic ?? null,
            has_data: r.difficulty != null && r.volume != null,
            intent: r.intent,
            intent_confidence: r.intent_confidence,
          })),
        },
      }
    }
    
    // Handle ResearchFinalOutput format: results array from research_final_selection step
    if (parsed.results && Array.isArray(parsed.results)) {
      console.log('[KeywordPicker] ResearchFinalOutput format - first item:', parsed.results[0])
      console.log('[KeywordPicker] Traffic field:', parsed.results[0]?.traffic)
      const results = parsed.results.map((r: any) => ({
        keyword: r.keyword,
        difficulty: r.difficulty ?? null,
        volume: r.volume ?? null,
        traffic: r.traffic ?? null,
        has_data: r.difficulty != null && r.volume != null,
        intent: r.intent,
        intent_confidence: r.intent_confidence,
      }))
      console.log('[KeywordPicker] Mapped results:', results.slice(0, 3))
      return {
        new_keywords: parsed.results.map((r: any) => r.keyword),
        total_candidates: parsed.results.length,
        filtered_out: 0,
        difficulty: {
          total: parsed.results.length,
          successful: parsed.results.length,
          results,
        },
      }
    }
    
    // Handle keywords array format (intermediate step output)
    if (parsed.keywords && Array.isArray(parsed.keywords)) {
      // Debug: log first keyword to verify traffic data
      if (parsed.keywords.length > 0) {
        console.log('[KeywordPicker] First keyword data:', parsed.keywords[0])
      }
      const results = parsed.keywords.map((k: any) => ({
        keyword: k.keyword || k,
        difficulty: k.kd ?? k.difficulty ?? null,
        volume: k.volume ?? null,
        traffic: k.traffic ?? null,
        has_data: k.kd != null || k.difficulty != null,
        intent: k.intent,
        intent_confidence: k.intent_confidence,
      }))
      console.log('[KeywordPicker] Parsed results with traffic:', results.slice(0, 3))
      return {
        new_keywords: parsed.keywords.map((k: any) => k.keyword || k),
        total_candidates: parsed.keywords.length,
        filtered_out: 0,
        difficulty: {
          total: parsed.keywords.length,
          successful: parsed.keywords.length,
          results,
        },
      }
    }
    
    // Legacy formats
    return parsed as KeywordResearchResult
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
    // Use parseRangeMidpoint for volume since Ahrefs returns ranges like "1,000-10,000"
    const rawVol = entry?.volume ?? entry?.topVolume
    const volume = rawVol == null
      ? null
      : typeof rawVol === 'number'
        ? (Number.isFinite(rawVol) ? rawVol : null)
        : parseRangeMidpoint(String(rawVol)) ?? parseMetric(rawVol)
    const traffic = parseMetric(entry?.traffic)
    const shortage = parseMetric(entry?.shortage)
    // has_data: explicit field from backend, or derived from non-null difficulty for legacy results
    const has_data = entry?.has_data !== undefined
      ? entry.has_data
      : entry?.difficulty != null
    return {
      keyword: kw,
      difficulty: entry ? kdValue(entry.difficulty) : null,
      volume,
      traffic,
      shortage,
      has_data: has_data ?? false,
      serp_count: entry?.serp_count,
      intent: entry?.intent,
      intent_confidence: entry?.intent_confidence,
    }
  })
}

// ─── Component ────────────────────────────────────────────────────────────────

export function KeywordPicker({ task, onTasksCreated }: KeywordPickerProps) {
  const queue = useQueue()
  const artifact =
    // New unified workflow artifacts
    task.artifacts.find(a => a.key === 'research_final_selection') ??
    // Legacy artifacts (for backward compatibility)
    task.artifacts.find(a => a.key === 'research_normalize_stage') ??
    task.artifacts.find(a => a.key === 'landing_page_research_agentic') ??
    task.artifacts.find(a => a.key === 'landing_page_analyze') ??
    task.artifacts.find(a => a.key === 'landing_page_research') ??
    task.artifacts.find(a => a.key === 'research_keywords_cli') ??
    task.artifacts.find(a => a.key === 'research_agent_stage')
  
  const isLandingPageResearch = task.type === 'research_landing_pages'
  const result = useMemo(
    () => (artifact?.content ? parseArtifact(artifact.content) : null),
    [artifact?.content],
  )

  const rows = useMemo(() => {
    if (!result) return []
    return extractRows(result).sort((a, b) => opportunityScore(b) - opportunityScore(a))
  }, [result])
  
  // Intent filter state
  const [intentFilter, setIntentFilter] = useState<string>('all')
  
  // Filter rows by intent
  const filteredRows = useMemo(() => {
    if (intentFilter === 'all') return rows
    return rows.filter(row => row.intent?.toLowerCase() === intentFilter)
  }, [rows, intentFilter])

  const defaultSelected = useMemo(
    () => new Set(filteredRows.filter(r => r.has_data && opportunityTier(r) !== 'Low').map(r => r.keyword)),
    [filteredRows],
  )

  const [selected, setSelected] = useState<Set<string>>(defaultSelected)
  useEffect(() => {
    setSelected(defaultSelected)
  }, [defaultSelected])
  const [creating, setCreating] = useState(false)
  const [error, setError] = useState<string | null>(null)
  
  // Get unique intents for filter dropdown
  const availableIntents = useMemo(() => {
    const intents = new Set<string>()
    rows.forEach(row => {
      if (row.intent) intents.add(row.intent.toLowerCase())
    })
    return Array.from(intents).sort()
  }, [rows])

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

  const noDataCount = rows.filter(r => !r.has_data).length

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
      if (next.has(keyword)) {
        next.delete(keyword)
      } else {
        next.add(keyword)
      }
      return next
    })
  }

  function selectAll() {
    setSelected(new Set(filteredRows.map(r => r.keyword)))
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
      
      // Auto-add created tasks to the queue (shopping cart pattern)
      if (tasks.length > 0) {
        queue.enqueueNext(
          tasks.map(t => ({
            taskId: t.id,
            projectId: t.project_id,
            title: t.title ?? 'Write article',
            taskType: t.type ?? 'write_article',
            projectName: undefined,
          }))
        )
      }
      
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
          Showing {filteredRows.length} of {rows.length} keyword{rows.length !== 1 ? 's' : ''} for selection
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
      
      {/* Intent Filter */}
      {availableIntents.length > 0 && (
        <div className="flex items-center gap-2">
          <span className="text-xs text-muted-foreground">Filter by intent:</span>
          <select
            value={intentFilter}
            onChange={(e) => setIntentFilter(e.target.value)}
            className="text-xs border border-border rounded px-2 py-1 bg-background"
          >
            <option value="all">All intents</option>
            {availableIntents.map(intent => (
              <option key={intent} value={intent}>
                {intentLabel(intent)}
              </option>
            ))}
          </select>
          {intentFilter !== 'all' && (
            <Button
              variant="ghost"
              size="xs"
              onClick={() => setIntentFilter('all')}
              className="h-auto py-0 text-xs text-muted-foreground hover:text-foreground"
            >
              Clear
            </Button>
          )}
        </div>
      )}

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
        {noDataCount > rows.length / 2 && (
          <div className="text-xs text-amber-700 bg-amber-50 border border-amber-200 px-3 py-2 rounded-md">
            {noDataCount} of {rows.length} keywords had no Ahrefs data — Ahrefs free tier doesn't index all keywords. Re-run with broader themes for better results.
          </div>
        )}
        <div className="max-h-[min(44vh,22rem)] rounded-md border border-border min-w-0 overflow-y-auto overflow-x-hidden">
          <div className="space-y-0.5 p-1">
          {filteredRows.map(row => {
            const isSelected = selected.has(row.keyword)
            const kd = row.difficulty
            // Traffic is only real SERP organic traffic — never falls back to shortage
            const trafficLabel = formatMetric(row.traffic ?? row.volume)
            const tier = opportunityTier(row)
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
                {row.intent && (
                  <Badge 
                    variant="outline" 
                    className={cn('text-[10px] px-1.5 py-0', intentColor(row.intent))}
                    title={intentDescription(row.intent)}
                  >
                    {intentLabel(row.intent)}
                  </Badge>
                )}
                <div className="flex items-center gap-1.5 shrink-0 max-w-[48%]">
                  {!row.has_data && (
                    <Badge variant="outline" className="text-[10px] px-1.5 py-0 bg-amber-50 text-amber-600 border-amber-200">
                      No data
                    </Badge>
                  )}
                  {row.has_data && (
                    <Badge variant="outline" className={cn('text-[10px] px-1.5 py-0', opportunityTierClass(tier))}>
                      {tier}
                    </Badge>
                  )}
                  <Badge variant="outline" className="text-[10px] border-border text-muted-foreground">
                    Traffic {trafficLabel}
                  </Badge>
                  {kd != null && row.has_data && (
                    <Badge variant="outline" className={cn('text-[10px] px-1.5 py-0', kdColor(kd))}>
                      KD {kd}
                    </Badge>
                  )}
                  {row.has_data && (
                    <span className={cn('text-[10px] truncate', kd == null ? 'text-muted-foreground' : kdColor(kd).split(' ')[1])}>
                      {kdLabel(kd)}
                    </span>
                  )}
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
            ) : isLandingPageResearch ? (
              <><Sparkles size={13} className="mr-1.5" />Create {selected.size} Landing Page Task{selected.size !== 1 ? 's' : ''}</>
            ) : (
              <><Sparkles size={13} className="mr-1.5" />Create {selected.size} Article Task{selected.size !== 1 ? 's' : ''}</>
            )}
          </Button>
        </>
      )}
    </div>
  )
}
