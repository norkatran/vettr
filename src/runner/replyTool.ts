import { parseReview, type ReplyKind } from '../shared/comments'

/** The server and tool name the SDK exposes to the agent as `mcp__vettr__respond_to_comment`. */
export const REPLY_SERVER = 'vettr'
export const REPLY_TOOL = 'respond_to_comment'

export const REPLY_DESCRIPTION =
  'Reply to one review comment from a <vettr-review> block. Give the comment id and your message. ' +
  'Set kind to "question" if you need more from the reviewer, or "resolved" if you believe you ' +
  'have fixed the issue; the reviewer decides whether the comment is actually resolved.'

export interface ReplyResult {
  text: string
  isError: boolean
}

/**
 * Remembers which comment ids the reviewer has sent, so a reply to an unknown id can be refused and
 * the agent can correct itself. The reply itself does nothing: the app reads the tool call from the
 * event stream.
 */
export class ReplyTracker {
  private readonly known = new Set<string>()

  /**
   * @param validate false for a resumed session: the runner never saw the earlier rounds, so it
   * cannot tell a wrong id from one sent before the resume.
   */
  private readonly validate: boolean

  constructor(validate: boolean) {
    this.validate = validate
  }

  /** Note the comment ids in a prompt, if it is a review. */
  noteMessage(text: string): void {
    for (const c of parseReview(text)?.comments ?? []) this.known.add(c.id)
  }

  reply(commentId: string, _message: string, _kind?: ReplyKind): ReplyResult {
    if (this.validate && !this.known.has(commentId)) {
      return {
        text: `Unknown comment id ${commentId}. Use an id from the <vettr-review> block.`,
        isError: true
      }
    }
    return { text: 'Reply recorded.', isError: false }
  }
}
