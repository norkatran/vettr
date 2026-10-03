export const SANDBOX_IMAGE = 'agentide-sandbox'

/** Where the transcripts dir is mounted inside the container; only this dir of the config is shared. */
export const CONTAINER_CONFIG_DIR = '/agentide-config'

export interface RunOptions {
  name: string
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
