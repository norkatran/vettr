import hljs from 'highlight.js/lib/core'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { DiffLine } from './diff'
import { highlightHunk, languageFor, splitHighlightedLines } from './highlight'

const line = (kind: DiffLine['kind'], text: string): DiffLine => ({
  kind,
  oldNo: kind === 'add' ? null : 1,
  newNo: kind === 'del' ? null : 1,
  text
})

describe('languageFor', () => {
  it('maps extensions and special filenames', () => {
    expect(languageFor('src/a/b.tsx')).toBe('typescript')
    expect(languageFor('Dockerfile')).toBe('dockerfile')
    expect(languageFor('x/README.MD')).toBe('markdown')
  })
  it('returns null for unknown or missing extensions', () => {
    expect(languageFor('LICENSE')).toBeNull()
    expect(languageFor('a.unknownext')).toBeNull()
    expect(languageFor('dir.d/file')).toBeNull()
  })
})

describe('splitHighlightedLines', () => {
  it('closes and reopens spans across newlines', () => {
    expect(splitHighlightedLines('<span class="c">a\nb</span>\nc')).toEqual([
      '<span class="c">a</span>',
      '<span class="c">b</span>',
      'c'
    ])
  })
  it('passes through a stray < that is not a span tag', () => {
    expect(splitHighlightedLines('a <b c')).toEqual(['a <b c'])
  })
  it('keeps escaped entities intact', () => {
    expect(splitHighlightedLines('a &lt; b')).toEqual(['a &lt; b'])
  })
})

describe('highlightHunk', () => {
  it('highlights each side and maps results back to lines', () => {
    const del = line('del', 'const a = 1')
    const add = line('add', 'const a = "x"')
    const ctx = line('context', '/* start')
    const lines = [del, add, ctx]
    const out = highlightHunk('f.ts', lines)
    expect(out.get(del)).toContain('hljs-keyword')
    expect(out.get(add)).toContain('hljs-string')
    expect(out.get(ctx)).toContain('hljs-comment')
  })
  it('returns nothing for unknown languages', () => {
    expect(highlightHunk('LICENSE', [line('add', 'x')]).size).toBe(0)
  })
})

describe('highlightHunk failure handling', () => {
  afterEach(() => vi.restoreAllMocks())

  it('falls back to plain text when the highlighter throws', () => {
    vi.spyOn(hljs, 'highlight').mockImplementation(() => {
      throw new Error('boom')
    })
    expect(highlightHunk('f.ts', [line('add', 'x')]).size).toBe(0)
  })
  it('discards output whose line count does not match the input', () => {
    vi.spyOn(hljs, 'highlight').mockReturnValue({ value: 'a\nb' } as ReturnType<
      typeof hljs.highlight
    >)
    expect(highlightHunk('f.ts', [line('add', 'x')]).size).toBe(0)
  })
  it('keeps the old side when only the new side fails', () => {
    const real = hljs.highlight.bind(hljs)
    let calls = 0
    vi.spyOn(hljs, 'highlight').mockImplementation((code, opts) => {
      if (++calls === 2) throw new Error('boom')
      return real(code, opts)
    })
    const del = line('del', 'const a = 1')
    const add = line('add', 'const b = 2')
    const out = highlightHunk('f.ts', [del, add])
    expect(out.has(del)).toBe(true)
    expect(out.has(add)).toBe(false)
  })
})
