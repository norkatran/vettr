import { type ReviewComment, reanchor, rehydrate, type SentComment } from '@shared/comments'
import type { RepoChanges } from '@shared/diff'
import { useCallback, useEffect, useRef, useState } from 'react'

export type NewComment = Omit<ReviewComment, 'id' | 'round' | 'sent' | 'outdated'>

export interface Review {
  comments: ReviewComment[]
  /** The current review round, starting at 1 and advancing each time comments are sent. */
  round: number
  add(comment: NewComment): void
  edit(id: string, text: string): void
  remove(id: string): void
  /**
   * The working tree recorded when the last review was sent (a git tree id), the baseline for
   * "changes since the last review"; null before the first send.
   */
  baseline: string | null
  /** Mark the given comments as sent, record the round's baseline tree and start the next round. */
  markSent(ids: string[], baseline: string | null): void
  /**
   * Replace the sent comments with those read from an opened session's transcript, re-anchored to
   * the current changes, and continue from the round after the last one sent. The baseline of the
   * old session no longer applies.
   */
  restore(sent: SentComment[]): void
}

/**
 * The review comments for the open project. They live above the views (like the agent session)
 * so switching views keeps them, and reset when another project is opened. Whenever the changes
 * reload, comments are re-anchored to the new diff or marked outdated.
 */
export function useReviewComments(project: string | null, changes: RepoChanges | null): Review {
  const [comments, setComments] = useState<ReviewComment[]>([])
  const [round, setRound] = useState(1)
  const [baseline, setBaseline] = useState<string | null>(null)
  const changesRef = useRef(changes)
  changesRef.current = changes
  const commentsRef = useRef(comments)
  commentsRef.current = comments

  useEffect(() => {
    setComments([])
    setRound(1)
    setBaseline(null)
  }, [project])

  useEffect(() => {
    if (changes) setComments((prev) => reanchor(prev, changes))
  }, [changes])

  const add = useCallback(
    (comment: NewComment) => {
      setComments((prev) => [
        ...prev,
        { ...comment, id: crypto.randomUUID(), round, sent: false, outdated: false }
      ])
    },
    [round]
  )
  const edit = useCallback(
    (id: string, text: string) =>
      setComments((prev) => prev.map((c) => (c.id === id ? { ...c, text } : c))),
    []
  )
  const remove = useCallback(
    (id: string) => setComments((prev) => prev.filter((c) => c.id !== id)),
    []
  )
  const markSent = useCallback((ids: string[], tree: string | null) => {
    if (tree) setBaseline(tree)
    setComments((prev) => prev.map((c) => (ids.includes(c.id) ? { ...c, sent: true } : c)))
    setRound((r) => r + 1)
  }, [])

  const restore = useCallback((sent: SentComment[]) => {
    const restored = rehydrate(commentsRef.current, sent, changesRef.current)
    setComments(restored.comments)
    setRound(restored.round)
    setBaseline(null)
  }, [])

  return { comments, round, baseline, add, edit, remove, markSent, restore }
}
