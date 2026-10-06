import { describe, expect, it } from 'vitest'
import {
  endsAt,
  formatReview,
  inRange,
  lineNo,
  parseReview,
  pendingComments,
  type ReviewComment,
  rangeOf,
  reanchor,
  rehydrate,
  type SentComment,
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
  id: 'c1',
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
    const pending = comment({ id: 'c1' })
    expect(pendingComments([pending, comment({ id: 'c2', sent: true })])).toEqual([pending])
  })
})

describe('formatReview', () => {
  it('wraps the comments in a vettr-review block with the round', () => {
    const message = formatReview(
      [
        comment(),
        comment({ id: 'c2', side: 'old', start: 2, end: 2, snapshot: ['two'], text: 'Keep this' })
      ],
      2
    )
    expect(message).toContain(
      [
        '<vettr-review round="2">',
        '  <comment id="c1" file="a.ts" side="new" lines="2-3">',
        '    <code>\nTWO\nthree\n</code>',
        '    <note>Why?</note>',
        '  </comment>',
        '  <comment id="c2" file="a.ts" side="old" lines="2">',
        '    <code>\ntwo\n</code>',
        '    <note>Keep this</note>',
        '  </comment>',
        '</vettr-review>'
      ].join('\n')
    )
  })

  it('introduces the format before the block', () => {
    expect(formatReview([comment()], 1).startsWith('Please address these review comments')).toBe(
      true
    )
  })

  it('marks outdated comments', () => {
    expect(formatReview([comment({ outdated: true })], 1)).toContain('lines="2-3" outdated="true">')
  })

  it('escapes markup in code, notes and attributes', () => {
    const message = formatReview(
      [comment({ file: 'a"<b>.ts', snapshot: ['x < y && </code>'], text: '</note> & "q"' })],
      1
    )
    expect(message).toContain('file="a&quot;&lt;b&gt;.ts"')
    expect(message).toContain('x &lt; y &amp;&amp; &lt;/code&gt;')
    expect(message).toContain(
      '<note>&lt;/note&gt; &amp; "q"</note>'.replace('"q"', '&quot;q&quot;')
    )
  })
})

describe('parseReview', () => {
  const roundTrip = (comments: ReviewComment[], round = 3) => {
    const parsed = parseReview(formatReview(comments, round))
    return parsed?.comments
  }

  it('returns null when there is no review block', () => {
    expect(parseReview('just a message')).toBeNull()
  })

  it('reads back what formatReview wrote', () => {
    const c = comment({ outdated: true })
    expect(roundTrip([c])).toEqual([
      {
        id: 'c1',
        file: 'a.ts',
        side: 'new',
        start: 2,
        end: 3,
        snapshot: ['TWO', 'three'],
        text: 'Why?',
        round: 3,
        outdated: true
      }
    ])
  })

  it('reads a single line and the round', () => {
    const parsed = parseReview(formatReview([comment({ start: 5, end: 5 })], 4))
    expect(parsed?.round).toBe(4)
    expect(parsed?.comments[0]).toMatchObject({ start: 5, end: 5 })
  })

  it('round-trips hostile text exactly', () => {
    const nasty = comment({
      file: 'we"ird <&>.ts',
      snapshot: ['</code></comment>', '', '  ]]> & &amp; &lt;', 'tab\there\r'],
      text: '\n  </note></vettr-review> & "quoted" &amp;\r\nlast\n'
    })
    const [back] = roundTrip([nasty]) ?? []
    expect(back).toMatchObject({ file: nasty.file, snapshot: nasty.snapshot, text: nasty.text })
  })

  it('tells no snapshot apart from one blank line', () => {
    expect(roundTrip([comment({ snapshot: [] })])?.[0]?.snapshot).toEqual([])
    expect(roundTrip([comment({ snapshot: [''] })])?.[0]?.snapshot).toEqual([''])
  })

  it('finds the block amid other text and skips malformed comments', () => {
    const message = `before\n${formatReview([comment()], 1)}\nafter`
    expect(parseReview(message)?.comments).toHaveLength(1)
    const broken =
      '<vettr-review round="1"><comment id="x" file="f" side="mid" lines="1"><code></code><note>n</note></comment></vettr-review>'
    expect(parseReview(broken)?.comments).toEqual([])
    const wrap = (attrs: string) =>
      `<vettr-review round="1"><comment ${attrs}><code></code><note>n</note></comment></vettr-review>`
    for (const attrs of [
      'file="f" side="new" lines="1"',
      'id="x" side="new" lines="1"',
      'id="x" file="f" lines="1"',
      'id="x" file="f" side="new"',
      'id="x" file="f" side="new" lines="a-b"'
    ]) {
      expect(parseReview(wrap(attrs))?.comments).toEqual([])
    }
    expect(parseReview(wrap('id="x" file="f" side="new" lines="1"'))?.comments).toHaveLength(1)
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

describe('rehydrate', () => {
  const sent = (id: string, round: number, text = id): SentComment => ({
    id,
    file: 'a.ts',
    side: 'new',
    start: 2,
    end: 3,
    snapshot: ['TWO', 'three'],
    text,
    round,
    outdated: false
  })
  const none: RepoChanges = { staged: [], unstaged: [file] }

  it('restores sent comments, anchored where their snapshot is', () => {
    const { comments, round } = rehydrate([], [sent('c1', 1), sent('c2', 2)], none)
    expect(comments.map((c) => [c.id, c.sent, c.outdated, c.start])).toEqual([
      ['c1', true, false, 2],
      ['c2', true, false, 2]
    ])
    expect(round).toBe(3)
  })

  it('finds a comment that is now on the staged diff', () => {
    const { comments } = rehydrate([], [sent('c1', 1)], { staged: [file], unstaged: [] })
    expect(comments[0]).toMatchObject({ staged: true, outdated: false })
  })

  it('marks a comment outdated when its code is gone', () => {
    const { comments } = rehydrate([], [sent('c1', 1)], { staged: [], unstaged: [] })
    expect(comments[0]?.outdated).toBe(true)
  })

  it('replaces old sent comments, keeps pending ones and moves them to the new round', () => {
    const current = [comment({ id: 'old', sent: true }), comment({ id: 'draft', round: 1 })]
    const { comments, round } = rehydrate(current, [sent('c1', 4)], none)
    expect(comments.map((c) => c.id)).toEqual(['c1', 'draft'])
    expect(comments[1]?.round).toBe(round)
    expect(round).toBe(5)
  })

  it('keeps the last text of a comment sent more than once', () => {
    const { comments } = rehydrate([], [sent('c1', 1, 'first'), sent('c1', 2, 'second')], none)
    expect(comments).toHaveLength(1)
    expect(comments[0]).toMatchObject({ text: 'second', round: 2 })
  })

  it('starts at round 1 with nothing sent, and skips anchoring without changes', () => {
    expect(rehydrate([], [], none).round).toBe(1)
    expect(rehydrate([], [sent('c1', 1)], null).comments[0]?.outdated).toBe(false)
  })
})
