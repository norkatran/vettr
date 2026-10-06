import { describe, expect, it } from 'vitest'
import { parseResolved, withResolved } from './resolution'

describe('parseResolved', () => {
  it('keeps the string ids once each', () => {
    expect(parseResolved(['a', 'b', 'a'])).toEqual(['a', 'b'])
  })

  it('drops anything else', () => {
    expect(parseResolved(['a', 1, null, ''])).toEqual(['a'])
    expect(parseResolved({ not: 'a list' })).toEqual([])
    expect(parseResolved(undefined)).toEqual([])
  })
})

describe('withResolved', () => {
  it('adds and removes an id', () => {
    expect(withResolved(['a'], 'b', true)).toEqual(['a', 'b'])
    expect(withResolved(['a', 'b'], 'a', false)).toEqual(['b'])
  })

  it('returns the same array when nothing changes', () => {
    const ids = ['a']
    expect(withResolved(ids, 'a', true)).toBe(ids)
    expect(withResolved(ids, 'z', false)).toBe(ids)
  })
})
