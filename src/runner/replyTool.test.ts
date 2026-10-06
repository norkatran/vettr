import { describe, expect, it } from 'vitest'
import { formatReview, type ReviewComment } from '../shared/comments'
import { ReplyTracker } from './replyTool'

const comment: ReviewComment = {
  id: 'c1',
  file: 'a.ts',
  staged: false,
  side: 'new',
  start: 1,
  end: 1,
  snapshot: ['x'],
  text: 'Why?',
  round: 1,
  sent: true,
  outdated: false
}

describe('ReplyTracker', () => {
  it('accepts a reply to a comment that was sent', () => {
    const tracker = new ReplyTracker(true)
    tracker.noteMessage(formatReview([comment], 1))
    expect(tracker.reply('c1', 'Done')).toEqual({ text: 'Reply recorded.', isError: false })
  })

  it('refuses an id it has not seen, telling the agent what to do', () => {
    const tracker = new ReplyTracker(true)
    tracker.noteMessage(formatReview([comment], 1))
    const result = tracker.reply('nope', 'Done')
    expect(result.isError).toBe(true)
    expect(result.text).toContain('Unknown comment id nope')
  })

  it('ignores prompts that are not reviews', () => {
    const tracker = new ReplyTracker(true)
    tracker.noteMessage('please fix it')
    expect(tracker.reply('c1', 'x').isError).toBe(true)
  })

  it('does not validate in a resumed session', () => {
    expect(new ReplyTracker(false).reply('anything', 'x').isError).toBe(false)
  })
})
