import { execFile } from 'node:child_process'
import { copyFile, mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { promisify } from 'node:util'
import { type FileChange, parseDiff } from '@shared/diff'
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
 * Every change in the working tree against `HEAD`, untracked files included, or null if it
 * cannot be read. Untracked files are picked up by staging everything into a throwaway copy
 * of the index (via `GIT_INDEX_FILE`), so the user's real index is never touched. Before the
 * first commit the comparison is against the empty tree.
 */
export async function getChanges(dir: string): Promise<FileChange[] | null> {
  let scratch = ''
  try {
    const git = async (args: string[], env?: NodeJS.ProcessEnv): Promise<string> => {
      const { stdout } = await run('git', args, { cwd: dir, env, maxBuffer: MAX_DIFF_BYTES })
      return stdout
    }
    const indexPath = (
      await git(['rev-parse', '--path-format=absolute', '--git-path', 'index'])
    ).trim()
    scratch = await mkdtemp(join(tmpdir(), 'agentide-index-'))
    const tempIndex = join(scratch, 'index')
    // A missing index (fresh repo) is fine: git starts from an empty one
    await copyFile(indexPath, tempIndex).catch(() => undefined)
    const env = { ...process.env, GIT_INDEX_FILE: tempIndex }
    await git(['add', '--all'], env)
    const base = await git(['rev-parse', '--verify', '--quiet', 'HEAD']).then(
      (out) => out.trim(),
      () => git(['hash-object', '-t', 'tree', '/dev/null']).then((out) => out.trim())
    )
    const patch = await git(
      ['-c', 'core.quotepath=false', 'diff', '--cached', '--no-color', '--no-ext-diff', '-M', base],
      env
    )
    return parseDiff(patch)
  } catch {
    return null
  } finally {
    if (scratch) await rm(scratch, { recursive: true, force: true })
  }
}
