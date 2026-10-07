import { describe, expect, it } from 'vitest'
import { ReplyTracker } from './replyTool'

const review = (id: string) =>
  `Please address these.\n\n<vettr-review round="1">\n  <comment id="${id}" file="a.ts" side="new" lines="1">\n    <code>\nx\n</code>\n    <note>Why?</note>\n  </comment>\n</vettr-review>`

describe('ReplyTracker', () => {
  it('accepts a reply to a comment that was sent', () => {
    const tracker = new ReplyTracker(true)
    tracker.noteMessage(review('c1'))
    expect(tracker.reply('c1', 'Done')).toEqual({ text: 'Reply recorded.', isError: false })
  })

  it('refuses an id it has not seen, telling the agent what to do', () => {
    const tracker = new ReplyTracker(true)
    tracker.noteMessage(review('c1'))
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
