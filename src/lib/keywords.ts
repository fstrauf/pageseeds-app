/// Keyword metric helpers — shared between KeywordPicker, KeywordResearch, etc.

import { parseMetric, formatMetric } from './artifacts'
export { parseMetric, formatMetric }

/** Normalize a raw KD value to a number or null. */
export function kdValue(raw: number | string | null | undefined): number | null {
  if (raw == null) return null
  const n = typeof raw === 'number' ? raw : parseInt(String(raw), 10)
  return isNaN(n) ? null : n
}

/** Human-readable KD label. */
export function kdLabel(kd: number | null): string {
  if (kd == null) return '—'
  if (kd < 10) return 'Very Easy'
  if (kd < 30) return 'Easy'
  if (kd < 50) return 'Medium'
  if (kd < 70) return 'Hard'
  return 'Very Hard'
}

/** Tailwind badge classes for a KD value. */
export function kdColor(kd: number | null): string {
  if (kd == null) return 'bg-secondary text-secondary-foreground border-transparent'
  if (kd < 10) return 'bg-emerald-100 text-emerald-700 border-transparent'
  if (kd < 30) return 'bg-green-100 text-green-700 border-transparent'
  if (kd < 50) return 'bg-amber-100 text-amber-700 border-transparent'
  if (kd < 70) return 'bg-orange-100 text-orange-700 border-transparent'
  return 'bg-red-100 text-red-700 border-transparent'
}

/** Row-shaped input for keyword scoring (used by KeywordPicker and friends). */
export interface KeywordRow {
  keyword: string
  difficulty: number | null
  volume: number | null
  traffic: number | null
  shortage: number | null
  has_data: boolean
  serp_count?: number
  intent?: string | null
  intent_confidence?: number | null
  winnability?: string | null
  winnability_reason?: string | null
  /** Cost per click in USD (DataForSEO landing research). */
  cpc?: number | null
}

/**
 * Winnability penalty folded into the opportunity score, mirroring the
 * backend bucket ordering (target → differentiate → avoid). The raw score is
 * 0–100, so the avoid penalty (150) exceeds the entire raw range: a keyword
 * that survived the backend trim with bucket "avoid" can never top the
 * picker list regardless of its volume/KD. The differentiate penalty (25)
 * keeps those rows below otherwise-equal targets without hiding them.
 */
const WINNABILITY_PENALTY: Record<string, number> = {
  differentiate: 25,
  avoid: 150,
}

/** Compute an opportunity score (0–100) from keyword metrics. */
export function opportunityScore(row: KeywordRow): number {
  const kd = row.difficulty
  const kdScore = kd == null ? 40 : Math.max(0, 100 - kd)
  // Use traffic or volume only — never use shortage as a traffic proxy.
  const trafficSignal = Math.max(0, row.traffic ?? row.volume ?? 0)
  const trafficScore = Math.min(100, Math.log10(trafficSignal + 1) * 25)
  const raw = kdScore * 0.6 + trafficScore * 0.4
  const penalty = WINNABILITY_PENALTY[row.winnability?.toLowerCase() ?? ''] ?? 0
  return Math.max(0, raw - penalty)
}

/** Classify a keyword row's opportunity score into a tier. */
export function opportunityTier(row: KeywordRow): 'High' | 'Medium' | 'Low' {
  const score = opportunityScore(row)
  if (score >= 70) return 'High'
  if (score >= 45) return 'Medium'
  return 'Low'
}

/** Tailwind badge classes for an opportunity tier. */
export function opportunityTierClass(tier: 'High' | 'Medium' | 'Low'): string {
  if (tier === 'High') return 'bg-emerald-100 text-emerald-700 border-transparent'
  if (tier === 'Medium') return 'bg-amber-100 text-amber-700 border-transparent'
  return 'bg-slate-100 text-slate-700 border-transparent'
}
