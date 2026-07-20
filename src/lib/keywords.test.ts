import { describe, it, expect } from 'vitest'
import { opportunityScore, opportunityTier, type KeywordRow } from './keywords'

function row(overrides: Partial<KeywordRow>): KeywordRow {
  return {
    keyword: 'kw',
    difficulty: 20,
    volume: 1000,
    traffic: null,
    shortage: null,
    has_data: true,
    ...overrides,
  }
}

describe('opportunityScore winnability penalty', () => {
  it('leaves scores unchanged when winnability is missing, null, or target', () => {
    const base = row({})
    const unscored = opportunityScore(base)
    expect(opportunityScore(row({ winnability: undefined }))).toBe(unscored)
    expect(opportunityScore(row({ winnability: null }))).toBe(unscored)
    expect(opportunityScore(row({ winnability: 'target' }))).toBe(unscored)
    expect(opportunityScore(row({ winnability: 'unexpected-bucket' }))).toBe(unscored)
  })

  it('ranks otherwise-equal rows target > differentiate > avoid', () => {
    const target = opportunityScore(row({ winnability: 'target' }))
    const differentiate = opportunityScore(row({ winnability: 'differentiate' }))
    const avoid = opportunityScore(row({ winnability: 'avoid' }))
    expect(target).toBeGreaterThan(differentiate)
    expect(differentiate).toBeGreaterThan(avoid)
  })

  it('is case-insensitive about the bucket value', () => {
    expect(opportunityScore(row({ winnability: 'Avoid' }))).toBe(
      opportunityScore(row({ winnability: 'avoid' })),
    )
  })

  it('never lets a surviving avoid top the list, whatever its volume/KD', () => {
    // Best-possible avoid: max volume, KD 0 → clamped to 0. Any non-avoid with
    // a positive raw score ranks above it.
    const avoidScore = opportunityScore(row({ winnability: 'avoid', difficulty: 0, volume: 10_000_000 }))
    const weakTarget = opportunityScore(row({ winnability: 'target', difficulty: 95, volume: 1 }))
    const weakUnscored = opportunityScore(row({ difficulty: 95, volume: 1 }))
    expect(weakTarget).toBeGreaterThan(0)
    expect(avoidScore).toBeLessThan(weakTarget)
    expect(avoidScore).toBeLessThan(weakUnscored)
  })

  it('stays within the documented 0–100 range', () => {
    expect(opportunityScore(row({ winnability: 'avoid', difficulty: 100, volume: 0 }))).toBe(0)
    expect(opportunityScore(row({ difficulty: 0, volume: 10_000_000 }))).toBeLessThanOrEqual(100)
  })

  it('keeps tier display coherent: avoid collapses to Low, differentiate shifts at most one tier', () => {
    expect(opportunityTier(row({ winnability: 'avoid', difficulty: 0, volume: 10_000_000 }))).toBe('Low')
    // A strong differentiate keyword remains selectable (not Low).
    expect(opportunityTier(row({ winnability: 'differentiate', difficulty: 0, volume: 10_000 }))).not.toBe('Low')
  })
})
