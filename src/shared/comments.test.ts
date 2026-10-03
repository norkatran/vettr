import { describe, expect, it } from 'vitest'
import {
  endsAt,
  formatReview,
  inRange,
  lineNo,
  pendingComments,
  type ReviewComment,
  rangeOf,
  reanchor,
  snapshotLines
} from './comments'
import type { DiffLine, FileChange, RepoChanges } from './diff'

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
  outdated: false,
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

describe('endsAt and outdated comments', () => {
  it('never anchors an outdated comment to a line', () => {
    expect(endsAt(comment({ outdated: true }), 'a.ts', false, 'new', 3)).toBe(false)
  })
})

describe('reanchor', () => {
  const changes = (unstaged: FileChange[], staged: FileChange[] = []): RepoChanges => ({
    staged,
    unstaged
  })
  const shifted = (offset: number, texts = ['TWO', 'three']): FileChange => ({
    ...file,
    hunks: [
      {
        header: '@@',
        lines: texts.map((text, n) => line('add', null, 2 + offset + n, text))
      }
    ]
  })

  it('returns the same array when nothing moved', () => {
    const comments = [comment()]
    expect(reanchor(comments, changes([file]))).toBe(comments)
  })

  it('follows the snapshot to its new line numbers', () => {
    const [c] = reanchor([comment()], changes([shifted(10)]))
    expect(c).toMatchObject({ start: 12, end: 13, outdated: false })
  })

  it('prefers the match nearest the old position', () => {
    const twice: FileChange = {
      ...file,
      hunks: [
        {
          header: '@@',
          lines: [
            line('add', null, 1, 'TWO'),
            line('add', null, 2, 'three'),
            line('add', null, 20, 'TWO'),
            line('add', null, 21, 'three'),
            line('add', null, 40, 'TWO'),
            line('add', null, 41, 'three')
          ]
        }
      ]
    }
    expect(reanchor([comment({ start: 19, end: 20 })], changes([twice]))[0]).toMatchObject({
      start: 20,
      end: 21
    })
  })

  it('does not match lines that are not consecutive', () => {
    const gap = shifted(0, ['TWO'])
    gap.hunks[0]?.lines.push(line('add', null, 9, 'three'))
    expect(reanchor([comment()], changes([gap]))[0]?.outdated).toBe(true)
  })

  it('marks a comment outdated when the text changed, keeping its position', () => {
    const [c] = reanchor([comment()], changes([shifted(0, ['other', 'three'])]))
    expect(c).toMatchObject({ start: 2, end: 3, outdated: true })
  })

  it('marks it outdated when the file left the diff or the snapshot is empty', () => {
    expect(reanchor([comment()], changes([]))[0]?.outdated).toBe(true)
    expect(reanchor([comment({ snapshot: [] })], changes([file]))[0]?.outdated).toBe(true)
  })

  it('revives an outdated comment when its text comes back', () => {
    const [c] = reanchor([comment({ outdated: true })], changes([file]))
    expect(c?.outdated).toBe(false)
  })

  it('follows a file moved between the unstaged and staged diffs', () => {
    expect(reanchor([comment()], changes([], [file]))[0]).toMatchObject({
      staged: true,
      outdated: false
    })
    expect(reanchor([comment({ staged: true })], changes([file]))[0]).toMatchObject({
      staged: false,
      outdated: false
    })
  })

  it('tolerates the same file being absent from the preferred diff only', () => {
    const other = { ...file, path: 'b.ts' }
    expect(reanchor([comment()], changes([other], [file]))[0]?.staged).toBe(true)
  })
})

describe('formatReview with outdated comments', () => {
  it('warns that the line numbers may be stale', () => {
    expect(formatReview([comment({ outdated: true })])).toContain(
      '1. a.ts, lines 2-3 (the code has since changed, so these line numbers may be stale):'
    )
  })
})
