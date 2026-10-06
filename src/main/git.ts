import { execFile } from 'node:child_process'
import { copyFile, mkdtemp, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { promisify } from 'node:util'
import { type FileChange, parseDiff, type RepoChanges } from '@shared/diff'
import {
  type Branch,
  type GitAction,
  invalidBranchName,
  parseBranches,
  planGitAction
} from '@shared/gitActions'
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
 * Paths inside `dir` that git ignores, relative to it with `/` separators, or null if git
 * cannot say. One command covers the whole tree: `--directory` reports a fully ignored
 * directory (`vendor/`) as one entry instead of listing its files, so it stays cheap on large
 * projects. Trailing slashes are dropped. Tracked files are never reported, even if a
 * pattern matches them.
 */
export async function listIgnored(dir: string): Promise<string[] | null> {
  try {
    const { stdout } = await run(
      'git',
      [
        '--no-optional-locks',
        'ls-files',
        '--others',
        '--ignored',
        '--exclude-standard',
        '--directory',
        '-z'
      ],
      { cwd: dir, maxBuffer: 256 * 1024 * 1024 }
    )
    return stdout
      .split('\0')
      .filter(Boolean)
      .map((path) => path.replace(/\/$/, ''))
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
  return withScratchIndex(dir, async ({ git, env, head }) => {
    const staged = parseDiff(await cachedDiff(git, env, head))
    const indexTree = (await git(['write-tree'], env)).trim()
    await git(['add', '--all'], env)
    const unstaged = parseDiff(await cachedDiff(git, env, indexTree))
    return { staged, unstaged }
  })
}

type Git = (args: string[], env?: NodeJS.ProcessEnv) => Promise<string>

const cachedDiff = (git: Git, env: NodeJS.ProcessEnv, base: string): Promise<string> =>
  git(
    ['-c', 'core.quotepath=false', 'diff', '--cached', '--no-color', '--no-ext-diff', '-M', base],
    env
  )

/**
 * Run `fn` against a throwaway copy of the index (via `GIT_INDEX_FILE`) so the user's real index
 * is never touched. `head` is `HEAD`, or the empty tree before the first commit. Null on failure.
 */
async function withScratchIndex<T>(
  dir: string,
  fn: (ctx: { git: Git; env: NodeJS.ProcessEnv; head: string }) => Promise<T>
): Promise<T | null> {
  let scratch = ''
  try {
    const git: Git = async (args, env) => {
      const { stdout } = await run('git', args, { cwd: dir, env, maxBuffer: MAX_DIFF_BYTES })
      return stdout
    }
    const indexPath = (
      await git(['rev-parse', '--path-format=absolute', '--git-path', 'index'])
    ).trim()
    scratch = await mkdtemp(join(tmpdir(), 'vettr-index-'))
    const tempIndex = join(scratch, 'index')
    // A missing index (fresh repo) is fine: git starts from an empty one
    await copyFile(indexPath, tempIndex).catch(() => undefined)
    const env = { ...process.env, GIT_INDEX_FILE: tempIndex }
    const head = await git(['rev-parse', '--verify', '--quiet', 'HEAD']).then(
      (out) => out.trim(),
      () => git(['hash-object', '-t', 'tree', '/dev/null']).then((out) => out.trim())
    )
    return await fn({ git, env, head })
  } catch {
    return null
  } finally {
    if (scratch) await rm(scratch, { recursive: true, force: true })
  }
}

/**
 * Record the whole working tree (untracked files included, ignored ones not) as a git tree object
 * and return its id, or null on failure. Used as the baseline for the round-to-round diff. The
 * object is unreferenced, so git may collect it after its prune grace period (two weeks).
 */
export function snapshotTree(dir: string): Promise<string | null> {
  return withScratchIndex(dir, async ({ git, env }) => {
    await git(['add', '--all'], env)
    return (await git(['write-tree'], env)).trim()
  })
}

/** The working tree against a tree from `snapshotTree`: what changed since then. Null on failure. */
export function getChangesSince(dir: string, tree: string): Promise<FileChange[] | null> {
  return withScratchIndex(dir, async ({ git, env }) => {
    await git(['add', '--all'], env)
    return parseDiff(await cachedDiff(git, env, tree))
  })
}

/** What git said when an index operation failed, for showing to the user. */
function failureMessage(error: unknown): string {
  const { stderr } = error as { stderr?: string }
  return stderr?.trim() || (error as Error).message
}

async function indexOp(dir: string, args: string[], paths: string[]): Promise<string | null> {
  if (paths.length === 0) return null
  try {
    // Literal pathspecs: file names containing `*` or `[` must not be treated as globs
    await run('git', ['--literal-pathspecs', ...args, '--', ...paths], { cwd: dir })
    return null
  } catch (error) {
    return failureMessage(error)
  }
}

/**
 * Stage whole files, including deletions and untracked files. Resolves to null on success or
 * to git's message on failure. Paths are relative to the repo root `dir`.
 */
export function stageFiles(dir: string, paths: string[]): Promise<string | null> {
  return indexOp(dir, ['add', '--all'], paths)
}

/**
 * Unstage whole files. `git reset` rather than `restore --staged` because it also works before
 * the first commit. For a staged rename pass both the old and new path.
 */
export function unstageFiles(dir: string, paths: string[]): Promise<string | null> {
  return indexOp(dir, ['reset', '-q'], paths)
}

/**
 * Commit what is staged with the user's message, using the host's `git` and hooks. Resolves to
 * null on success or to git's message on failure (nothing staged, a hook rejected it, ...).
 * The message is a single argument, never run through a shell.
 */
export async function commitStaged(dir: string, message: string): Promise<string | null> {
  try {
    await run('git', ['commit', '-m', message], { cwd: dir })
    return null
  } catch (error) {
    return failureMessage(error)
  }
}

/** Environment that makes git fail instead of prompting, since there is no terminal. */
function noPromptEnv(): NodeJS.ProcessEnv {
  return {
    ...process.env,
    GIT_TERMINAL_PROMPT: '0',
    GIT_ASKPASS: 'true',
    SSH_ASKPASS: 'true',
    GIT_SSH_COMMAND: process.env.GIT_SSH_COMMAND ?? 'ssh -o BatchMode=yes'
  }
}

async function push(dir: string, args: string[]): Promise<string | null> {
  try {
    await run('git', ['push', ...args], { cwd: dir, env: noPromptEnv() })
    return null
  } catch (error) {
    return failureMessage(error)
  }
}

/**
 * Push the current branch to its upstream using the host's `git`, credentials and config.
 * Prompting is disabled so a push that needs input (passphrase, credentials) fails rather than
 * hanging. Resolves to null on success or to git's message on failure.
 */
export function pushCurrent(dir: string): Promise<string | null> {
  return push(dir, [])
}

/** Names of the configured remotes, `origin` first; empty if there are none or on error. */
export async function listRemotes(dir: string): Promise<string[]> {
  try {
    const { stdout } = await run('git', ['remote'], { cwd: dir })
    const names = stdout.split('\n').filter(Boolean)
    return names.includes('origin') ? ['origin', ...names.filter((n) => n !== 'origin')] : names
  } catch {
    return []
  }
}

/**
 * Publish the current branch to `remote` and set it as the upstream (`git push -u`), like VS Code's
 * "Publish Branch". Pushes `HEAD` so the branch name is never interpolated; `--end-of-options`
 * stops a remote name starting with `-` being read as a flag.
 */
export function publishBranch(dir: string, remote: string): Promise<string | null> {
  return push(dir, ['-u', '--end-of-options', remote, 'HEAD'])
}

/** Local and remote-tracking branches (local first), or an empty list on error. */
export async function listBranches(dir: string): Promise<Branch[]> {
  try {
    const { stdout } = await run(
      'git',
      ['for-each-ref', '--format=%(HEAD)%(refname)', 'refs/heads', 'refs/remotes'],
      { cwd: dir }
    )
    return parseBranches(stdout)
  } catch {
    return []
  }
}

/**
 * Run a command palette git action with the host's `git`. Prompting is disabled and merges never
 * open an editor, since there is no terminal. Resolves to null on success or to git's message.
 */
export async function runGitAction(dir: string, action: GitAction): Promise<string | null> {
  if ('name' in action) {
    const invalid = invalidBranchName(action.name)
    if (invalid) return invalid
  }
  const plan = planGitAction(action)
  const env = { ...noPromptEnv(), GIT_MERGE_AUTOEDIT: 'no' }
  for (const step of plan.steps) {
    try {
      await run('git', step, { cwd: dir, env })
    } catch (error) {
      const message = failureMessage(error)
      if (!plan.abortOnFailure) return message
      // Leave the repo as it was rather than half-merged; a failed abort just means nothing to undo
      await run('git', plan.abortOnFailure, { cwd: dir, env }).catch(() => {})
      return `${message}\n\n${plan.abortNote}`
    }
  }
  return null
}
