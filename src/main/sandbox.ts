import type { ChildProcessWithoutNullStreams } from 'node:child_process'
import { execFile } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { lstat } from 'node:fs/promises'
import { join } from 'node:path'
import { promisify } from 'node:util'
import { buildRunArgs, SANDBOX_IMAGE } from '@shared/sandbox'

const run = promisify(execFile)

/** Runs a command and rejects if it fails; injected so Docker checks can be tested without Docker. */
export type Exec = (file: string, args: string[]) => Promise<unknown>

export type Spawn = (
  file: string,
  args: string[],
  options: { stdio: 'pipe' }
) => ChildProcessWithoutNullStreams

/** A user-facing problem with the Docker setup, or null when the sandbox can be started. */
export async function checkDocker(exec: Exec): Promise<string | null> {
  try {
    await exec('docker', ['version', '--format', '{{.Server.Version}}'])
  } catch {
    return 'Docker is not available. Install Docker and make sure its daemon is running and your user can use it.'
  }
  try {
    await exec('docker', ['image', 'inspect', SANDBOX_IMAGE])
  } catch {
    return `The sandbox image "${SANDBOX_IMAGE}" is missing. Build it with "npm run build:sandbox".`
  }
  return null
}

/**
 * Paths inside or beside the project that the container must only read: the git directory
 * (so the agent cannot commit or edit hooks and config that the host's git would later run), the
 * common directory for linked worktrees, and the `.git` file that points at them in worktrees
 * and submodules (otherwise the agent could redirect it).
 */
export async function readOnlyGitPaths(project: string): Promise<string[]> {
  const { stdout } = await run(
    'git',
    ['rev-parse', '--path-format=absolute', '--git-dir', '--git-common-dir'],
    { cwd: project }
  )
  const paths = stdout.split('\n').filter(Boolean)
  const dotGit = join(project, '.git')
  if ((await lstat(dotGit)).isFile()) paths.push(dotGit)
  return paths
}

export interface StartOptions {
  project: string
  spawn: Spawn
  uid: number
  gid: number
}

/**
 * Start a sandbox container for `project`. The returned process speaks the runner protocol on
 * stdin and stdout; stderr is diagnostics. Throws if the git paths cannot be resolved.
 */
export async function startSandbox({
  project,
  spawn,
  uid,
  gid
}: StartOptions): Promise<ChildProcessWithoutNullStreams> {
  const args = buildRunArgs({
    name: `agentide-${randomUUID().slice(0, 8)}`,
    project,
    readOnlyPaths: await readOnlyGitPaths(project),
    uid,
    gid
  })
  return spawn('docker', args, { stdio: 'pipe' })
}

/** Ask the container to stop. `--init` forwards the signal, so the runner and SDK shut down. */
export function stopSandbox(container: ChildProcessWithoutNullStreams): void {
  container.kill('SIGTERM')
}
