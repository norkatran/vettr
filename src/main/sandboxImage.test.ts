import type { ChildProcessWithoutNullStreams } from 'node:child_process'
import { EventEmitter } from 'node:events'
import { PassThrough } from 'node:stream'
import { describe, expect, it, vi } from 'vitest'
import {
  buildImageArgs,
  buildImageFromApp,
  buildSandboxImage,
  progressLine,
  readSdkVersion
} from './sandboxImage'

class FakeBuild extends EventEmitter {
  stdout = new PassThrough()
  stderr = new PassThrough()
}

function fakeSpawn() {
  const child = new FakeBuild()
  const spawn = vi.fn(() => child as unknown as ChildProcessWithoutNullStreams)
  return { child, spawn }
}

describe('buildImageArgs', () => {
  it('passes the SDK version and builds from the context directory', () => {
    expect(buildImageArgs('img', '1.2.3', '/ctx')).toEqual([
      'build',
      '--progress=plain',
      '-t',
      'img',
      '--build-arg',
      'SDK_VERSION=1.2.3',
      '/ctx'
    ])
  })
})

describe('progressLine', () => {
  it('trims, skips blanks and shortens long lines', () => {
    expect(progressLine('  #5 RUN npm install  ')).toBe('#5 RUN npm install')
    expect(progressLine('   ')).toBeNull()
    expect(progressLine('x'.repeat(150))).toBe(`${'x'.repeat(99)}…`)
  })
})

describe('buildSandboxImage', () => {
  const options = (spawn: ReturnType<typeof fakeSpawn>['spawn'], onProgress = vi.fn()) => ({
    spawn,
    contextDir: '/ctx',
    image: 'img',
    sdkVersion: '1.0.0',
    onProgress
  })

  it('reports progress lines from both streams and resolves null on success', async () => {
    const { child, spawn } = fakeSpawn()
    const onProgress = vi.fn()
    const done = buildSandboxImage(options(spawn, onProgress))
    child.stdout.write('#1 [1/3] FROM node\n#2 split ')
    child.stdout.write('line\n\n')
    child.stderr.write('#3 from stderr\n')
    child.stdout.write('partial without newline')
    await new Promise((resolve) => setImmediate(resolve))
    child.emit('close', 0)
    expect(await done).toBeNull()
    expect(onProgress.mock.calls.map((c) => c[0])).toEqual([
      '#1 [1/3] FROM node',
      '#2 split line',
      '#3 from stderr'
    ])
    expect(onProgress.mock.calls.map((c) => c[0])).not.toContain('partial without newline')
    expect(spawn).toHaveBeenCalledWith('docker', buildImageArgs('img', '1.0.0', '/ctx'), {
      stdio: 'pipe'
    })
  })

  it('includes the end of the output when the build fails', async () => {
    const { child, spawn } = fakeSpawn()
    const done = buildSandboxImage(options(spawn))
    child.stderr.write('E: package not found\n')
    await new Promise((resolve) => setImmediate(resolve))
    child.emit('close', 100)
    expect(await done).toBe(
      'Building the sandbox image failed (exit code 100).\nE: package not found'
    )
  })

  it('keeps only the tail of very long output', async () => {
    const { child, spawn } = fakeSpawn()
    const done = buildSandboxImage(options(spawn))
    child.stdout.write(`${'a'.repeat(5000)}END\n`)
    await new Promise((resolve) => setImmediate(resolve))
    child.emit('close', 1)
    const message = (await done) as string
    expect(message.endsWith('END')).toBe(true)
    expect(message.length).toBeLessThan(2100)
  })

  it('reports Docker failing to run', async () => {
    const { child, spawn } = fakeSpawn()
    const done = buildSandboxImage(options(spawn))
    child.emit('error', new Error('ENOENT'))
    expect(await done).toContain('ENOENT')
  })
})

describe('readSdkVersion', () => {
  it('reads the installed version', async () => {
    const readFile = vi.fn(async () => '{"version":"0.3.288"}')
    expect(await readSdkVersion('/app', readFile)).toBe('0.3.288')
    expect(readFile).toHaveBeenCalledWith(
      '/app/node_modules/@anthropic-ai/claude-agent-sdk/package.json'
    )
  })

  it('is null when unreadable, malformed or without a version', async () => {
    expect(await readSdkVersion('/app', async () => Promise.reject(new Error('x')))).toBeNull()
    expect(await readSdkVersion('/app', async () => 'not json')).toBeNull()
    expect(await readSdkVersion('/app', async () => '{"version":3}')).toBeNull()
  })
})

describe('buildImageFromApp', () => {
  it('builds from the app sandbox directory', async () => {
    const { child, spawn } = fakeSpawn()
    const done = buildImageFromApp({
      spawn,
      appPath: '/app',
      image: 'img',
      exists: async () => true,
      readFile: async () => '{"version":"9.9.9"}',
      onProgress: () => {}
    })
    await vi.waitFor(() => expect(spawn).toHaveBeenCalled())
    expect(spawn.mock.calls[0]).toEqual([
      'docker',
      buildImageArgs('img', '9.9.9', '/app/sandbox'),
      { stdio: 'pipe' }
    ])
    child.emit('close', 0)
    expect(await done).toBeNull()
  })

  it('does nothing without the bundled runner or the SDK version', async () => {
    const { spawn } = fakeSpawn()
    const base = { spawn, appPath: '/app', image: 'img', onProgress: () => {} }
    expect(
      await buildImageFromApp({ ...base, exists: async () => false, readFile: async () => '{}' })
    ).toBeUndefined()
    expect(
      await buildImageFromApp({ ...base, exists: async () => true, readFile: async () => '{}' })
    ).toBeUndefined()
    expect(spawn).not.toHaveBeenCalled()
  })
})
