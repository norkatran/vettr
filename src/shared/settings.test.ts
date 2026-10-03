import { describe, expect, it } from 'vitest'
import { defaultSettings, EDITOR_PRESETS, matchPreset, parseSettings } from './settings'

describe('parseSettings', () => {
  it('falls back to defaults for non-objects', () => {
    expect(parseSettings(null)).toEqual(defaultSettings)
    expect(parseSettings('x')).toEqual(defaultSettings)
  })
  it('keeps a trimmed editor command', () => {
    expect(parseSettings({ editorCommand: '  code -g {file}:{line} ' })).toEqual({
      editorCommand: 'code -g {file}:{line}'
    })
  })
  it('ignores a wrongly typed editor command', () => {
    expect(parseSettings({ editorCommand: 3 })).toEqual(defaultSettings)
  })
})

describe('matchPreset', () => {
  it('finds a preset by its command', () => {
    const [first] = EDITOR_PRESETS
    expect(matchPreset(first?.command ?? '')).toBe(first)
  })
  it('returns null for empty or custom commands', () => {
    expect(matchPreset('')).toBeNull()
    expect(matchPreset('nano {file}')).toBeNull()
  })
})
