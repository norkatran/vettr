export type ChangeStatus = 'added' | 'modified' | 'deleted' | 'renamed'

export interface DiffLine {
  kind: 'context' | 'add' | 'del'
  /** Line numbers in the old and new file; null on the side the line does not exist. */
  oldNo: number | null
  newNo: number | null
  text: string
}

export interface Hunk {
  /** The `@@ -a,b +c,d @@ section` header line. */
  header: string
  lines: DiffLine[]
}

export interface FileChange {
  /** Path in the working tree (the new path for a rename). */
  path: string
  /** Previous path, only for renames. */
  oldPath: string | null
  status: ChangeStatus
  binary: boolean
  /** True when the diff was dropped for size; the counts are still accurate. */
  tooLarge: boolean
  additions: number
  deletions: number
  hunks: Hunk[]
}

/** A file with more changed lines than this is listed but its diff is not rendered. */
export const MAX_CHANGED_LINES = 5000

/** One row of the split view: the old side on the left, the new side on the right. */
export interface SplitRow {
  left: DiffLine | null
  right: DiffLine | null
}

/** Pair deletions with the additions that follow them so the split view lines them up. */
export function splitRows(lines: DiffLine[]): SplitRow[] {
  const rows: SplitRow[] = []
  let i = 0
  while (i < lines.length) {
    const line = lines[i] as DiffLine
    if (line.kind === 'context') {
      rows.push({ left: line, right: line })
      i++
      continue
    }
    const dels: DiffLine[] = []
    const adds: DiffLine[] = []
    while (lines[i]?.kind === 'del') dels.push(lines[i++] as DiffLine)
    while (lines[i]?.kind === 'add') adds.push(lines[i++] as DiffLine)
    for (let n = 0; n < Math.max(dels.length, adds.length); n++) {
      rows.push({ left: dels[n] ?? null, right: adds[n] ?? null })
    }
  }
  return rows
}

const ESCAPES: Record<string, number> = { t: 9, n: 10, r: 13, a: 7, b: 8, f: 12, v: 11 }

/** Decode a path quoted by git (C-style escapes, octal for raw bytes). */
export function unquotePath(raw: string): string {
  if (raw.length < 2 || !raw.startsWith('"') || !raw.endsWith('"')) return raw
  const body = raw.slice(1, -1)
  const bytes: number[] = []
  const encoder = new TextEncoder()
  for (let i = 0; i < body.length; i++) {
    const ch = body.charAt(i)
    if (ch !== '\\') {
      bytes.push(...encoder.encode(ch))
      continue
    }
    const octal = /^[0-7]{3}/.exec(body.slice(i + 1))
    if (octal) {
      bytes.push(Number.parseInt(octal[0], 8))
      i += 3
    } else {
      const next = body.charAt(++i)
      bytes.push(ESCAPES[next] ?? next.charCodeAt(0))
    }
  }
  return new TextDecoder().decode(new Uint8Array(bytes))
}

const QUOTED_HEADER = /^"a\/((?:[^"\\]|\\.)*)" "b\/(?:[^"\\]|\\.)*"$/

/**
 * The path from a `diff --git a/X b/X` line. Only valid when both sides are the same path
 * (renames overwrite it from their `rename to` line, so the result is never used for those).
 */
function headerPath(rest: string): string {
  const quoted = QUOTED_HEADER.exec(rest)
  if (quoted) return unquotePath(`"${quoted[1]}"`)
  // "a/" + X + " b/" + X is 2|X| + 5 characters long
  return rest.slice(2, (rest.length - 5) / 2 + 2)
}

const HUNK_HEADER = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/

/** Parse the output of `git diff` (patch format) into per-file changes. */
export function parseDiff(output: string): FileChange[] {
  const files: FileChange[] = []
  let file: FileChange | null = null
  let hunk: Hunk | null = null
  let oldNo = 0
  let newNo = 0

  for (const line of output.split('\n')) {
    if (line.startsWith('diff --git ')) {
      file = {
        path: headerPath(line.slice('diff --git '.length)),
        oldPath: null,
        status: 'modified',
        binary: false,
        tooLarge: false,
        additions: 0,
        deletions: 0,
        hunks: []
      }
      hunk = null
      files.push(file)
      continue
    }
    if (!file) continue
    if (line.startsWith('@@ ')) {
      const match = HUNK_HEADER.exec(line)
      oldNo = Number(match?.[1])
      newNo = Number(match?.[2])
      hunk = { header: line, lines: [] }
      file.hunks.push(hunk)
    } else if (hunk) {
      // Anything but these is "\ No newline at end of file" or the trailing blank line
      const text = line.slice(1)
      if (line.startsWith('+')) {
        hunk.lines.push({ kind: 'add', oldNo: null, newNo: newNo++, text })
        file.additions++
      } else if (line.startsWith('-')) {
        hunk.lines.push({ kind: 'del', oldNo: oldNo++, newNo: null, text })
        file.deletions++
      } else if (line.startsWith(' ')) {
        hunk.lines.push({ kind: 'context', oldNo: oldNo++, newNo: newNo++, text })
      }
    } else if (line.startsWith('new file mode')) {
      file.status = 'added'
    } else if (line.startsWith('deleted file mode')) {
      file.status = 'deleted'
    } else if (line.startsWith('rename from ')) {
      file.status = 'renamed'
      file.oldPath = unquotePath(line.slice('rename from '.length))
    } else if (line.startsWith('rename to ')) {
      file.path = unquotePath(line.slice('rename to '.length))
    } else if (line.startsWith('Binary files ') || line.startsWith('GIT binary patch')) {
      file.binary = true
    }
  }

  for (const f of files) {
    if (f.additions + f.deletions > MAX_CHANGED_LINES) {
      f.tooLarge = true
      f.hunks = []
    }
  }
  return files
}
