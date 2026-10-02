/** Persisted project state: the current project plus recently opened ones. */
export interface ProjectState {
  current: string | null
  /** Most recent first, always includes `current` when set. */
  recent: string[]
}

export const MAX_RECENT_PROJECTS = 10

export const emptyProjectState: ProjectState = { current: null, recent: [] }

/** Make `path` the current project and move it to the front of the recent list. */
export function openProjectState(state: ProjectState, path: string): ProjectState {
  const recent = [path, ...state.recent.filter((p) => p !== path)].slice(0, MAX_RECENT_PROJECTS)
  return { current: path, recent }
}

/** Forget a path (for example a folder that no longer exists). */
export function removeProjectState(state: ProjectState, path: string): ProjectState {
  return {
    current: state.current === path ? null : state.current,
    recent: state.recent.filter((p) => p !== path)
  }
}

/** Validate untrusted JSON read from disk, falling back to empty state. */
export function parseProjectState(raw: unknown): ProjectState {
  if (typeof raw !== 'object' || raw === null) return emptyProjectState
  const { current, recent } = raw as Record<string, unknown>
  const list = Array.isArray(recent)
    ? recent.filter((p): p is string => typeof p === 'string' && p !== '')
    : []
  const unique = [...new Set(list)].slice(0, MAX_RECENT_PROJECTS)
  return {
    current: typeof current === 'string' && current !== '' ? current : null,
    recent: unique
  }
}
