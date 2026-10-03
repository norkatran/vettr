import { execFile } from 'node:child_process'
import { copyFile, mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { promisify } from 'node:util'
import { parseDiff, type RepoChanges } from '@shared/diff'
import { parseRepoStatus, type RepoStatus } from '@shared/repoStatus'

const run = promisify(execFile)

/**
 * The root of the git working tree containing `dir`, or null if `dir` is not
 * inside one (or does not exist). Uses git itself so worktrees, submodules and
 * `.git` files are handled.
 */
export async function findRepoRoot(dir: string): Promise<string | null> {
  try {
    const { stdout } = await run('git', ['rev-parse', '--show-toplevel'], { cwd: dir })
    return stdout.trim()
  } catch {
    return null
  }
}

/**
 * Branch, upstream divergence and change count for the repo at `dir`, or null if it cannot
 * be read. `--no-optional-locks` stops this background query from contending with the
 * user's own git commands over the index lock.
 */
export async function getRepoStatus(dir: string): Promise<RepoStatus | null> {
  try {
    const { stdout } = await run(
      'git',
      ['--no-optional-locks', 'status', '--porcelain=v2', '--branch'],
      { cwd: dir }
    )
    return parseRepoStatus(stdout)
  } catch {
    return null
  }
}

const MAX_DIFF_BYTES = 256 * 1024 * 1024

/**
 * The changes in `dir` split by index state, or null if they cannot be read. `staged` is the
 * index against `HEAD` (the empty tree before the first commit); `unstaged` is the working tree
 * against the index, untracked files included. A partially staged file is in both.
 *
 * Everything runs against a throwaway copy of the index (via `GIT_INDEX_FILE`): the staged diff
 * reads it as is, then `add --all` on the copy picks up untracked files for the unstaged diff.
 * The user's real index is never touched.
 */
export async function getChanges(dir: string): Promise<RepoChanges | null> {
  let scratch = ''
  try {
    const git = async (args: string[], env?: NodeJS.ProcessEnv): Promise<string> => {
      const { stdout } = await run('git', args, { cwd: dir, env, maxBuffer: MAX_DIFF_BYTES })
      return stdout
    }
    const diff = (env: NodeJS.ProcessEnv, base: string): Promise<string> =>
      git(
        [
          '-c',
          'core.quotepath=false',
          'diff',
          '--cached',
          '--no-color',
          '--no-ext-diff',
          '-M',
          base
        ],
        env
      )
    const indexPath = (
      await git(['rev-parse', '--path-format=absolute', '--git-path', 'index'])
    ).trim()
    scratch = await mkdtemp(join(tmpdir(), 'agentide-index-'))
    const tempIndex = join(scratch, 'index')
    // A missing index (fresh repo) is fine: git starts from an empty one
    await copyFile(indexPath, tempIndex).catch(() => undefined)
    const env = { ...process.env, GIT_INDEX_FILE: tempIndex }
    const head = await git(['rev-parse', '--verify', '--quiet', 'HEAD']).then(
      (out) => out.trim(),
      () => git(['hash-object', '-t', 'tree', '/dev/null']).then((out) => out.trim())
    )
    const staged = parseDiff(await diff(env, head))
    const indexTree = (await git(['write-tree'], env)).trim()
    await git(['add', '--all'], env)
    const unstaged = parseDiff(await diff(env, indexTree))
    return { staged, unstaged }
  } catch {
    return null
  } finally {
    if (scratch) await rm(scratch, { recursive: true, force: true })
  }
}
