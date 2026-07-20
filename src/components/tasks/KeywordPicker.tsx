import { useEffect, useMemo, useState } from 'react'
import { CheckSquare, Square, Loader2, Sparkles } from 'lucide-react'
import { useErrorHandler } from '../../lib/toast-context'
import { createArticleTasksFromKeywords } from '../../lib/tauri'
import { useQueue } from '../../lib/queue-context'
import type { KeywordDifficultyEntry, KeywordResearchResult, Task } from '../../lib/types'
import {
  kdValue,
  kdLabel,
  kdColor,
  opportunityScore,
  opportunityTier,
  opportunityTierClass,
  parseMetric,
  formatMetric,
  type KeywordRow,
} from '../../lib/keywords'
import { extractJsonString } from '../../lib/artifacts'
import { Button } from '@/components/ui/button'
import { Badge } from '@/components/ui/badge'
import { Separator } from '@/components/ui/separator'
import { cn } from '../../lib/utils'

interface KeywordPickerProps {
  task: Task
  onTasksCreated: (tasks: Task[]) => void
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

// ─── Winnability helpers ──────────────────────────────────────────────────────

function winnabilityColor(bucket: string | null | undefined): string {
  switch (bucket?.toLowerCase()) {
    case 'target':
      return 'bg-emerald-100 text-emerald-700 border-transparent'
    case 'differentiate':
      return 'bg-amber-100 text-amber-700 border-transparent'
    case 'avoid':
      return 'bg-red-100 text-red-700 border-transparent'
    default:
      return 'bg-secondary text-secondary-foreground border-transparent'
  }
}

function winnabilityLabel(bucket: string | null | undefined): string {
  if (!bucket) return ''
  return bucket.charAt(0).toUpperCase() + bucket.slice(1).toLowerCase()
}

function isAvoid(row: KeywordRow): boolean {
  return row.winnability?.toLowerCase() === 'avoid'
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

interface LandingPageCandidateArtifact {
  keyword: string
  estimated_kd?: number | string | null
  difficulty?: number | string | null
  estimated_volume?: number | string | null
  volume?: number | string | null
  landing_page_type?: string | null
  opportunity_score?: number | null
  opportunity_reason?: string | null
  proposed_title?: string | null
  cpc?: number | null
  winnability?: string | null
  winnability_reason?: string | null
}

interface DifficultyArtifact {
  keyword: string
  difficulty?: number | string | null
  volume?: number | string | null
  traffic?: number | string | null
  intent?: string | null
  intent_confidence?: number | null
  winnability?: string | null
  winnability_reason?: string | null
  gap_score?: number | null
}

type KeywordArtifact = {
  keyword?: string
  kd?: number | string | null
  difficulty?: number | string | null
  volume?: number | string | null
  traffic?: number | string | null
  intent?: string | null
  intent_confidence?: number | null
} | string

function parseArtifact(content: string): KeywordResearchResult | null {
  try {
    const cleanContent = extractJsonString(content)
    if (!cleanContent) {
      // Agentic output can be markdown tables instead of JSON. Build a compatible
      // synthetic result so the picker still works.
      return buildFromMarkdownTable(content)
    }
    const parsed = JSON.parse(cleanContent)
    
    // Handle new unified format: landing_page_candidates (from research_final_selection step)
    if (parsed.landing_page_candidates && Array.isArray(parsed.landing_page_candidates)) {
      const candidates = parsed.landing_page_candidates as LandingPageCandidateArtifact[]
      return {
        new_keywords: candidates.map((c: LandingPageCandidateArtifact) => c.keyword),
        total_candidates: candidates.length,
        filtered_out: 0,
        difficulty: {
          total: candidates.length,
          successful: candidates.length,
          results: candidates.map((c: LandingPageCandidateArtifact) => ({
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
            cpc: c.cpc ?? null,
            winnability: c.winnability ?? null,
            winnability_reason: c.winnability_reason ?? null,
          })),
        },
      }
    }
    
    // Handle new unified format: difficulty.results (from research_final_selection step)
    if (parsed.difficulty && parsed.difficulty.results && Array.isArray(parsed.difficulty.results)) {
      const difficultyResults = parsed.difficulty.results as DifficultyArtifact[]
      return {
        new_keywords: difficultyResults.map((r: DifficultyArtifact) => r.keyword),
        total_candidates: difficultyResults.length,
        filtered_out: 0,
        difficulty: {
          total: parsed.difficulty.total ?? difficultyResults.length,
          successful: parsed.difficulty.successful ?? difficultyResults.length,
          results: difficultyResults.map((r: DifficultyArtifact) => ({
            keyword: r.keyword,
            difficulty: r.difficulty ?? null,
            volume: r.volume ?? null,
            traffic: r.traffic ?? null,
            has_data: r.difficulty != null && r.volume != null,
            intent: r.intent,
            intent_confidence: r.intent_confidence,
            winnability: r.winnability ?? null,
            winnability_reason: r.winnability_reason ?? null,
          })),
        },
      }
    }
    
    // Handle ResearchFinalOutput format: results array from research_final_selection step
    if (parsed.results && Array.isArray(parsed.results)) {
      const resultsSource = parsed.results as DifficultyArtifact[]
      console.log('[KeywordPicker] ResearchFinalOutput format - first item:', parsed.results[0])
      console.log('[KeywordPicker] Traffic field:', parsed.results[0]?.traffic)
      const results = resultsSource.map((r: DifficultyArtifact) => ({
        keyword: r.keyword,
        difficulty: r.difficulty ?? null,
        volume: r.volume ?? null,
        traffic: r.traffic ?? null,
        has_data: r.difficulty != null && r.volume != null,
        intent: r.intent,
        intent_confidence: r.intent_confidence,
        winnability: r.winnability ?? null,
        winnability_reason: r.winnability_reason ?? null,
      }))
      console.log('[KeywordPicker] Mapped results:', results.slice(0, 3))
      return {
        new_keywords: resultsSource.map((r: DifficultyArtifact) => r.keyword),
        total_candidates: resultsSource.length,
        filtered_out: 0,
        difficulty: {
          total: resultsSource.length,
          successful: resultsSource.length,
          results,
        },
      }
    }
    
    // Handle keywords array format (intermediate step output)
    if (parsed.keywords && Array.isArray(parsed.keywords)) {
      const keywords = parsed.keywords as KeywordArtifact[]
      // Debug: log first keyword to verify traffic data
      if (keywords.length > 0) {
        console.log('[KeywordPicker] First keyword data:', keywords[0])
      }
      const results = keywords.map((k: KeywordArtifact) => ({
        keyword: typeof k === 'string' ? k : (k.keyword ?? ''),
        difficulty: typeof k === 'string' ? null : (k.kd ?? k.difficulty ?? null),
        volume: typeof k === 'string' ? null : (k.volume ?? null),
        traffic: typeof k === 'string' ? null : (k.traffic ?? null),
        has_data: typeof k === 'string' ? false : (k.kd != null || k.difficulty != null),
        intent: typeof k === 'string' ? null : (k.intent ?? null),
        intent_confidence: typeof k === 'string' ? null : (k.intent_confidence ?? null),
      }))
      console.log('[KeywordPicker] Parsed results with traffic:', results.slice(0, 3))
      return {
        new_keywords: keywords.map((k: KeywordArtifact) => typeof k === 'string' ? k : (k.keyword ?? '')),
        total_candidates: keywords.length,
        filtered_out: 0,
        difficulty: {
          total: keywords.length,
          successful: keywords.length,
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
      winnability: entry?.winnability,
      winnability_reason: entry?.winnability_reason,
      cpc: typeof entry?.cpc === 'number' && Number.isFinite(entry.cpc) ? entry.cpc : null,
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
  const { showError } = useErrorHandler()
  
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
    try {
      const tasks = await createArticleTasksFromKeywords(
        task.project_id,
        task.id,
        Array.from(selected),
      )
      
      // Auto-add created tasks to the queue (shopping cart pattern)
      if (tasks.length > 0) {
        queue.enqueue(
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
      showError(String(e))
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
                  // De-emphasize keywords the SERP enrichment scored unwinnable.
                  isAvoid(row) && 'opacity-60',
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
                {row.winnability && (
                  <Badge
                    variant="outline"
                    className={cn('text-[10px] px-1.5 py-0', winnabilityColor(row.winnability))}
                    title={row.winnability_reason ?? undefined}
                  >
                    {winnabilityLabel(row.winnability)}
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
                  {row.cpc != null && (
                    <Badge
                      variant="outline"
                      className="text-[10px] px-1.5 py-0 border-border text-muted-foreground"
                      title="Cost per click — paid-search value proxy for this keyword"
                    >
                      CPC ${row.cpc.toFixed(2)}
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
