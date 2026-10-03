import { execFileSync } from 'node:child_process'
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  renameSync,
  rmSync,
  writeFileSync
} from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import {
  findRepoRoot,
  getChanges,
  getRepoStatus,
  pushCurrent,
  stageFiles,
  unstageFiles
} from './git'

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

describe('getChanges', () => {
  const git = (...args: string[]): string =>
    execFileSync('git', ['-C', root, '-c', 'user.name=t', '-c', 'user.email=t@t', ...args], {
      encoding: 'utf8'
    })
  const write = (name: string, content: string): void => {
    writeFileSync(join(root, name), content)
  }
  const commitAll = (): void => {
    git('add', '-A')
    git('commit', '-q', '-m', 'c')
  }
  const paths = (files: { path: string }[] | undefined): string[] =>
    (files ?? []).map((f) => f.path).sort()

  it('returns null when the folder is not a repo', async () => {
    expect(await getChanges(root)).toBeNull()
  })

  it('returns no changes for a clean repo', async () => {
    git('init', '-q', '-b', 'main')
    write('a.txt', 'a\n')
    commitAll()
    expect(await getChanges(root)).toEqual({ staged: [], unstaged: [] })
  })

  it('lists untracked files as unstaged in a repo with no commits', async () => {
    git('init', '-q', '-b', 'main')
    write('a.txt', 'one\ntwo\n')
    const changes = await getChanges(root)
    expect(changes?.staged).toEqual([])
    expect(changes?.unstaged[0]).toMatchObject({
      path: 'a.txt',
      status: 'added',
      additions: 2,
      deletions: 0
    })
  })

  it('lists files staged before the first commit as staged', async () => {
    git('init', '-q', '-b', 'main')
    write('a.txt', 'one\n')
    git('add', 'a.txt')
    const changes = await getChanges(root)
    expect(changes?.staged[0]).toMatchObject({ path: 'a.txt', status: 'added' })
    expect(changes?.unstaged).toEqual([])
  })

  it('reports modified, deleted, added and renamed files', async () => {
    git('init', '-q', '-b', 'main')
    write('mod.txt', 'a\nb\nc\n')
    write('del.txt', 'bye\n')
    write('old name.txt', 'some long enough content\nto be detected\nas a rename\n')
    commitAll()
    write('mod.txt', 'a\nB\nc\n')
    rmSync(join(root, 'del.txt'))
    renameSync(join(root, 'old name.txt'), join(root, 'new name.txt'))
    write('fresh.txt', 'new\n')
    git('add', '-A')
    const changes = await getChanges(root)
    const byPath = Object.fromEntries((changes?.staged ?? []).map((f) => [f.path, f]))
    expect(Object.keys(byPath).sort()).toEqual(['del.txt', 'fresh.txt', 'mod.txt', 'new name.txt'])
    expect(byPath['mod.txt']).toMatchObject({ status: 'modified', additions: 1, deletions: 1 })
    expect(byPath['del.txt']?.status).toBe('deleted')
    expect(byPath['fresh.txt']?.status).toBe('added')
    expect(byPath['new name.txt']).toMatchObject({ status: 'renamed', oldPath: 'old name.txt' })
    expect(changes?.unstaged).toEqual([])
  })

  it('separates staged, unstaged and untracked files', async () => {
    git('init', '-q', '-b', 'main')
    write('staged.txt', 'a\n')
    write('unstaged.txt', 'a\n')
    commitAll()
    write('staged.txt', 'b\n')
    git('add', 'staged.txt')
    write('unstaged.txt', 'b\n')
    write('untracked.txt', 'x\n')
    const changes = await getChanges(root)
    expect(paths(changes?.staged)).toEqual(['staged.txt'])
    expect(paths(changes?.unstaged)).toEqual(['unstaged.txt', 'untracked.txt'])
  })

  it('puts a partially staged file in both lists with only its own lines in each', async () => {
    git('init', '-q', '-b', 'main')
    write('a.txt', 'one\n')
    commitAll()
    write('a.txt', 'one\ntwo\n')
    git('add', 'a.txt')
    write('a.txt', 'one\ntwo\nthree\n')
    const changes = await getChanges(root)
    expect(changes?.staged[0]).toMatchObject({ path: 'a.txt', additions: 1 })
    expect(changes?.unstaged[0]).toMatchObject({ path: 'a.txt', additions: 1 })
  })

  it('does not modify the real index or leave scratch files behind', async () => {
    git('init', '-q', '-b', 'main')
    write('a.txt', 'one\n')
    commitAll()
    write('untracked.txt', 'x\n')
    const indexPath = join(root, '.git', 'index')
    const before = readFileSync(indexPath)
    await getChanges(root)
    expect(readFileSync(indexPath).equals(before)).toBe(true)
    expect(git('status', '--porcelain')).toBe('?? untracked.txt\n')
    expect(existsSync(join(root, '.git', 'index.lock'))).toBe(false)
  })

  it('flags binary files', async () => {
    git('init', '-q', '-b', 'main')
    writeFileSync(join(root, 'img.bin'), Buffer.from([0, 1, 2, 0, 255, 0]))
    const changes = await getChanges(root)
    expect(changes?.unstaged[0]).toMatchObject({ path: 'img.bin', binary: true, hunks: [] })
  })
})

