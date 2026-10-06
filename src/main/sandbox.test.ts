import type { ChildProcessWithoutNullStreams } from 'node:child_process'
import { execFileSync } from 'node:child_process'
import { existsSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  checkDocker,
  checkDockerDetailed,
  isProcessAlive,
  readOnlyGitPaths,
  startSandbox,
  stopSandbox,
  sweepOrphans
} from './sandbox'

let root = ''
beforeEach(() => {
  root = realpathSync(mkdtempSync(join(tmpdir(), 'vettr-sandbox-test-')))
})
afterEach(() => {
  rmSync(root, { recursive: true, force: true })
})

const git = (cwd: string, ...args: string[]): void => {
  execFileSync('git', ['-c', 'user.name=t', '-c', 'user.email=t@t', ...args], { cwd })
}

describe('checkDocker', () => {
  it('passes when the daemon answers and the image exists', async () => {
    const exec = vi.fn().mockResolvedValue({})
    expect(await checkDocker(exec)).toBeNull()
    expect(exec).toHaveBeenCalledTimes(2)
  })

  it('reports Docker as unavailable when the daemon check fails', async () => {
    const exec = vi.fn().mockRejectedValue(new Error('no daemon'))
    expect(await checkDocker(exec)).toContain('Docker is not available')
    expect(exec).toHaveBeenCalledTimes(1)
  })

  it('tells the user how to build a missing image', async () => {
    const exec = vi.fn().mockResolvedValueOnce({}).mockRejectedValueOnce(new Error('no image'))
    expect(await checkDocker(exec)).toContain('npm run build:sandbox')
  })
})

describe('readOnlyGitPaths', () => {
  it('returns the git directory for a normal repo', async () => {
    git(root, 'init', '-q')
    expect(await readOnlyGitPaths(root)).toEqual([join(root, '.git'), join(root, '.git')])
  })

  it('also returns the common dir, worktree dir and .git file for a linked worktree', async () => {
    const main = join(root, 'main')
    const linked = join(root, 'linked')
    git(root, 'init', '-q', main)
    writeFileSync(join(main, 'f'), 'x')
    git(main, 'add', '.')
    git(main, 'commit', '-q', '-m', 'init')
    git(main, 'worktree', 'add', '-q', linked)
    expect(await readOnlyGitPaths(linked)).toEqual([
      join(main, '.git', 'worktrees', 'linked'),
      join(main, '.git'),
      join(linked, '.git')
    ])
  })

  it('rejects outside a repo', async () => {
    await expect(readOnlyGitPaths(root)).rejects.toThrow()
  })
})

describe('startSandbox', () => {
  it('spawns docker with the run arguments for the project', async () => {
    git(root, 'init', '-q')
    const transcriptsDir = join(root, 'data', 'transcripts')
    const child = {} as ChildProcessWithoutNullStreams
    const spawn = vi.fn().mockReturnValue(child)
    expect(
      await startSandbox({ project: root, ownerPid: 7, transcriptsDir, spawn, uid: 1, gid: 2 })
    ).toBe(child)
    const [file, args, options] = spawn.mock.calls[0]
    expect(file).toBe('docker')
    expect(options).toEqual({ stdio: 'pipe' })
    expect(args).toContain(`${root}:${root}`)
    expect(args).toContain(`${join(root, '.git')}:${join(root, '.git')}:ro`)
    expect(args).toContain('1:2')
    expect(args).toContain(`${transcriptsDir}:/vettr-config`)
    expect(existsSync(transcriptsDir)).toBe(true)
    expect(args[args.indexOf('--name') + 1]).toMatch(/^vettr-[0-9a-f]{8}$/)
  })
})

describe('stopSandbox', () => {
  it('sends SIGTERM to the docker process', () => {
    const kill = vi.fn()
    stopSandbox({ kill } as unknown as ChildProcessWithoutNullStreams)
    expect(kill).toHaveBeenCalledWith('SIGTERM')
  })
})

describe('sweepOrphans', () => {
  const listing = 'dead1 100\nlive1 200\nnopid\n'

  it('removes labelled containers whose owner is gone and keeps live ones', async () => {
    const exec = vi.fn().mockResolvedValueOnce(listing).mockResolvedValue('')
    const removed = await sweepOrphans(exec, (pid) => pid === 200)
    expect(removed).toBe(2)
    expect(exec.mock.calls[0]?.[1]).toContain('label=vettr.app=1')
    expect(exec).toHaveBeenCalledWith('docker', ['rm', '-f', 'dead1'])
    expect(exec).toHaveBeenCalledWith('docker', ['rm', '-f', 'nopid'])
    expect(exec).not.toHaveBeenCalledWith('docker', ['rm', '-f', 'live1'])
  })

  it('keeps going when one removal fails', async () => {
    const exec = vi
      .fn()
      .mockResolvedValueOnce('a 1\nb 2\n')
      .mockRejectedValueOnce(new Error('gone'))
      .mockResolvedValue('')
    expect(await sweepOrphans(exec, () => false)).toBe(2)
    expect(exec).toHaveBeenCalledWith('docker', ['rm', '-f', 'b'])
  })

  it('does nothing when there are no containers', async () => {
    const exec = vi.fn().mockResolvedValue('\n')
    expect(await sweepOrphans(exec, () => false)).toBe(0)
    expect(exec).toHaveBeenCalledTimes(1)
  })
})

describe('isProcessAlive', () => {
  it('is true when the signal is delivered', () => {
    expect(isProcessAlive(1, vi.fn() as unknown as typeof process.kill)).toBe(true)
  })

  it('is true when permission is denied, false when there is no such process', () => {
    const fail = (code: string) =>
      (() => {
        throw Object.assign(new Error(code), { code })
      }) as unknown as typeof process.kill
    expect(isProcessAlive(1, fail('EPERM'))).toBe(true)
    expect(isProcessAlive(1, fail('ESRCH'))).toBe(false)
  })
})

describe('checkDockerDetailed', () => {
  it('tells a missing daemon from a missing image', async () => {
    const noDaemon = vi.fn().mockRejectedValue(new Error('no'))
    expect((await checkDockerDetailed(noDaemon))?.kind).toBe('docker')
    const noImage = vi.fn().mockResolvedValueOnce({}).mockRejectedValueOnce(new Error('no'))
    expect((await checkDockerDetailed(noImage))?.kind).toBe('image')
    expect(await checkDockerDetailed(vi.fn().mockResolvedValue({}))).toBeNull()
  })
})
