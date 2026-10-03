import type { FileChange, RepoChanges } from '@shared/diff'
import { useEffect, useState } from 'react'

export interface ChangesState {
  /** Null until the first load finishes, or when there is no project or it cannot be read. */
  changes: RepoChanges | null
  /**
   * Staged then unstaged files in one list (null when `changes` is). A partially staged file
   * appears twice. Interim until the Changes view shows the two groups separately.
   */
  files: FileChange[] | null
  loading: boolean
}

const EMPTY: ChangesState = { changes: null, files: null, loading: false }

const loaded = (changes: RepoChanges | null): ChangesState => ({
  changes,
  files: changes && [...changes.staged, ...changes.unstaged],
  loading: false
})

/** Working-tree changes for `project`, reloaded when the window regains focus. */
export function useChanges(project: string | null): ChangesState {
  const [state, setState] = useState<ChangesState>(EMPTY)

  useEffect(() => {
    if (!project) {
      setState(EMPTY)
      return
    }
    let stale = false
    setState({ ...EMPTY, loading: true })
    const refresh = (): void => {
      void window.agentide.getChanges(project).then((changes) => {
        if (!stale) setState(loaded(changes))
      })
    }
    refresh()
    // The main process watches the tree; focus covers events missed while the watcher was down.
    window.addEventListener('focus', refresh)
    const unsubscribe = window.agentide.onRepoChanged(refresh)
    return () => {
      stale = true
      unsubscribe()
      window.removeEventListener('focus', refresh)
    }
  }, [project])

  return state
}

/**
 * What changed in `project` since the round baseline `tree` (from `snapshotTree`), reloaded with
 * the same triggers as `useChanges`. Null while loading, when there is no baseline, or on failure.
 */
export function useChangesSince(project: string | null, tree: string | null): FileChange[] | null {
  const [files, setFiles] = useState<FileChange[] | null>(null)

  useEffect(() => {
    setFiles(null)
    if (!project || !tree) return
    let stale = false
    const refresh = (): void => {
      void window.agentide.getChangesSince(project, tree).then((next) => {
        if (!stale) setFiles(next)
      })
    }
    refresh()
    window.addEventListener('focus', refresh)
    const unsubscribe = window.agentide.onRepoChanged(refresh)
    return () => {
      stale = true
      unsubscribe()
      window.removeEventListener('focus', refresh)
    }
  }, [project, tree])

  return files
}
