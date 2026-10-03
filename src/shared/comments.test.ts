import { describe, expect, it } from 'vitest'
import {
  endsAt,
  formatReview,
  inRange,
  lineNo,
  pendingComments,
  type ReviewComment,
  rangeOf,
  snapshotLines
} from './comments'
import type { DiffLine, FileChange } from './diff'

const line = (
  kind: DiffLine['kind'],
  oldNo: number | null,
  newNo: number | null,
  text: string
) => ({
  kind,
  oldNo,
  newNo,
  text
})

const file: FileChange = {
  path: 'a.ts',
  oldPath: null,
  status: 'modified',
  binary: false,
  tooLarge: false,
  additions: 2,
  deletions: 1,
  hunks: [
    {
      header: '@@',
      lines: [
        line('context', 1, 1, 'one'),
        line('del', 2, null, 'two'),
        line('add', null, 2, 'TWO'),
        line('add', null, 3, 'three')
      ]
    }
  ]
}

const comment = (over: Partial<ReviewComment> = {}): ReviewComment => ({
  id: 1,
  file: 'a.ts',
  staged: false,
  side: 'new',
  start: 2,
  end: 3,
  snapshot: ['TWO', 'three'],
  text: 'Why?',
  round: 1,
  sent: false,
  ...over
})

describe('snapshotLines', () => {
  it('quotes the lines of a side within the range', () => {
    expect(snapshotLines(file, 'new', 2, 3)).toEqual(['TWO', 'three'])
    expect(snapshotLines(file, 'old', 1, 2)).toEqual(['one', 'two'])
  })

  it('skips lines outside the range', () => {
    expect(snapshotLines(file, 'new', 3, 3)).toEqual(['three'])
    expect(snapshotLines(file, 'new', 9, 9)).toEqual([])
  })
})

describe('lineNo, rangeOf, inRange', () => {
  it('reads the number for a side', () => {
    expect(lineNo(line('del', 2, null, 'x'), 'old')).toBe(2)
    expect(lineNo(line('add', null, 5, 'x'), 'new')).toBe(5)
  })

  it('orders a range whichever way it was selected', () => {
    expect(rangeOf(3, 5)).toEqual([3, 5])
    expect(rangeOf(5, 3)).toEqual([3, 5])
  })

  it('tests membership inclusively', () => {
    expect(inRange({ start: 2, end: 4 }, 2)).toBe(true)
    expect(inRange({ start: 2, end: 4 }, 4)).toBe(true)
    expect(inRange({ start: 2, end: 4 }, 5)).toBe(false)
    expect(inRange({ start: 2, end: 4 }, 1)).toBe(false)
  })
})

describe('endsAt', () => {
  it('matches only the exact file, group, side and end line', () => {
    const c = comment()
    expect(endsAt(c, 'a.ts', false, 'new', 3)).toBe(true)
    expect(endsAt(c, 'b.ts', false, 'new', 3)).toBe(false)
    expect(endsAt(c, 'a.ts', true, 'new', 3)).toBe(false)
    expect(endsAt(c, 'a.ts', false, 'old', 3)).toBe(false)
    expect(endsAt(c, 'a.ts', false, 'new', 2)).toBe(false)
  })
})

describe('pendingComments', () => {
  it('drops comments already sent', () => {
    const pending = comment({ id: 1 })
    expect(pendingComments([pending, comment({ id: 2, sent: true })])).toEqual([pending])
  })
})

describe('formatReview', () => {
  it('lists each comment with its location, quoted code and text', () => {
    const message = formatReview([
      comment(),
      comment({ side: 'old', start: 2, end: 2, snapshot: ['two'], text: 'Keep this' })
    ])
    expect(message).toBe(
      [
        'Please address these review comments on your changes:',
        '',
        '1. a.ts, lines 2-3:',
        '```\nTWO\nthree\n```',
        'Why?',
        '',
        '2. a.ts, line 2, before your change (removed code):',
        '```\ntwo\n```',
        'Keep this'
      ].join('\n')
    )
  })

  it('uses a longer fence when the code contains backticks', () => {
    const message = formatReview([comment({ snapshot: ['a ```b``` c', '`d`'] })])
    expect(message).toContain('````\na ```b``` c\n`d`\n````')
  })

  it('handles a comment with no visible snapshot', () => {
    expect(formatReview([comment({ snapshot: [] })])).toContain('```\n\n```')
  })
})
