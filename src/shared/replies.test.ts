import { describe, expect, it } from 'vitest'
import { formatReview, REPLY_TOOL_NAME, type ReviewComment } from './comments'
import { repliesByComment, replyOf, sentComments } from './replies'
import type { TranscriptItem } from './session'

const call = (input: unknown, status: 'done' | 'error' = 'done', name = REPLY_TOOL_NAME) =>
  ({ kind: 'tool', id: 't', name, input, status, output: '' }) as TranscriptItem

describe('replyOf', () => {
  it('reads a reply and its kind', () => {
    expect(replyOf(call({ comment_id: 'c1', message: 'Done', kind: 'resolved' }))).toEqual({
      commentId: 'c1',
      message: 'Done',
      kind: 'resolved'
    })
  })

  it('treats a missing or unknown kind as a plain reply', () => {
    expect(replyOf(call({ comment_id: 'c1', message: 'x' }))?.kind).toBeNull()
    expect(replyOf(call({ comment_id: 'c1', message: 'x', kind: 'wat' }))?.kind).toBeNull()
  })

  it('ignores other tools, failed calls and malformed input', () => {
    expect(replyOf({ kind: 'text', text: 'hi' })).toBeNull()
    expect(replyOf(call({ comment_id: 'c1', message: 'x' }, 'done', 'Bash'))).toBeNull()
    expect(replyOf(call({ comment_id: 'c1', message: 'x' }, 'error'))).toBeNull()
    expect(replyOf(call({ comment_id: 1, message: 'x' }))).toBeNull()
    expect(replyOf(call(null))).toBeNull()
  })
})

describe('repliesByComment', () => {
  it('groups replies by comment in order', () => {
    const threads = repliesByComment([
      call({ comment_id: 'a', message: '1' }),
      { kind: 'text', text: 'between' },
      call({ comment_id: 'b', message: '2' }),
      call({ comment_id: 'a', message: '3', kind: 'question' })
    ])
    expect(threads.get('a')?.map((r) => r.message)).toEqual(['1', '3'])
    expect(threads.get('b')).toHaveLength(1)
  })
})

describe('sentComments', () => {
  it('collects the comments of every review the user sent', () => {
    const base: Omit<ReviewComment, 'id' | 'file' | 'text' | 'round'> = {
      staged: false,
      side: 'new',
      start: 1,
      end: 1,
      snapshot: [],
      sent: true,
      outdated: false
    }
    const review = (id: string, round: number) =>
      formatReview([{ ...base, id, file: 'a.ts', text: id, round }], round)
    const found = sentComments([
      { kind: 'user', text: 'hello' },
      { kind: 'user', text: review('c1', 1) },
      { kind: 'text', text: review('nope', 1) },
      { kind: 'user', text: review('c2', 2) }
    ])
    expect(found.map((c) => [c.id, c.round])).toEqual([
      ['c1', 1],
      ['c2', 2]
    ])
  })
})
