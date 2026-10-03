import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

let userData = ''

vi.mock('electron', () => ({ app: { getPath: () => userData } }))

import { getSettings, loadSettings, updateSettings } from './settingsStore'

let root = ''
const file = (): string => join(userData, 'settings.json')

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'vettr-test-'))
  userData = join(root, 'userData')
  mkdirSync(userData)
})

afterEach(() => {
  vi.restoreAllMocks()
  rmSync(root, { recursive: true, force: true })
})

describe('settingsStore', () => {
  it('uses defaults when nothing is saved or the file is corrupt', () => {
    loadSettings()
    expect(getSettings()).toEqual({ editorCommand: '' })
    writeFileSync(file(), '{nope')
    loadSettings()
    expect(getSettings()).toEqual({ editorCommand: '' })
  })

  it('persists updates and reloads them', () => {
    expect(updateSettings({ editorCommand: 'zed {file}:{line}' })).toEqual({
      editorCommand: 'zed {file}:{line}'
    })
    expect(JSON.parse(readFileSync(file(), 'utf8'))).toEqual({ editorCommand: 'zed {file}:{line}' })
    loadSettings()
    expect(getSettings().editorCommand).toBe('zed {file}:{line}')
  })

  it('keeps the settings in memory when saving fails', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    rmSync(userData, { recursive: true })
    writeFileSync(userData, 'a file, not a directory')
    expect(updateSettings({ editorCommand: 'x' }).editorCommand).toBe('x')
    expect(error).toHaveBeenCalled()
  })
})
