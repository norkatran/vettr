import { execFileSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, realpathSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { findRepoRoot } from './git'

let root = ''

beforeEach(() => {
  root = realpathSync(mkdtempSync(join(tmpdir(), 'agentide-git-test-')))
})

afterEach(() => {
  rmSync(root, { recursive: true, force: true })
})

describe('findRepoRoot', () => {
  it('returns the folder itself when it is a repo root', async () => {
    execFileSync('git', ['init', '-q', root])
    expect(await findRepoRoot(root)).toBe(root)
  })

  it('resolves a subdirectory to the repo root', async () => {
    execFileSync('git', ['init', '-q', root])
    const sub = join(root, 'a', 'b')
    mkdirSync(sub, { recursive: true })
    expect(await findRepoRoot(sub)).toBe(root)
  })

  it('returns null for a folder that is not in a repo', async () => {
    expect(await findRepoRoot(root)).toBeNull()
  })

  it('returns null for a folder that does not exist', async () => {
    expect(await findRepoRoot(join(root, 'gone'))).toBeNull()
  })
})
