// The slice of the app's review-comment format the runner needs: reading a `<vettr-review>` block
// back out of a prompt. The writer lives in the Rust app (rs/comments.rs); keep the format in sync.

/** Which side of the diff a comment is on: the old file (deleted lines) or the new one. */
export type Side = 'old' | 'new'

/** What the agent says about a comment when it replies: a question for the reviewer, or "fixed". */
export type ReplyKind = 'question' | 'resolved'

/** A comment as read back from a sent review. */
export interface SentComment {
  id: string
  file: string
  side: Side
  start: number
  end: number
  snapshot: string[]
  text: string
  round: number
  outdated: boolean
}

function unescapeXml(text: string): string {
  const named: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', '#13': '\r' }
  return text.replace(/&(amp|lt|gt|quot|#13);/g, (_, name: string) => named[name] as string)
}

function attributes(source: string): Record<string, string> {
  const found: Record<string, string> = {}
  for (const m of source.matchAll(/([\w-]+)="([^"]*)"/g)) {
    found[m[1] as string] = unescapeXml(m[2] as string)
  }
  return found
}

/**
 * Read the comments back out of a message written by the app (rs/comments.rs). Returns null
 * when the text holds no `<vettr-review>` block. Malformed comments inside a block are skipped.
 */
export function parseReview(message: string): { round: number; comments: SentComment[] } | null {
  const block = /<vettr-review round="(\d+)">([\s\S]*?)<\/vettr-review>/.exec(message)
  if (!block) return null
  const round = Number(block[1])
  const comments: SentComment[] = []
  const comment =
    /<comment ([^>]*)>\s*<code>([\s\S]*?)<\/code>\s*<note>([\s\S]*?)<\/note>\s*<\/comment>/g
  for (const m of (block[2] as string).matchAll(comment)) {
    const a = attributes(m[1] as string)
    const range = /^(\d+)(?:-(\d+))?$/.exec(a.lines ?? '')
    if (!a.id || a.file === undefined || (a.side !== 'old' && a.side !== 'new') || !range) continue
    const code = unescapeXml(m[2] as string)
    comments.push({
      id: a.id,
      file: a.file,
      side: a.side,
      start: Number(range[1]),
      end: Number(range[2] ?? range[1]),
      snapshot: code === '' ? [] : code.slice(1, -1).split('\n'),
      text: unescapeXml(m[3] as string),
      round,
      outdated: a.outdated === 'true'
    })
  }
  return { round, comments }
}
