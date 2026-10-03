import { describe, expect, it } from 'vitest'
import { otherTheme, parseThemeChoice, resolveTheme } from './theme'

describe('resolveTheme', () => {
  it('follows the OS when there is no explicit choice', () => {
    expect(resolveTheme(null, true)).toBe('dark')
    expect(resolveTheme(null, false)).toBe('light')
  })

  it('prefers an explicit choice over the OS', () => {
    expect(resolveTheme('light', true)).toBe('light')
    expect(resolveTheme('dark', false)).toBe('dark')
  })
})

describe('parseThemeChoice', () => {
  it('accepts the two themes and rejects anything else', () => {
    expect(parseThemeChoice('light')).toBe('light')
    expect(parseThemeChoice('dark')).toBe('dark')
    expect(parseThemeChoice('solarized')).toBeNull()
    expect(parseThemeChoice(null)).toBeNull()
  })
})

describe('otherTheme', () => {
  it('flips between light and dark', () => {
    expect(otherTheme('dark')).toBe('light')
    expect(otherTheme('light')).toBe('dark')
  })
})
