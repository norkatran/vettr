import type { DiffLine, FileChange, RepoChanges } from './diff'

/** Which side of the diff a comment is on: the old file (deleted lines) or the new one. */
export type Side = 'old' | 'new'

export interface ReviewComment {
  /** Unique for the comment's whole life, including across review rounds (a UUID). */
  id: string
  /** Path in the working tree, as in `FileChange.path`. */
  file: string
  /** Whether it was made on the staged or the unstaged diff; line numbers differ between them. */
  staged: boolean
  side: Side
  /** First and last line of the range (inclusive, `start <= end`) in that side's numbering. */
  start: number
  end: number
  /** The text of the commented lines when the comment was made, used later for re-anchoring. */
  snapshot: string[]
  text: string
  /** The review round it was made in (the first round is 1). */
  round: number
  /** True once it has been sent to the agent. */
  sent: boolean
  /** True when the snapshot can no longer be found in the diff (see `reanchor`). */
  outdated: boolean
}

/** The lines of a file's diff that exist on `side`, in order. */
function sideLines(file: FileChange, side: Side): DiffLine[] {
  return file.hunks
    .flatMap((hunk) => hunk.lines)
    .filter((line) => (side === 'old' ? line.oldNo : line.newNo) !== null)
}

/** The number of `line` on `side`; only called for lines known to have one. */
export const lineNo = (line: DiffLine, side: Side): number =>
  (side === 'old' ? line.oldNo : line.newNo) as number

/** Order two clicked line numbers into a range. */
export const rangeOf = (a: number, b: number): [number, number] => (a <= b ? [a, b] : [b, a])

/** The text of lines `start` to `end` on `side` that are visible in the diff. */
export function snapshotLines(file: FileChange, side: Side, start: number, end: number): string[] {
  return sideLines(file, side)
    .filter((line) => lineNo(line, side) >= start && lineNo(line, side) <= end)
    .map((line) => line.text)
}

/**
 * Whether a comment is anchored at (ends on) this line, which is where it is drawn. Outdated
 * comments are anchored nowhere; they are listed separately.
 */
export const endsAt = (c: ReviewComment, file: string, staged: boolean, side: Side, no: number) =>
  !c.outdated && c.file === file && c.staged === staged && c.side === side && c.end === no

/** Whether a line lies inside the range being selected or commented. */
export const inRange = (range: { start: number; end: number }, no: number): boolean =>
  no >= range.start && no <= range.end

/**
 * Where a snapshot sits in `file` on `side`: the start line of the run of consecutive lines whose
 * text equals the snapshot, nearest to `near` when there are several. Null when there is none (or
 * the snapshot is empty, which proves nothing).
 */
function findSnapshot(
  file: FileChange,
  side: Side,
  snapshot: string[],
  near: number
): number | null {
  const lines = sideLines(file, side)
  let best: number | null = null
  for (let i = 0; i + snapshot.length <= lines.length && snapshot.length > 0; i++) {
    const run = lines.slice(i, i + snapshot.length)
    const first = lineNo(run[0] as DiffLine, side)
    const matches = run.every((l, n) => l.text === snapshot[n] && lineNo(l, side) === first + n)
    if (matches && (best === null || Math.abs(first - near) < Math.abs(best - near))) best = first
  }
  return best
}

/**
 * Re-anchor comments against the current diff by matching their snapshot text, on the same
 * staged/unstaged diff first and then the other (staging a file moves it between them). A match
 * moves the comment to the new line numbers; no match marks it outdated, keeping its last position.
 * Outdated is not sticky: if the text comes back the comment is anchored again. Returns the same
 * array when nothing changed.
 */
export function reanchor(comments: ReviewComment[], changes: RepoChanges): ReviewComment[] {
  let changed = false
  const next = comments.map((c) => {
    let moved: ReviewComment = { ...c, outdated: true }
    for (const staged of c.staged ? [true, false] : [false, true]) {
      const file = (staged ? changes.staged : changes.unstaged).find((f) => f.path === c.file)
      const start = file ? findSnapshot(file, c.side, c.snapshot, c.start) : null
      if (start !== null) {
        moved = { ...c, staged, start, end: start + c.snapshot.length - 1, outdated: false }
        break
      }
    }
    const same =
      moved.staged === c.staged &&
      moved.start === c.start &&
      moved.end === c.end &&
      moved.outdated === c.outdated
    if (same) return c
    changed = true
    return moved
  })
  return changed ? next : comments
}

