import { existsSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { describe, expect, it, vi } from 'vitest'
import { readResolved, setResolved } from './resolvedStore'

const dir = () => mkdtempSync(join(tmpdir(), 'vettr-resolved-'))

describe('resolvedStore', () => {
  it('starts empty', () => {
    expect(readResolved(dir())).toEqual([])
  })

  it('persists resolving and reopening', () => {
    const d = dir()
    expect(setResolved(d, 'a', true)).toEqual(['a'])
    expect(setResolved(d, 'b', true)).toEqual(['a', 'b'])
    expect(readResolved(d)).toEqual(['a', 'b'])
    expect(setResolved(d, 'a', false)).toEqual(['b'])
    expect(readResolved(d)).toEqual(['b'])
  })

  it('does not touch the file when nothing changes', () => {
    const d = dir()
    expect(setResolved(d, 'a', false)).toEqual([])
    expect(existsSync(join(d, 'resolved-comments.json'))).toBe(false)
    setResolved(d, 'a', true)
    expect(setResolved(d, 'a', true)).toEqual(['a'])
  })

  it('creates the data dir when needed', () => {
    const d = join(dir(), 'nested', 'project')
    expect(setResolved(d, 'a', true)).toEqual(['a'])
    expect(readResolved(d)).toEqual(['a'])
  })

  it('treats a corrupt file as empty', () => {
    const d = dir()
    writeFileSync(join(d, 'resolved-comments.json'), '{nope')
    expect(readResolved(d)).toEqual([])
  })

  it('returns the old list and does not throw when it cannot save', () => {
    // A regular file where the data dir should be, so creating or writing under it fails
    const blocker = join(dir(), 'not-a-dir')
    writeFileSync(blocker, '')
    const error = vi.spyOn(console, 'error').mockImplementation(() => {})
    try {
      expect(setResolved(blocker, 'a', true)).toEqual([])
      expect(error).toHaveBeenCalled()
    } finally {
      error.mockRestore()
    }
  })
})
