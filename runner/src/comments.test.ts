import { describe, expect, it } from 'vitest'
import { parseReview } from './comments'

const escapeXml = (text: string) =>
  text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/\r/g, '&#13;')

interface Spec {
  id?: string
  file?: string
  lines?: string
  snapshot?: string[]
  text?: string
  outdated?: boolean
}

/** Writes a review block the way the app does (see rs/comments.rs). */
function review(specs: Spec[], round = 3): string {
  const entries = specs.map((c) => {
    const outdated = c.outdated ? ' outdated="true"' : ''
    const snapshot = c.snapshot ?? ['TWO', 'three']
    const code = snapshot.length === 0 ? '' : `\n${escapeXml(snapshot.join('\n'))}\n`
    return [
      `  <comment id="${escapeXml(c.id ?? 'c1')}" file="${escapeXml(c.file ?? 'a.ts')}" side="new" lines="${c.lines ?? '2-3'}"${outdated}>`,
      `    <code>${code}</code>`,
      `    <note>${escapeXml(c.text ?? 'Why?')}</note>`,
      '  </comment>'
    ].join('\n')
  })
  return `intro\n\n<vettr-review round="${round}">\n${entries.join('\n')}\n</vettr-review>`
}

describe('parseReview', () => {
  const roundTrip = (specs: Spec[], round = 3) => parseReview(review(specs, round))?.comments

  it('returns null when there is no review block', () => {
    expect(parseReview('just a message')).toBeNull()
  })

  it('reads the comments of a review', () => {
    expect(roundTrip([{ outdated: true }])).toEqual([
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
    const parsed = parseReview(review([{ lines: '5' }], 4))
    expect(parsed?.round).toBe(4)
    expect(parsed?.comments[0]).toMatchObject({ start: 5, end: 5 })
  })

  it('round-trips hostile text exactly', () => {
    const nasty = {
      file: 'we"ird <&>.ts',
      snapshot: ['</code></comment>', '', '  ]]> & &amp; &lt;', 'tab\there\r'],
      text: '\n  </note></vettr-review> & "quoted" &amp;\r\nlast\n'
    }
    const [back] = roundTrip([nasty]) ?? []
    expect(back).toMatchObject(nasty)
  })

  it('tells no snapshot apart from one blank line', () => {
    expect(roundTrip([{ snapshot: [] }])?.[0]?.snapshot).toEqual([])
    expect(roundTrip([{ snapshot: [''] }])?.[0]?.snapshot).toEqual([''])
  })

  it('finds the block amid other text and skips malformed comments', () => {
    expect(parseReview(`before\n${review([{}])}\nafter`)?.comments).toHaveLength(1)
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
