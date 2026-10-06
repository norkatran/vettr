import type { Spawn } from './sandbox'

export interface BuildImageOptions {
  spawn: Spawn
  /** The `sandbox/` directory (Dockerfile and `dist/runner.mjs`). */
  contextDir: string
  image: string
  sdkVersion: string
  /** Called with the latest line of build output. */
  onProgress(line: string): void
}

const OUTPUT_TAIL = 2000
const MAX_PROGRESS = 100

/** Build args for `docker build` with plain, line-based progress output. */
export function buildImageArgs(image: string, sdkVersion: string, contextDir: string): string[] {
  return [
    'build',
    '--progress=plain',
    '-t',
    image,
    '--build-arg',
    `SDK_VERSION=${sdkVersion}`,
    contextDir
  ]
}

/** A build log line made short enough for a status message, or null when it is blank. */
export function progressLine(raw: string): string | null {
  const line = raw.trim()
  if (!line) return null
  return line.length > MAX_PROGRESS ? `${line.slice(0, MAX_PROGRESS - 1)}…` : line
}

/**
 * Build the sandbox image. Resolves to null on success or to a message for the user, which
 * includes the end of Docker's output when the build fails.
 */
export function buildSandboxImage({
  spawn,
  contextDir,
  image,
  sdkVersion,
  onProgress
}: BuildImageOptions): Promise<string | null> {
  return new Promise((resolve) => {
    const child = spawn('docker', buildImageArgs(image, sdkVersion, contextDir), { stdio: 'pipe' })
    let tail = ''
    const pending = { stdout: '', stderr: '' }
    const read = (stream: 'stdout' | 'stderr') => (chunk: Buffer | string) => {
      const text = chunk.toString()
      tail = (tail + text).slice(-OUTPUT_TAIL)
      const parts = (pending[stream] + text).split('\n')
      pending[stream] = parts.pop() as string
      for (const part of parts) {
        const line = progressLine(part)
        if (line) onProgress(line)
      }
    }
    child.stdout.on('data', read('stdout'))
    child.stderr.on('data', read('stderr'))
    child.on('error', (error) =>
      resolve(`Could not run Docker to build the sandbox image: ${error.message}`)
    )
    child.on('close', (code) =>
      resolve(
        code === 0
          ? null
          : `Building the sandbox image failed (exit code ${code}).\n${tail.trim()}`.trim()
      )
    )
  })
}

/** The installed Agent SDK version, which the image must match, or null when it cannot be read. */
export async function readSdkVersion(
  appPath: string,
  readFile: (path: string) => Promise<string>
): Promise<string | null> {
  try {
    const manifest = JSON.parse(
      await readFile(`${appPath}/node_modules/@anthropic-ai/claude-agent-sdk/package.json`)
    ) as { version?: unknown }
    return typeof manifest.version === 'string' ? manifest.version : null
  } catch {
    return null
  }
}

export interface EnsureImageOptions {
  spawn: Spawn
  /** The app's root directory, which holds `sandbox/` and `node_modules/` when run from source. */
  appPath: string
  image: string
  exists(path: string): Promise<boolean>
  readFile(path: string): Promise<string>
  onProgress(line: string): void
}

/**
 * Build the image from the app's own `sandbox/` directory. Resolves to `undefined` when that is
 * not possible here (no bundled runner, or the SDK version is unknown), so the caller falls back
 * to telling the user to run `npm run build:sandbox`.
 */
export async function buildImageFromApp({
  appPath,
  exists,
  readFile,
  ...rest
}: EnsureImageOptions): Promise<string | null | undefined> {
  const contextDir = `${appPath}/sandbox`
  if (!(await exists(`${contextDir}/dist/runner.mjs`))) return undefined
  const sdkVersion = await readSdkVersion(appPath, readFile)
  if (!sdkVersion) return undefined
  return buildSandboxImage({ ...rest, contextDir, sdkVersion })
}
