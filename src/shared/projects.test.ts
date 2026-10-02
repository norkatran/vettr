import { describe, expect, it } from 'vitest'
import {
  emptyProjectState,
  MAX_RECENT_PROJECTS,
  openProjectState,
  parseProjectState,
  removeProjectState
} from './projects'

describe('openProjectState', () => {
  it('sets the current project and puts it first', () => {
    const a = openProjectState(emptyProjectState, '/a')
    const b = openProjectState(a, '/b')
    expect(b).toEqual({ current: '/b', recent: ['/b', '/a'] })
  })

  it('moves a reopened project to the front without duplicating it', () => {
    let s = openProjectState(emptyProjectState, '/a')
    s = openProjectState(s, '/b')
    s = openProjectState(s, '/a')
    expect(s.recent).toEqual(['/a', '/b'])
  })

  it('caps the recent list', () => {
    let s = emptyProjectState
    for (let i = 0; i < MAX_RECENT_PROJECTS + 5; i++) s = openProjectState(s, `/p${i}`)
    expect(s.recent).toHaveLength(MAX_RECENT_PROJECTS)
    expect(s.recent[0]).toBe(`/p${MAX_RECENT_PROJECTS + 4}`)
  })
})

describe('removeProjectState', () => {
  it('clears current when it is the removed path', () => {
    const s = openProjectState(emptyProjectState, '/a')
    expect(removeProjectState(s, '/a')).toEqual(emptyProjectState)
  })

  it('keeps current when removing another path', () => {
    let s = openProjectState(emptyProjectState, '/a')
    s = openProjectState(s, '/b')
    expect(removeProjectState(s, '/a')).toEqual({ current: '/b', recent: ['/b'] })
  })
})

describe('parseProjectState', () => {
  it('falls back to empty state for bad input', () => {
    expect(parseProjectState(null)).toEqual(emptyProjectState)
    expect(parseProjectState('x')).toEqual(emptyProjectState)
    expect(parseProjectState({ current: 5, recent: 'nope' })).toEqual(emptyProjectState)
  })

  it('drops invalid and duplicate entries', () => {
    expect(parseProjectState({ current: '/a', recent: ['/a', 1, '', '/a', '/b'] })).toEqual({
      current: '/a',
      recent: ['/a', '/b']
    })
  })
})