/** Comments not yet sent to the agent. */
export const pendingComments = (comments: ReviewComment[]): ReviewComment[] =>
  comments.filter((c) => !c.sent)

/** Escape text for use in XML content or a double-quoted attribute (lossless, see `unescapeXml`). */
function escapeXml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/\r/g, '&#13;')
}

function unescapeXml(text: string): string {
  const named: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', '#13': '\r' }
  return text.replace(/&(amp|lt|gt|quot|#13);/g, (_, name: string) => named[name] as string)
}

/** What the agent says about a comment when it replies: a question for the reviewer, or "fixed". */
export type ReplyKind = 'question' | 'resolved'

/** The name the agent calls the reply tool by (server `vettr`, tool `respond_to_comment`). */
export const REPLY_TOOL_NAME = 'mcp__vettr__respond_to_comment'

const REVIEW_INTRO =
  'Please address these review comments on your changes. They are in the <vettr-review> block ' +
  'below. Each <comment> has an id, the file, the side (new is the file as it is now, old is the ' +
  'code before your change) and the line range, followed by the commented <code> and the ' +
  "reviewer's <note>. If a comment is marked outdated, the code has since changed, so its line " +
  'numbers may be stale. Reply to individual comments with the ' +
  `${REPLY_TOOL_NAME} tool (comment_id, message, and optionally kind: question or resolved).`

/**
 * The message sent to the agent: a short introduction and a `<vettr-review>` block with one
 * `<comment>` per comment (id, file, side, line range, the quoted code and the note). Everything
 * the reader needs to rebuild the comments is in the block (see `parseReview`).
 */
export function formatReview(comments: ReviewComment[], round: number): string {
  const entries = comments.map((c) => {
    const lines = c.start === c.end ? `${c.start}` : `${c.start}-${c.end}`
    const outdated = c.outdated ? ' outdated="true"' : ''
    const code = c.snapshot.length === 0 ? '' : `\n${escapeXml(c.snapshot.join('\n'))}\n`
    return [
      `  <comment id="${escapeXml(c.id)}" file="${escapeXml(c.file)}" side="${c.side}" lines="${lines}"${outdated}>`,
      `    <code>${code}</code>`,
      `    <note>${escapeXml(c.text)}</note>`,
      '  </comment>'
    ].join('\n')
  })
  return `${REVIEW_INTRO}\n\n<vettr-review round="${round}">\n${entries.join('\n')}\n</vettr-review>`
}

/** A comment as read back from a sent review: what the message carries, no staging or sent state. */
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

function attributes(source: string): Record<string, string> {
  const found: Record<string, string> = {}
  for (const m of source.matchAll(/([\w-]+)="([^"]*)"/g)) {
    found[m[1] as string] = unescapeXml(m[2] as string)
  }
  return found
}

/**
 * Read the comments back out of a message made by `formatReview`; the inverse of it. Returns null
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

/**
 * Rebuild the sent comments from what the transcript says was sent (a stored session being opened).
 * The sent comments in `current` are replaced; pending ones are kept, moved to the next round. The
 * message does not say whether a comment was on the staged or unstaged diff, so each starts on the
 * unstaged one and `reanchor` finds it on either. A comment sent in several rounds keeps its last
 * text. Returns the comments and the round to continue from.
 */
export function rehydrate(
  current: ReviewComment[],
  sent: SentComment[],
  changes: RepoChanges | null
): { comments: ReviewComment[]; round: number } {
  const latest = new Map<string, SentComment>()
  for (const c of sent) latest.set(c.id, c)
  const round = Math.max(0, ...sent.map((c) => c.round)) + 1
  const restored: ReviewComment[] = [...latest.values()].map((c) => ({
    id: c.id,
    file: c.file,
    staged: false,
    side: c.side,
    start: c.start,
    end: c.end,
    snapshot: c.snapshot,
    text: c.text,
    round: c.round,
    sent: true,
    outdated: c.outdated
  }))
  const pending = pendingComments(current).map((c) => ({ ...c, round }))
  const comments = [...restored, ...pending]
  return { comments: changes ? reanchor(comments, changes) : comments, round }
}
