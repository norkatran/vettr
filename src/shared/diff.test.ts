import { describe, expect, it } from 'vitest'
import {
  changedFileCount,
  type DiffLine,
  type FileChange,
  MAX_CHANGED_LINES,
  parseDiff,
  splitRows,
  unquotePath
} from './diff'

describe('parseDiff', () => {
  it('returns nothing for empty output or text before the first file', () => {
    expect(parseDiff('')).toEqual([])
    expect(parseDiff('warning: something\n')).toEqual([])
  })

  it('parses a modified file with line numbers and counts', () => {
    const out = [
      'diff --git a/src/a.ts b/src/a.ts',
      'index 111..222 100644',
      '--- a/src/a.ts',
      '+++ b/src/a.ts',
      '@@ -1,3 +1,3 @@ function x() {',
      ' keep',
      '-old',
      '+new',
      ' tail',
      '\\ No newline at end of file',
      '@@ -10 +10,2 @@',
      '+added',
      ' end',
      ''
    ].join('\n')
    const [file] = parseDiff(out)
    expect(file).toMatchObject({
      path: 'src/a.ts',
      oldPath: null,
      status: 'modified',
      binary: false,
      tooLarge: false,
      additions: 2,
      deletions: 1
    })
    expect(file?.hunks).toHaveLength(2)
    expect(file?.hunks[0]?.header).toBe('@@ -1,3 +1,3 @@ function x() {')
    expect(file?.hunks[0]?.lines).toEqual([
      { kind: 'context', oldNo: 1, newNo: 1, text: 'keep' },
      { kind: 'del', oldNo: 2, newNo: null, text: 'old' },
      { kind: 'add', oldNo: null, newNo: 2, text: 'new' },
      { kind: 'context', oldNo: 3, newNo: 3, text: 'tail' }
    ])
    expect(file?.hunks[1]?.lines[0]).toEqual({
      kind: 'add',
      oldNo: null,
      newNo: 10,
      text: 'added'
    })
  })

  it('detects added and deleted files', () => {
    const out = [
      'diff --git a/new.txt b/new.txt',
      'new file mode 100644',
      '--- /dev/null',
      '+++ b/new.txt',
      '@@ -0,0 +1 @@',
      '+hi',
      'diff --git a/gone.txt b/gone.txt',
      'deleted file mode 100644',
      '--- a/gone.txt',
      '+++ /dev/null',
      '@@ -1 +0,0 @@',
      '-bye',
      ''
    ].join('\n')
    expect(parseDiff(out).map((f) => [f.path, f.status])).toEqual([
      ['new.txt', 'added'],
      ['gone.txt', 'deleted']
    ])
  })

  it('detects renames, with and without content changes', () => {
    const out = [
      'diff --git a/old name.ts b/new name.ts',
      'similarity index 100%',
      'rename from old name.ts',
      'rename to new name.ts',
      'diff --git a/x.ts b/y.ts',
      'similarity index 90%',
      'rename from x.ts',
      'rename to y.ts',
      '@@ -1 +1 @@',
      '-a',
      '+b',
      ''
    ].join('\n')
    const [pure, edited] = parseDiff(out)
    expect(pure).toMatchObject({ path: 'new name.ts', oldPath: 'old name.ts', status: 'renamed' })
    expect(pure?.hunks).toEqual([])
    expect(edited).toMatchObject({ path: 'y.ts', oldPath: 'x.ts', additions: 1, deletions: 1 })
  })

  it('flags binary files', () => {
    const out = [
      'diff --git a/img.png b/img.png',
      'index 1..2 100644',
      'Binary files a/img.png and b/img.png differ',
      'diff --git a/blob.bin b/blob.bin',
      'GIT binary patch',
      'literal 3',
      'KcmZQz',
      ''
    ].join('\n')
    const files = parseDiff(out)
    expect(files.map((f) => f.binary)).toEqual([true, true])
    expect(files.map((f) => f.hunks)).toEqual([[], []])
  })

  it('reads paths with spaces and quoted special characters from the header', () => {
    const out = [
      'diff --git a/my file.txt b/my file.txt',
      'old mode 100644',
      'new mode 100755',
      'diff --git "a/tab\\there.txt" "b/tab\\there.txt"',
      'new mode 100755',
      ''
    ].join('\n')
    expect(parseDiff(out).map((f) => f.path)).toEqual(['my file.txt', 'tab\there.txt'])
  })

  it('drops the hunks of a file with too many changed lines but keeps the counts', () => {
    const lines = Array.from({ length: MAX_CHANGED_LINES + 1 }, (_, i) => `+line ${i}`)
    const out = ['diff --git a/big.txt b/big.txt', '@@ -0,0 +1 @@', ...lines, ''].join('\n')
    const [file] = parseDiff(out)
    expect(file).toMatchObject({ tooLarge: true, hunks: [], additions: MAX_CHANGED_LINES + 1 })
  })

  it('keeps a file at exactly the limit', () => {
    const lines = Array.from({ length: MAX_CHANGED_LINES }, (_, i) => `+line ${i}`)
    const out = ['diff --git a/big.txt b/big.txt', '@@ -0,0 +1 @@', ...lines, ''].join('\n')
    expect(parseDiff(out)[0]?.tooLarge).toBe(false)
  })
})

