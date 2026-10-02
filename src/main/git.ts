import { execFile } from 'node:child_process'
import { promisify } from 'node:util'

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
