/// Frontend artifact utilities — shared helpers for JSON extraction, metric formatting,
/// and typed artifact parsing. Keep keyword-specific UI logic in components.

import type { NormalizedArtifact } from './types'

// ─── JSON Extraction ──────────────────────────────────────────────────────────

/** Extract JSON from raw LLM text (mirrors engine/text.rs::extract_json). */
export function extractJsonArtifact(raw: string): NormalizedArtifact {
  const trimmed = raw.trim()

  // 1. Whole text is JSON
  try {
    const parsed = JSON.parse(trimmed)
    return {
      raw_output: raw,
      json_artifact: parsed,
      extraction_method: 'bare_json',
      success: true,
    }
  } catch {
    // continue
  }

  // 2. Fenced code block
  const fencePatterns = ['```json\n', '```json\r\n', '```JSON\n', '```\n']
  for (const pat of fencePatterns) {
    const start = raw.indexOf(pat)
    if (start !== -1) {
      const afterOpen = start + pat.length
      const rest = raw.slice(afterOpen)
      const end = rest.indexOf('```')
      if (end !== -1) {
        const candidate = rest.slice(0, end).trim()
        try {
          const parsed = JSON.parse(candidate)
          return {
            raw_output: raw,
            json_artifact: parsed,
            extraction_method: 'json_block',
            success: true,
          }
        } catch {
          // continue
        }
      }
    }
  }

  // 3. Bare JSON object/array
  const objStart = trimmed.indexOf('{')
  const objEnd = trimmed.lastIndexOf('}')
  if (objStart !== -1 && objEnd > objStart) {
    try {
      const parsed = JSON.parse(trimmed.slice(objStart, objEnd + 1))
      return {
        raw_output: raw,
        json_artifact: parsed,
        extraction_method: 'bare_json',
        success: true,
      }
    } catch {
      // continue
    }
  }
  const arrStart = trimmed.indexOf('[')
  const arrEnd = trimmed.lastIndexOf(']')
  if (arrStart !== -1 && arrEnd > arrStart) {
    try {
      const parsed = JSON.parse(trimmed.slice(arrStart, arrEnd + 1))
      return {
        raw_output: raw,
        json_artifact: parsed,
        extraction_method: 'bare_json',
        success: true,
      }
    } catch {
      // continue
    }
  }

  return {
    raw_output: raw,
    json_artifact: null,
    extraction_method: 'none',
    success: false,
  }
}

/** Extract the raw JSON string from agent output without parsing. */
export function extractJsonString(raw: string): string | null {
  const trimmed = raw.trim()

  // 1. Whole text is JSON
  try {
    JSON.parse(trimmed)
    return trimmed
  } catch {
    // continue
  }

  // 2. Fenced code block
  const fencePatterns = ['```json\n', '```json\r\n', '```JSON\n', '```\n']
  for (const pat of fencePatterns) {
    const start = raw.indexOf(pat)
    if (start !== -1) {
      const afterOpen = start + pat.length
      const rest = raw.slice(afterOpen)
      const end = rest.indexOf('```')
      if (end !== -1) {
        const candidate = rest.slice(0, end).trim()
        try {
          JSON.parse(candidate)
          return candidate
        } catch {
          // continue
        }
      }
    }
  }

  // 3. Bare JSON object
  const objStart = trimmed.indexOf('{')
  const objEnd = trimmed.lastIndexOf('}')
  if (objStart !== -1 && objEnd > objStart) {
    const candidate = trimmed.slice(objStart, objEnd + 1)
    try {
      JSON.parse(candidate)
      return candidate
    } catch {
      // continue
    }
  }

  // 4. Bare JSON array
  const arrStart = trimmed.indexOf('[')
  const arrEnd = trimmed.lastIndexOf(']')
  if (arrStart !== -1 && arrEnd > arrStart) {
    const candidate = trimmed.slice(arrStart, arrEnd + 1)
    try {
      JSON.parse(candidate)
      return candidate
    } catch {
      // continue
    }
  }

  return null
}

// ─── Metric Formatting ────────────────────────────────────────────────────────

/** Parse a metric value that may be string or number, with comma handling. */
export function parseMetric(raw: number | string | null | undefined): number | null {
  if (raw == null) return null
  if (typeof raw === 'number') return Number.isFinite(raw) ? raw : null
  const cleaned = String(raw).replace(/,/g, '').trim()
  if (!cleaned) return null
  const n = Number.parseInt(cleaned, 10)
  return Number.isNaN(n) ? null : n
}

/** Format a number metric for display. */
export function formatMetric(n: number | null): string {
  if (n == null) return '—'
  return n.toLocaleString('en-US')
}

/** Coerce a bigint or string ID to a number safely. */
export function numberFromId(raw: string | number | bigint | null | undefined): number | null {
  if (raw == null) return null
  if (typeof raw === 'number') return Number.isFinite(raw) ? raw : null
  if (typeof raw === 'bigint') {
    const n = Number(raw)
    return Number.isSafeInteger(n) ? n : null
  }
  const n = Number.parseInt(String(raw), 10)
  return Number.isNaN(n) ? null : n
}

// ─── Date Helpers ─────────────────────────────────────────────────────────────

/** Parse an ISO date string and return a locale date string, or fallback. */
export function formatDate(raw: string | null | undefined, fallback = '—'): string {
  if (!raw) return fallback
  const d = new Date(raw)
  return Number.isNaN(d.getTime()) ? fallback : d.toLocaleDateString('en-US')
}

/** Parse a date range like "2024-01-01 to 2024-02-01" into start/end Dates. */
export function parseDateRange(raw: string): { start: Date | null; end: Date | null } {
  const parts = raw.split(/\s+to\s+|\s+-\s+/)
  const start = parts[0] ? new Date(parts[0].trim()) : null
  const end = parts[1] ? new Date(parts[1].trim()) : null
  return {
    start: start && !Number.isNaN(start.getTime()) ? start : null,
    end: end && !Number.isNaN(end.getTime()) ? end : null,
  }
}
