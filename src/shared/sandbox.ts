export const SANDBOX_IMAGE = 'vettr-sandbox'

/** Label put on every container vettr starts, so leftovers can be found and removed. */
export const CONTAINER_LABEL = 'vettr.app=1'
/** Label holding the pid of the vettr process that started the container. */
export const OWNER_LABEL = 'vettr.pid'

/** Where the transcripts dir is mounted inside the container; only this dir of the config is shared. */
export const CONTAINER_CONFIG_DIR = '/vettr-config'

export interface RunOptions {
  name: string
  /** Pid of the app process that owns the container (see `OWNER_LABEL`). */
  ownerPid: number
  /** Absolute project root; mounted at the same path inside the container so paths line up. */
  project: string
  /** Absolute paths to mount read-only over the project (git dirs and a `.git` file). */
  readOnlyPaths: string[]
  /** Host dir mounted as the container's Claude config dir, so SDK transcripts outlive it. */
  transcriptsDir: string
  uid: number
  gid: number
}

/**
 * Arguments for `docker run` (after the `docker` command). The container runs as the host user
 * so files are not root-owned, drops all capabilities, and keeps stdin open for the protocol.
 * Read-only mounts come after the project mount so they overlay it. The runner is the image
 * entrypoint, so nothing follows the image name.
 */
export function buildRunArgs({
  name,
  ownerPid,
  project,
  readOnlyPaths,
  transcriptsDir,
  uid,
  gid
}: RunOptions): string[] {
  const readOnly = [...new Set(readOnlyPaths)].flatMap((path) => ['-v', `${path}:${path}:ro`])
  return [
    'run',
    '--rm',
    '-i',
    '--init',
    '--name',
    name,
    '--label',
    CONTAINER_LABEL,
    '--label',
    `${OWNER_LABEL}=${ownerPid}`,
    '--user',
    `${uid}:${gid}`,
    '--cap-drop',
    'ALL',
    '--security-opt',
    'no-new-privileges',
    // Without this, SELinux hosts (Fedora, RHEL) deny the container access to the bind mounts.
    // The alternative, relabelling the mounts with :z, would change labels on the user's files.
    '--security-opt',
    'label=disable',
    '-v',
    `${transcriptsDir}:${CONTAINER_CONFIG_DIR}`,
    '-e',
    `CLAUDE_CONFIG_DIR=${CONTAINER_CONFIG_DIR}`,
    '--workdir',
    project,
    '-v',
    `${project}:${project}`,
    ...readOnly,
    SANDBOX_IMAGE
  ]
}

/** One line of `docker ps` output for the sweep: container id and owner pid (may be empty). */
export function parseOrphanCandidates(output: string): { id: string; pid: number | null }[] {
  return output
    .split('\n')
    .map((line) => line.trim().split(/\s+/))
    .filter(([id]) => !!id)
    .map(([id, pid]) => ({ id: id as string, pid: /^\d+$/.test(pid ?? '') ? Number(pid) : null }))
}

/** Why the sandbox cannot start: Docker itself, or only its image (which the app can build). */
export interface DockerProblem {
  kind: 'docker' | 'image'
  message: string
}
