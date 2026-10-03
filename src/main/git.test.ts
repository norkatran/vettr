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
  commitStaged,
  findRepoRoot,
  getChanges,
  getChangesSince,
  getRepoStatus,
  listBranches,
  listRemotes,
  publishBranch,
  pushCurrent,
  runGitAction,
  snapshotTree,
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

describe('listRemotes and publishBranch', () => {
  const git = (cwd: string, ...args: string[]): string =>
    execFileSync('git', ['-C', cwd, '-c', 'user.name=t', '-c', 'user.email=t@t', ...args], {
      encoding: 'utf8'
    })
  const setup = (): { remote: string; work: string } => {
    const remote = join(root, 'remote.git')
    const work = join(root, 'work')
    execFileSync('git', ['init', '-q', '--bare', '-b', 'main', remote])
    execFileSync('git', ['init', '-q', '-b', 'main', work])
    writeFileSync(join(work, 'a.txt'), 'a\n')
    git(work, 'add', '.')
    git(work, 'commit', '-q', '-m', 'one')
    return { remote, work }
  }

  it('lists remotes with origin first', async () => {
    const { remote, work } = setup()
    git(work, 'remote', 'add', 'backup', remote)
    git(work, 'remote', 'add', 'origin', remote)
    expect(await listRemotes(work)).toEqual(['origin', 'backup'])
  })

  it('lists remotes without origin as they come', async () => {
    const { remote, work } = setup()
    git(work, 'remote', 'add', 'backup', remote)
    expect(await listRemotes(work)).toEqual(['backup'])
  })

  it('returns an empty list with no remotes or outside a repo', async () => {
    const { work } = setup()
    expect(await listRemotes(work)).toEqual([])
    expect(await listRemotes(join(root, 'missing'))).toEqual([])
  })

  it('publishes the branch and sets its upstream', async () => {
    const { remote, work } = setup()
    git(work, 'remote', 'add', 'origin', remote)
    expect(await publishBranch(work, 'origin')).toBeNull()
    expect(git(remote, 'log', '--format=%s', 'main')).toBe('one\n')
    expect(git(work, 'rev-parse', '--abbrev-ref', '@{u}').trim()).toBe('origin/main')
  })

  it('returns git output when publishing fails', async () => {
    const { work } = setup()
    expect(await publishBranch(work, 'nope')).toContain('fatal')
  })
})

describe('commitStaged', () => {
  const git = (cwd: string, ...args: string[]): string =>
    execFileSync('git', ['-C', cwd, '-c', 'user.name=t', '-c', 'user.email=t@t', ...args], {
      encoding: 'utf8'
    })

  it('commits the staged files with the message', async () => {
    execFileSync('git', ['init', '-q', '-b', 'main', root])
    git(root, 'config', 'user.name', 't')
    git(root, 'config', 'user.email', 't@t')
    writeFileSync(join(root, 'a.txt'), 'a\n')
    git(root, 'add', '.')
    expect(await commitStaged(root, 'hello')).toBeNull()
    expect(git(root, 'log', '--format=%s')).toBe('hello\n')
  })

  it('returns git output when there is nothing to commit', async () => {
    execFileSync('git', ['init', '-q', '-b', 'main', root])
    expect(await commitStaged(root, 'hello')).toEqual(expect.any(String))
  })
})

