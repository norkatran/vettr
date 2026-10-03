import type { FileChange } from '@shared/diff'
import { useEffect, useState } from 'react'

export interface ChangesState {
  /** Null until the first load finishes, or when there is no project or it cannot be read. */
  files: FileChange[] | null
  loading: boolean
}

/** Working-tree changes for `project`, reloaded when the window regains focus. */
export function useChanges(project: string | null): ChangesState {
  const [state, setState] = useState<ChangesState>({ files: null, loading: false })

  useEffect(() => {
    if (!project) {
      setState({ files: null, loading: false })
      return
    }
    let stale = false
    setState({ files: null, loading: true })
    const refresh = (): void => {
      void window.agentide.getChanges(project).then((files) => {
        if (!stale) setState({ files, loading: false })
      })
    }
    refresh()
    // No watcher yet: edits made while the app is in the background show up on return.
    window.addEventListener('focus', refresh)
    return () => {
      stale = true
      window.removeEventListener('focus', refresh)
    }
  }, [project])

  return state
}
