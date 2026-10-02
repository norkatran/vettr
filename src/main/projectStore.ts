import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import {
  emptyProjectState,
  openProjectState,
  type ProjectState,
  parseProjectState,
  removeProjectState
} from '@shared/projects'
import { app } from 'electron'

let state: ProjectState = emptyProjectState

function filePath(): string {
  return join(app.getPath('userData'), 'projects.json')
}

function save(): void {
  try {
    mkdirSync(dirname(filePath()), { recursive: true })
    writeFileSync(filePath(), JSON.stringify(state, null, 2))
  } catch (err) {
    console.error('Failed to save project state', err)
  }
}

/** Load persisted state, dropping folders that no longer exist. */
export function loadProjectState(): void {
  try {
    state = parseProjectState(JSON.parse(readFileSync(filePath(), 'utf8')))
  } catch {
    state = emptyProjectState
  }
  for (const path of state.recent) {
    if (!existsSync(path)) state = removeProjectState(state, path)
  }
  if (state.current && !existsSync(state.current)) state = { ...state, current: null }
}

export function getProjectState(): ProjectState {
  return state
}

export function setCurrentProject(path: string): void {
  state = openProjectState(state, path)
  save()
}

export function forgetProject(path: string): void {
  state = removeProjectState(state, path)
  save()
}
