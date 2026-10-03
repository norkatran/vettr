import type { DiffLine, FileChange, RepoChanges } from './diff'

/** Which side of the diff a comment is on: the old file (deleted lines) or the new one. */
export type Side = 'old' | 'new'

export interface ReviewComment {
  id: number
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

/** A code fence long enough that the quoted code cannot close it early. */
function fenceFor(lines: string[]): string {
  const longest = Math.max(
    0,
    ...lines.flatMap((l) => [...l.matchAll(/`+/g)].map((m) => m[0].length))
  )
  return '`'.repeat(Math.max(3, longest + 1))
}

/**
 * The structured message sent to the agent: one numbered entry per comment with the file, line
 * range and side, the quoted code and the comment text.
 */
export function formatReview(comments: ReviewComment[]): string {
  const entries = comments.map((c, i) => {
    const lines = c.start === c.end ? `line ${c.start}` : `lines ${c.start}-${c.end}`
    const where = c.side === 'old' ? `${lines}, before your change (removed code)` : `${lines}`
    const fence = fenceFor(c.snapshot)
    const note = c.outdated
      ? ' (the code has since changed, so these line numbers may be stale)'
      : ''
    return [
      `${i + 1}. ${c.file}, ${where}${note}:`,
      `${fence}\n${c.snapshot.join('\n')}\n${fence}`,
      c.text
    ].join('\n')
  })
  return `Please address these review comments on your changes:\n\n${entries.join('\n\n')}`
}