describe('snapshotTree and getChangesSince', () => {
  const git = (...args: string[]): string =>
    execFileSync('git', ['-C', root, '-c', 'user.name=t', '-c', 'user.email=t@t', ...args], {
      encoding: 'utf8'
    })
  const write = (name: string, content: string): void => {
    writeFileSync(join(root, name), content)
  }

  it('return null when the folder is not a repo', async () => {
    expect(await snapshotTree(root)).toBeNull()
    expect(await getChangesSince(root, 'abc')).toBeNull()
  })

  it('return null for a tree that does not exist', async () => {
    git('init', '-q', '-b', 'main')
    expect(await getChangesSince(root, '0'.repeat(40))).toBeNull()
  })

  it('show only what changed after the snapshot, untracked files included', async () => {
    git('init', '-q', '-b', 'main')
    write('a.txt', 'one\n')
    git('add', '-A')
    git('commit', '-q', '-m', 'c')
    write('a.txt', 'one\ntwo\n')
    write('new.txt', 'x\n')
    const tree = await snapshotTree(root)
    expect(tree).toMatch(/^[0-9a-f]{40}$/)
    expect(await getChangesSince(root, tree as string)).toEqual([])

    write('a.txt', 'one\ntwo\nthree\n')
    write('later.txt', 'y\n')
    const since = await getChangesSince(root, tree as string)
    expect((since ?? []).map((f) => f.path).sort()).toEqual(['a.txt', 'later.txt'])
    expect(since?.find((f) => f.path === 'a.txt')?.additions).toBe(1)
  })

  it('leave the real index alone', async () => {
    git('init', '-q', '-b', 'main')
    write('a.txt', 'one\n')
    await snapshotTree(root)
    expect(git('status', '--porcelain')).toBe('?? a.txt\n')
  })
})

