import { execFileSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { findRepoRoot, getRepoStatus } from './git'

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

describe('getRepoStatus', () => {
  const git = (...args: string[]): void => {
    execFileSync('git', ['-C', root, '-c', 'user.name=t', '-c', 'user.email=t@t', ...args])
  }

  it('reports a fresh repo with an untracked file', async () => {
    git('init', '-q', '-b', 'main')
    writeFileSync(join(root, 'a.txt'), 'a')
    expect(await getRepoStatus(root)).toEqual({
      branch: 'main',
      sha: null,
      upstream: null,
      ahead: 0,
      behind: 0,
      changes: 1
    })
  })

  it('reports ahead/behind against an upstream', async () => {
    git('init', '-q', '-b', 'main')
    git('commit', '-q', '--allow-empty', '-m', 'one')
    git('clone', '-q', root, join(root, 'clone'))
    const clone = join(root, 'clone')
    const inClone = (...args: string[]): void => {
      execFileSync('git', ['-C', clone, '-c', 'user.name=t', '-c', 'user.email=t@t', ...args])
    }
    inClone('commit', '-q', '--allow-empty', '-m', 'local')
    git('commit', '-q', '--allow-empty', '-m', 'upstream')
    inClone('fetch', '-q')
    const status = await getRepoStatus(clone)
    expect(status).toMatchObject({
      branch: 'main',
      upstream: 'origin/main',
      ahead: 1,
      behind: 1,
      changes: 0
    })
    expect(status?.sha).toMatch(/^[0-9a-f]{7}$/)
  })

  it('reports a detached HEAD', async () => {
    git('init', '-q', '-b', 'main')
    git('commit', '-q', '--allow-empty', '-m', 'one')
    git('checkout', '-q', '--detach')
    expect(await getRepoStatus(root)).toMatchObject({ branch: null })
  })

  it('returns null when the folder is not a repo', async () => {
    expect(await getRepoStatus(root)).toBeNull()
  })
})