describe('stageFiles and unstageFiles', () => {
  const git = (...args: string[]): string =>
    execFileSync('git', ['-C', root, '-c', 'user.name=t', '-c', 'user.email=t@t', ...args], {
      encoding: 'utf8'
    })
  const write = (name: string, content: string): void => {
    writeFileSync(join(root, name), content)
  }
  const status = (): string => git('status', '--porcelain')

  it('stages and unstages an untracked file before the first commit', async () => {
    git('init', '-q', '-b', 'main')
    write('a.txt', 'a\n')
    expect(await stageFiles(root, ['a.txt'])).toBeNull()
    expect(status()).toBe('A  a.txt\n')
    expect(await unstageFiles(root, ['a.txt'])).toBeNull()
    expect(status()).toBe('?? a.txt\n')
  })

  it('stages modifications and deletions', async () => {
    git('init', '-q', '-b', 'main')
    write('mod.txt', 'a\n')
    write('del.txt', 'a\n')
    git('add', '-A')
    git('commit', '-q', '-m', 'c')
    write('mod.txt', 'b\n')
    rmSync(join(root, 'del.txt'))
    expect(await stageFiles(root, ['mod.txt', 'del.txt'])).toBeNull()
    expect(status()).toBe('D  del.txt\nM  mod.txt\n')
    expect(await unstageFiles(root, ['mod.txt', 'del.txt'])).toBeNull()
    expect(status()).toBe(' D del.txt\n M mod.txt\n')
  })

  it('moves a rename in one step when given both paths', async () => {
    git('init', '-q', '-b', 'main')
    write('old.txt', 'some long enough content\nto be detected\nas a rename\n')
    git('add', '-A')
    git('commit', '-q', '-m', 'c')
    renameSync(join(root, 'old.txt'), join(root, 'new.txt'))
    await stageFiles(root, ['old.txt', 'new.txt'])
    expect(status()).toBe('R  old.txt -> new.txt\n')
    await unstageFiles(root, ['old.txt', 'new.txt'])
    expect(status()).toBe(' D old.txt\n?? new.txt\n')
  })

  it('treats paths literally, not as globs', async () => {
    git('init', '-q', '-b', 'main')
    write('a.txt', 'a\n')
    write('*.txt', 'star\n')
    expect(await stageFiles(root, ['*.txt'])).toBeNull()
    expect(status()).toBe('A  *.txt\n?? a.txt\n')
  })

  it('does nothing for an empty list', async () => {
    git('init', '-q', '-b', 'main')
    write('a.txt', 'a\n')
    expect(await stageFiles(root, [])).toBeNull()
    expect(await unstageFiles(root, [])).toBeNull()
    expect(status()).toBe('?? a.txt\n')
  })

  it("returns git's message when it fails", async () => {
    git('init', '-q', '-b', 'main')
    expect(await stageFiles(root, ['missing.txt'])).toContain('missing.txt')
  })

  it('returns the error text when git cannot run at all', async () => {
    expect(await stageFiles(join(root, 'gone'), ['a'])).toEqual(expect.any(String))
  })
})

describe('pushCurrent', () => {
  const git = (cwd: string, ...args: string[]): string =>
    execFileSync('git', ['-C', cwd, '-c', 'user.name=t', '-c', 'user.email=t@t', ...args], {
      encoding: 'utf8'
    })

  it('pushes commits to the upstream', async () => {
    const remote = join(root, 'remote.git')
    const work = join(root, 'work')
    execFileSync('git', ['init', '-q', '--bare', '-b', 'main', remote])
    execFileSync('git', ['init', '-q', '-b', 'main', work])
    git(work, 'remote', 'add', 'origin', remote)
    writeFileSync(join(work, 'a.txt'), 'a\n')
    git(work, 'add', '.')
    git(work, 'commit', '-q', '-m', 'one')
    git(work, 'push', '-q', '-u', 'origin', 'main')
    writeFileSync(join(work, 'b.txt'), 'b\n')
    git(work, 'add', '.')
    git(work, 'commit', '-q', '-m', 'two')
    expect(await pushCurrent(work)).toBeNull()
    expect(git(remote, 'log', '--format=%s', 'main')).toBe('two\none\n')
  })

  it('returns git output when the push fails', async () => {
    execFileSync('git', ['init', '-q', '-b', 'main', root])
    expect(await pushCurrent(root)).toContain('fatal')
  })

  it('falls back to the error message when git gives no stderr', async () => {
    expect(await pushCurrent(join(root, 'missing'))).toEqual(expect.any(String))
  })
})
