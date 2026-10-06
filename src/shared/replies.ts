import { parseReview, REPLY_TOOL_NAME, type ReplyKind, type SentComment } from './comments'
import type { TranscriptItem } from './session'

/** The agent's answer to one review comment, read from a call to the reply tool. */
export interface AgentReply {
  commentId: string
  message: string
  kind: ReplyKind | null
}

/**
 * The reply a transcript item carries, or null if it is not a reply tool call or its input is
 * malformed. A call that failed (for example an unknown comment id) is not a reply.
 */
export function replyOf(item: TranscriptItem): AgentReply | null {
  if (item.kind !== 'tool' || item.name !== REPLY_TOOL_NAME) return null
  if (item.status === 'error') return null
  const input = item.input as { comment_id?: unknown; message?: unknown; kind?: unknown } | null
  if (typeof input?.comment_id !== 'string' || typeof input.message !== 'string') return null
  const kind = input.kind === 'question' || input.kind === 'resolved' ? input.kind : null
  return { commentId: input.comment_id, message: input.message, kind }
}

/** Every reply in the transcript grouped by comment id, in the order the agent made them. */
export function repliesByComment(items: TranscriptItem[]): Map<string, AgentReply[]> {
  const threads = new Map<string, AgentReply[]>()
  for (const item of items) {
    const reply = replyOf(item)
    if (reply) threads.set(reply.commentId, [...(threads.get(reply.commentId) ?? []), reply])
  }
  return threads
}

/** How a reply's kind reads in the UI. */
export const REPLY_KIND_LABEL: Record<ReplyKind, string> = {
  question: 'asks a question',
  resolved: 'believes this is fixed'
}

/** Every comment the transcript shows as sent to the agent, in order (see `parseReview`). */
export function sentComments(items: TranscriptItem[]): SentComment[] {
  return items.flatMap((item) =>
    item.kind === 'user' ? (parseReview(item.text)?.comments ?? []) : []
  )
}
