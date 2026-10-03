import { describe, expect, it } from 'vitest'
import { formatSessionTime, sessionLabel } from './sessions'

const local = new Date(2026, 0, 5, 9, 7).getTime()

describe('formatSessionTime', () => {
  it('pads the local date and time', () => {
    expect(formatSessionTime(local)).toBe('2026-01-05 09:07')
  })
})

describe('sessionLabel', () => {
  it('is [timestamp] title', () => {
    expect(sessionLabel({ id: 'a', title: 'Fix bug', lastModified: local })).toBe(
      '[2026-01-05 09:07] Fix bug'
    )
  })
})
