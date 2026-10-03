import { describe, expect, it } from 'vitest'
import { fuzzyFilter, fuzzyScore } from './fuzzy'

describe('fuzzyScore', () => {
  it('returns null when the characters are not a subsequence', () => {
    expect(fuzzyScore('xyz', 'Git: Push')).toBeNull()
    expect(fuzzyScore('hsup', 'push')).toBeNull()
  })

  it('matches case-insensitively', () => {
    expect(fuzzyScore('PUSH', 'git: push')).not.toBeNull()
  })

  it('scores consecutive and word-start matches higher', () => {
    const tight = fuzzyScore('pu', 'Git: Pull') as number
    const loose = fuzzyScore('pu', 'Git: Stash Pop Undo') as number
    expect(tight).toBeGreaterThan(0)
    expect(fuzzyScore('gp', 'Git: Pull') as number).toBeGreaterThan(
      fuzzyScore('gp', 'Digit grep') as number
    )
    expect(tight).toBeGreaterThanOrEqual(loose)
  })
})

describe('fuzzyFilter', () => {
  const items = ['Git: Pull', 'Git: Push', 'Git: Publish Branch']

  it('returns everything for a blank query', () => {
    expect(fuzzyFilter(items, '  ', (s) => s)).toEqual(items)
  })

  it('drops non-matches and ranks the best first', () => {
    expect(fuzzyFilter(items, 'push', (s) => s)[0]).toBe('Git: Push')
    expect(fuzzyFilter(items, 'zzz', (s) => s)).toEqual([])
    expect(fuzzyFilter(items, 'pub', (s) => s)[0]).toBe('Git: Publish Branch')
  })

  it('keeps the original order for equal scores', () => {
    expect(fuzzyFilter(['b1', 'b2'], 'b', (s) => s)).toEqual(['b1', 'b2'])
  })
})
