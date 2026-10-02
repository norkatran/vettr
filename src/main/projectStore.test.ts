import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

let userData = ''

vi.mock('electron', () => ({ app: { getPath: () => userData } }))

import { forgetProject, getProjectState, loadProjectState, setCurrentProject } from './projectStore'

let root = ''
const stateFile = (): string => join(userData, 'projects.json')
const makeDir = (name: string): string => {
  const dir = join(root, name)
  mkdirSync(dir)
  return dir
}

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'agentide-test-'))
  userData = join(root, 'userData')
  mkdirSync(userData)
})

afterEach(() => {
  rmSync(root, { recursive: true, force: true })
})

describe('projectStore', () => {
  it('starts empty when nothing has been saved', () => {
    loadProjectState()
    expect(getProjectState()).toEqual({ current: null, recent: [] })
  })

  it('persists the current project and recents to disk', () => {
    loadProjectState()
    const a = makeDir('a')
    setCurrentProject(a)
    expect(JSON.parse(readFileSync(stateFile(), 'utf8'))).toEqual({ current: a, recent: [a] })
  })

  it('restores the last opened project after a restart', () => {
    loadProjectState()
    const a = makeDir('a')
    const b = makeDir('b')
    setCurrentProject(a)
    setCurrentProject(b)

    // Simulate a restart: wipe in-memory state by loading from a fresh file read.
    loadProjectState()
    expect(getProjectState()).toEqual({ current: b, recent: [b, a] })
  })

  it('changing project makes the new one the default', () => {
    loadProjectState()
    const a = makeDir('a')
    const b = makeDir('b')
    setCurrentProject(a)
    setCurrentProject(b)
    setCurrentProject(a)
    loadProjectState()
    expect(getProjectState().current).toBe(a)
    expect(getProjectState().recent).toEqual([a, b])
  })

  it('drops folders that no longer exist on load', () => {
    loadProjectState()
    const a = makeDir('a')
    const b = makeDir('b')
    setCurrentProject(a)
    setCurrentProject(b)
    rmSync(b, { recursive: true })

    loadProjectState()
    expect(getProjectState()).toEqual({ current: null, recent: [a] })
  })

  it('forgetProject removes it and persists the change', () => {
    loadProjectState()
    const a = makeDir('a')
    const b = makeDir('b')
    setCurrentProject(a)
    setCurrentProject(b)
    forgetProject(a)
    loadProjectState()
    expect(getProjectState()).toEqual({ current: b, recent: [b] })
  })

  it('recovers from a corrupt state file', () => {
    writeFileSync(stateFile(), '{not json')
    loadProjectState()
    expect(getProjectState()).toEqual({ current: null, recent: [] })
  })

  it('logs and keeps in-memory state when saving fails', () => {
    loadProjectState()
    const a = makeDir('a')
    // A file where the userData directory should be makes mkdir/write fail.
    userData = join(root, 'blocker')
    writeFileSync(userData, '')
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})

    setCurrentProject(a)

    expect(error).toHaveBeenCalled()
    expect(getProjectState()).toEqual({ current: a, recent: [a] })
    error.mockRestore()
  })

  it('clears a missing current project even if it is not in the recent list', () => {
    const a = makeDir('a')
    writeFileSync(stateFile(), JSON.stringify({ current: join(root, 'gone'), recent: [a] }))
    loadProjectState()
    expect(getProjectState()).toEqual({ current: null, recent: [a] })
  })
})
