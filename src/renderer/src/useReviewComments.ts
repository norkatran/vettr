import { type ReviewComment, reanchor } from '@shared/comments'
import type { RepoChanges } from '@shared/diff'
import { useCallback, useEffect, useState } from 'react'

export type NewComment = Omit<ReviewComment, 'id' | 'round' | 'sent' | 'outdated'>

export interface Review {
  comments: ReviewComment[]
  /** The current review round, starting at 1 and advancing each time comments are sent. */
  round: number
  add(comment: NewComment): void
  edit(id: number, text: string): void
  remove(id: number): void
  /** Mark the given comments as sent and start the next round. */
  markSent(ids: number[]): void
}

/**
 * The review comments for the open project. They live above the views (like the agent session)
 * so switching views keeps them, and reset when another project is opened. Whenever the changes
 * reload, comments are re-anchored to the new diff or marked outdated.
 */
export function useReviewComments(project: string | null, changes: RepoChanges | null): Review {
  const [comments, setComments] = useState<ReviewComment[]>([])
  const [round, setRound] = useState(1)
  const [nextId, setNextId] = useState(1)

  useEffect(() => {
    setComments([])
    setRound(1)
    setNextId(1)
  }, [project])

  useEffect(() => {
    if (changes) setComments((prev) => reanchor(prev, changes))
  }, [changes])

  const add = useCallback(
    (comment: NewComment) => {
      setComments((prev) => [
        ...prev,
        { ...comment, id: nextId, round, sent: false, outdated: false }
      ])
      setNextId((n) => n + 1)
    },
    [nextId, round]
  )
  const edit = useCallback(
    (id: number, text: string) =>
      setComments((prev) => prev.map((c) => (c.id === id ? { ...c, text } : c))),
    []
  )
  const remove = useCallback(
    (id: number) => setComments((prev) => prev.filter((c) => c.id !== id)),
    []
  )
  const markSent = useCallback((ids: number[]) => {
    setComments((prev) => prev.map((c) => (ids.includes(c.id) ? { ...c, sent: true } : c)))
    setRound((r) => r + 1)
  }, [])

  return { comments, round, add, edit, remove, markSent }
}
