import { withResolved } from '@shared/resolution'
import { useCallback, useEffect, useState } from 'react'

export interface ResolvedComments {
  /** Ids of the comments (and so threads) the user has resolved in this project. */
  ids: Set<string>
  /** Resolve or reopen a comment; only the user does this, never the agent. */
  set(id: string, resolved: boolean): void
}

/** The comments the user resolved in the open project, kept by the main process across restarts. */
export function useResolvedComments(project: string | null): ResolvedComments {
  const [list, setList] = useState<string[]>([])

  useEffect(() => {
    setList([])
    if (!project) return
    let current = true
    void window.vettr.getResolvedComments(project).then((ids) => {
      if (current) setList(ids)
    })
    return () => {
      current = false
    }
  }, [project])

  const set = useCallback(
    (id: string, resolved: boolean) => {
      if (!project) return
      setList((prev) => withResolved(prev, id, resolved))
      void window.vettr.setCommentResolved(project, id, resolved).then(setList)
    },
    [project]
  )

  return { ids: new Set(list), set }
}
