import type { ChildProcessWithoutNullStreams } from 'node:child_process'
import { execFile } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { lstat, mkdir } from 'node:fs/promises'
import { join } from 'node:path'
import { promisify } from 'node:util'
import {
  buildRunArgs,
  CONTAINER_LABEL,
  type DockerProblem,
  OWNER_LABEL,
  parseOrphanCandidates,
  SANDBOX_IMAGE
} from '@shared/sandbox'

const run = promisify(execFile)

/** Runs a command and rejects if it fails; injected so Docker checks can be tested without Docker. */
export type Exec = (file: string, args: string[]) => Promise<unknown>

export type Spawn = (
  file: string,
  args: string[],
  options: { stdio: 'pipe' }
) => ChildProcessWithoutNullStreams

/** What is wrong with the Docker setup, or null when the sandbox can be started. */
export async function checkDockerDetailed(exec: Exec): Promise<DockerProblem | null> {
  try {
    await exec('docker', ['version', '--format', '{{.Server.Version}}'])
  } catch {
    return {
      kind: 'docker',
      message:
        'Docker is not available. Install Docker and make sure its daemon is running and your user can use it.'
    }
  }
  try {
    await exec('docker', ['image', 'inspect', SANDBOX_IMAGE])
  } catch {
    return {
      kind: 'image',
      message: `The sandbox image "${SANDBOX_IMAGE}" is missing. Build it with "npm run build:sandbox".`
    }
  }
  return null
}

/** A user-facing problem with the Docker setup, or null when the sandbox can be started. */
export async function checkDocker(exec: Exec): Promise<string | null> {
  return (await checkDockerDetailed(exec))?.message ?? null
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
  /** Pid of this app process, recorded on the container for the orphan sweep. */
  ownerPid: number
  /** Host dir for the container's Claude config (see `transcriptsDir`); created if missing. */
  transcriptsDir: string
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
  ownerPid,
  transcriptsDir,
  spawn,
  uid,
  gid
}: StartOptions): Promise<ChildProcessWithoutNullStreams> {
  // Made by the host user before docker runs, so the container user (same uid) can write to it
  await mkdir(transcriptsDir, { recursive: true })
  const args = buildRunArgs({
    name: `vettr-${randomUUID().slice(0, 8)}`,
    ownerPid,
    project,
    readOnlyPaths: await readOnlyGitPaths(project),
    transcriptsDir,
    uid,
    gid
  })
  return spawn('docker', args, { stdio: 'pipe' })
}

/** Ask the container to stop. `--init` forwards the signal, so the runner and SDK shut down. */
export function stopSandbox(container: ChildProcessWithoutNullStreams): void {
  container.kill('SIGTERM')
}

/** Runs a command and resolves to its stdout. */
export type ExecOutput = (file: string, args: string[]) => Promise<string>

/**
 * Remove vettr containers left behind by an app process that is gone (a crash or a kill, which
 * `--rm` does not cover). Containers owned by a live process, such as another running vettr,
 * are left alone. Resolves to how many were removed.
 */
export async function sweepOrphans(
  exec: ExecOutput,
  isAlive: (pid: number) => boolean
): Promise<number> {
  const listing = await exec('docker', [
    'ps',
    '-a',
    '--filter',
    `label=${CONTAINER_LABEL}`,
    '--format',
    `{{.ID}} {{.Label "${OWNER_LABEL}"}}`
  ])
  const orphans = parseOrphanCandidates(listing).filter(({ pid }) => pid === null || !isAlive(pid))
  for (const { id } of orphans) await exec('docker', ['rm', '-f', id]).catch(() => undefined)
  return orphans.length
}

/** Whether a process with this pid exists (signal 0 only checks; EPERM means it is someone else's). */
export function isProcessAlive(pid: number, kill: typeof process.kill = process.kill): boolean {
  try {
    kill(pid, 0)
    return true
  } catch (error) {
    return (error as NodeJS.ErrnoException).code === 'EPERM'
  }
}
