import { describe, expect, it } from 'vitest'
import { buildRunArgs, SANDBOX_IMAGE } from './sandbox'

const base = { name: 'c1', project: '/work/p', readOnlyPaths: [], uid: 1000, gid: 1001 }

describe('buildRunArgs', () => {
  it('runs as the host user with the project mounted at the same path', () => {
    const args = buildRunArgs(base)
    expect(args.slice(0, 3)).toEqual(['run', '--rm', '-i'])
    expect(args).toContain('--init')
    expect(args.join(' ')).toContain('--name c1')
    expect(args.join(' ')).toContain('--user 1000:1001')
    expect(args.join(' ')).toContain('--workdir /work/p')
    expect(args.join(' ')).toContain('-v /work/p:/work/p')
  })

  it('drops capabilities and ends with the image so the runner gets no extra args', () => {
    const args = buildRunArgs(base)
    expect(args.join(' ')).toContain('--cap-drop ALL')
    expect(args.join(' ')).toContain('--security-opt no-new-privileges')
    expect(args.join(' ')).toContain('--security-opt label=disable')
    expect(args.at(-1)).toBe(SANDBOX_IMAGE)
  })

  it('mounts read-only paths after the project mount, without duplicates', () => {
    const args = buildRunArgs({
      ...base,
      readOnlyPaths: ['/work/p/.git', '/work/p/.git', '/main/.git/worktrees/x']
    })
    const mounts = args.filter((_, i) => args[i - 1] === '-v')
    expect(mounts).toEqual([
      '/work/p:/work/p',
      '/work/p/.git:/work/p/.git:ro',
      '/main/.git/worktrees/x:/main/.git/worktrees/x:ro'
    ])
  })
})