describe('unquotePath', () => {
  it('returns unquoted paths unchanged', () => {
    expect(unquotePath('plain.txt')).toBe('plain.txt')
    expect(unquotePath('"')).toBe('"')
    expect(unquotePath('"half')).toBe('"half')
  })

  it('decodes named escapes, quotes, backslashes and unicode', () => {
    expect(unquotePath('"a\\tb\\nc\\"d\\\\e\\u"')).toBe('a\tb\nc"d\\eu')
    expect(unquotePath('"café"')).toBe('café')
  })

  it('decodes octal escapes as UTF-8 bytes', () => {
    expect(unquotePath('"caf\\303\\251"')).toBe('café')
  })
})

describe('splitRows', () => {
  const ctx = (n: number): DiffLine => ({ kind: 'context', oldNo: n, newNo: n, text: `c${n}` })
  const del = (n: number): DiffLine => ({ kind: 'del', oldNo: n, newNo: null, text: `d${n}` })
  const add = (n: number): DiffLine => ({ kind: 'add', oldNo: null, newNo: n, text: `a${n}` })

  it('shows context on both sides', () => {
    expect(splitRows([ctx(1)])).toEqual([{ left: ctx(1), right: ctx(1) }])
  })

  it('pairs deletions with the additions that follow', () => {
    expect(splitRows([del(1), del(2), add(1), add(2)])).toEqual([
      { left: del(1), right: add(1) },
      { left: del(2), right: add(2) }
    ])
  })

  it('pads the shorter side of a change block', () => {
    expect(splitRows([del(1), del(2), add(1), ctx(3)])).toEqual([
      { left: del(1), right: add(1) },
      { left: del(2), right: null },
      { left: ctx(3), right: ctx(3) }
    ])
    expect(splitRows([add(1), add(2)])).toEqual([
      { left: null, right: add(1) },
      { left: null, right: add(2) }
    ])
  })

  it('returns no rows for no lines', () => {
    expect(splitRows([])).toEqual([])
  })
})

describe('changedFileCount', () => {
  const file = (path: string): FileChange => ({
    path,
    oldPath: null,
    status: 'modified',
    binary: false,
    tooLarge: false,
    additions: 1,
    deletions: 0,
    hunks: []
  })

  it('is zero with no changes', () => {
    expect(changedFileCount({ staged: [], unstaged: [] })).toBe(0)
  })

  it('counts a partially staged file once', () => {
    const changes = { staged: [file('a'), file('b')], unstaged: [file('b'), file('c')] }
    expect(changedFileCount(changes)).toBe(3)
  })
})
