import type { DiffLine, FileChange } from './diff'

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

/** Whether a comment is anchored at (ends on) this line, which is where it is drawn. */
export const endsAt = (c: ReviewComment, file: string, staged: boolean, side: Side, no: number) =>
  c.file === file && c.staged === staged && c.side === side && c.end === no

/** Whether a line lies inside the range being selected or commented. */
export const inRange = (range: { start: number; end: number }, no: number): boolean =>
  no >= range.start && no <= range.end

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
    return [
      `${i + 1}. ${c.file}, ${where}:`,
      `${fence}\n${c.snapshot.join('\n')}\n${fence}`,
      c.text
    ].join('\n')
  })
  return `Please address these review comments on your changes:\n\n${entries.join('\n\n')}`
}