describe('listBranches and runGitAction', () => {
  const git = (...args: string[]): string =>
    execFileSync('git', ['-C', root, ...args], { encoding: 'utf8' })
  const write = (name: string, content: string): void => writeFileSync(join(root, name), content)
  const commit = (name: string, content: string): void => {
    write(name, content)
    git('add', '.')
    git('commit', '-q', '-m', `edit ${name}`)
  }
  const branch = (): string => git('branch', '--show-current').trim()

  beforeEach(() => {
    execFileSync('git', ['init', '-q', '-b', 'main', root])
    git('config', 'user.name', 't')
    git('config', 'user.email', 't@t')
    commit('a.txt', 'a\n')
  })

  it('lists local and remote branches, marking the current one', async () => {
    const remote = join(root, '..', `${root.split('/').pop()}-remote.git`)
    execFileSync('git', ['init', '-q', '--bare', '-b', 'main', remote])
    git('remote', 'add', 'origin', remote)
    git('push', '-q', '-u', 'origin', 'main')
    git('branch', 'dev')
    try {
      expect(await listBranches(root)).toEqual([
        { ref: 'dev', remote: false, current: false },
        { ref: 'main', remote: false, current: true },
        { ref: 'origin/main', remote: true, current: false }
      ])
    } finally {
      rmSync(remote, { recursive: true, force: true })
    }
  })

  it('returns no branches when the folder is not a repo', async () => {
    expect(await listBranches(join(root, 'missing'))).toEqual([])
  })

  it('creates, switches to and deletes branches', async () => {
    expect(await runGitAction(root, { kind: 'createBranch', name: 'topic' })).toBeNull()
    expect(branch()).toBe('topic')
    expect(await runGitAction(root, { kind: 'checkout', name: 'main' })).toBeNull()
    expect(branch()).toBe('main')
    expect(await runGitAction(root, { kind: 'deleteBranch', name: 'topic' })).toBeNull()
    expect(git('branch', '--list', 'topic')).toBe('')
  })

  it('refuses to delete an unmerged branch', async () => {
    git('switch', '-c', 'topic')
    commit('b.txt', 'b\n')
    git('switch', 'main')
    expect(await runGitAction(root, { kind: 'deleteBranch', name: 'topic' })).toContain(
      'not fully merged'
    )
  })

  it('rejects bad branch names before running git', async () => {
    expect(await runGitAction(root, { kind: 'createBranch', name: '-x' })).toMatch(/cannot start/)
    expect(branch()).toBe('main')
  })

  it('stages and unstages everything', async () => {
    write('b.txt', 'b\n')
    expect(await runGitAction(root, { kind: 'stageAll' })).toBeNull()
    expect(git('status', '--porcelain')).toBe('A  b.txt\n')
    expect(await runGitAction(root, { kind: 'unstageAll' })).toBeNull()
    expect(git('status', '--porcelain')).toBe('?? b.txt\n')
  })

  it('discards tracked changes and untracked files', async () => {
    write('a.txt', 'changed\n')
    write('new.txt', 'x\n')
    expect(await runGitAction(root, { kind: 'discardAll' })).toBeNull()
    expect(git('status', '--porcelain')).toBe('')
    expect(readFileSync(join(root, 'a.txt'), 'utf8')).toBe('a\n')
  })

  it('stashes and pops changes', async () => {
    write('a.txt', 'changed\n')
    write('new.txt', 'x\n')
    expect(await runGitAction(root, { kind: 'stash' })).toBeNull()
    expect(git('status', '--porcelain')).toBe('')
    expect(await runGitAction(root, { kind: 'stashPop' })).toBeNull()
    expect(git('status', '--porcelain')).toContain('a.txt')
  })

  it('reports an error when there is nothing to pop', async () => {
    expect(await runGitAction(root, { kind: 'stashPop' })).toContain('No stash')
  })

  it('fetches and pulls from a remote', async () => {
    const remote = join(root, '..', `${root.split('/').pop()}-remote.git`)
    execFileSync('git', ['init', '-q', '--bare', '-b', 'main', remote])
    git('remote', 'add', 'origin', remote)
    git('push', '-q', '-u', 'origin', 'main')
    const other = `${remote}-clone`
    execFileSync('git', ['clone', '-q', remote, other])
    const g = (...args: string[]): string =>
      execFileSync('git', ['-C', other, '-c', 'user.name=t', '-c', 'user.email=t@t', ...args], {
        encoding: 'utf8'
      })
    writeFileSync(join(other, 'c.txt'), 'c\n')
    g('add', '.')
    g('commit', '-q', '-m', 'remote change')
    g('push', '-q')
    try {
      expect(await runGitAction(root, { kind: 'fetch' })).toBeNull()
      expect(git('rev-list', '--count', 'HEAD..origin/main').trim()).toBe('1')
      expect(await runGitAction(root, { kind: 'pull' })).toBeNull()
      expect(existsSync(join(root, 'c.txt'))).toBe(true)
    } finally {
      rmSync(remote, { recursive: true, force: true })
      rmSync(other, { recursive: true, force: true })
    }
  })

  it('merges a branch', async () => {
    git('switch', '-c', 'topic')
    commit('b.txt', 'b\n')
    git('switch', 'main')
    expect(await runGitAction(root, { kind: 'merge', name: 'topic' })).toBeNull()
    expect(existsSync(join(root, 'b.txt'))).toBe(true)
  })

  it('aborts a conflicting merge and says so', async () => {
    git('switch', '-c', 'topic')
    commit('a.txt', 'topic\n')
    git('switch', 'main')
    commit('a.txt', 'main\n')
    const message = await runGitAction(root, { kind: 'merge', name: 'topic' })
    expect(message).toContain('The merge was aborted')
    expect(git('status', '--porcelain')).toBe('')
  })

  it('rebases onto a branch', async () => {
    git('switch', '-c', 'topic')
    commit('b.txt', 'b\n')
    git('switch', 'main')
    commit('c.txt', 'c\n')
    git('switch', 'topic')
    expect(await runGitAction(root, { kind: 'rebase', name: 'main' })).toBeNull()
    expect(git('log', '--format=%s').split('\n')[0]).toBe('edit b.txt')
    expect(existsSync(join(root, 'c.txt'))).toBe(true)
  })

  it('aborts a conflicting rebase', async () => {
    git('switch', '-c', 'topic')
    commit('a.txt', 'topic\n')
    git('switch', 'main')
    commit('a.txt', 'main\n')
    git('switch', 'topic')
    expect(await runGitAction(root, { kind: 'rebase', name: 'main' })).toContain(
      'The rebase was aborted'
    )
    expect(branch()).toBe('topic')
  })

  it('still reports git output when the abort itself fails', async () => {
    // Merging a missing branch fails before any merge starts, so there is nothing to abort
    const message = await runGitAction(root, { kind: 'merge', name: 'nope' })
    expect(message).toContain('nope')
    expect(message).toContain('The merge was aborted')
  })
})
