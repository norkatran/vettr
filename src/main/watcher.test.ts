import { execFileSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createProjectWatcher, isIgnored, watchTree } from './watcher'

let root = ''

/**
 * Native file events are not instant: right after `ready` the first writes can be missed or
 * arrive late, and a burst can be split into batches. The tests allow for that with a short
 * warm-up, a generous wait for the first call and a debounce well above the batch gap.
 */
const WARM_UP_MS = 100
const FIRST_CALL_TIMEOUT_MS = 5000
const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms))

beforeEach(() => {
  root = realpathSync(mkdtempSync(join(tmpdir(), 'vettr-watch-test-')))
})

afterEach(() => {
  rmSync(root, { recursive: true, force: true })
})

describe('isIgnored', () => {
  it('ignores listed paths and everything under them', () => {
    const ignored = new Set(['vendor', 'pkg/node_modules', 'debug.log'])
    expect(isIgnored(root, join(root, 'vendor'), ignored)).toBe(true)
    expect(isIgnored(root, join(root, 'vendor', 'a', 'b.php'), ignored)).toBe(true)
    expect(isIgnored(root, join(root, 'pkg', 'node_modules', 'x.js'), ignored)).toBe(true)
    expect(isIgnored(root, join(root, 'debug.log'), ignored)).toBe(true)
  })

  it('skips nothing by name, so node_modules is watched unless git ignores it', () => {
    expect(isIgnored(root, join(root, 'node_modules', 'x.js'))).toBe(false)
    expect(isIgnored(root, join(root, 'vendor', 'x.php'))).toBe(false)
    const ignored = new Set(['vendor'])
    expect(isIgnored(root, join(root, 'node_modules'), ignored)).toBe(false)
    expect(isIgnored(root, join(root, 'src', 'vendor.ts'), ignored)).toBe(false)
  })

  it('keeps ordinary files and the root itself', () => {
    expect(isIgnored(root, root)).toBe(false)
    expect(isIgnored(root, join(root, 'src', 'a.ts'))).toBe(false)
  })

  it('ignores .git contents except HEAD and index', () => {
    expect(isIgnored(root, join(root, '.git'))).toBe(false)
    expect(isIgnored(root, join(root, '.git', 'HEAD'))).toBe(false)
    expect(isIgnored(root, join(root, '.git', 'index'))).toBe(false)
    expect(isIgnored(root, join(root, '.git', 'objects'))).toBe(true)
    expect(isIgnored(root, join(root, '.git', 'refs', 'HEAD'))).toBe(true)
  })
})

describe('watchTree', () => {
  it('fires once for a burst of changes', async () => {
    const onChange = vi.fn()
    const stop = await watchTree(root, onChange, 200)
    await sleep(WARM_UP_MS)
    writeFileSync(join(root, 'a.txt'), '1')
    writeFileSync(join(root, 'b.txt'), '2')
    await vi.waitFor(() => expect(onChange).toHaveBeenCalled(), {
      timeout: FIRST_CALL_TIMEOUT_MS
    })
    await sleep(600)
    expect(onChange).toHaveBeenCalledTimes(1)
    await stop()
  })

  it('does not fire for paths git ignores, and does for the rest', async () => {
    execFileSync('git', ['init', '-q', root])
    writeFileSync(join(root, '.gitignore'), 'vendor/\nnode_modules/\n')
    mkdirSync(join(root, 'vendor'))
    mkdirSync(join(root, 'node_modules'))
    mkdirSync(join(root, 'tracked_deps'))
    const onChange = vi.fn()
    const stop = await watchTree(root, onChange, 50)
    await sleep(WARM_UP_MS)
    writeFileSync(join(root, 'vendor', 'x.php'), '1')
    writeFileSync(join(root, 'node_modules', 'x.js'), '1')
    await sleep(300)
    expect(onChange).not.toHaveBeenCalled()
    writeFileSync(join(root, 'tracked_deps', 'y.js'), '1')
    await vi.waitFor(() => expect(onChange).toHaveBeenCalled(), {
      timeout: FIRST_CALL_TIMEOUT_MS
    })
    await stop()
  })

  it('watches node_modules when .gitignore does not exclude it', async () => {
    execFileSync('git', ['init', '-q', root])
    mkdirSync(join(root, 'node_modules'))
    const onChange = vi.fn()
    const stop = await watchTree(root, onChange, 50)
    await sleep(WARM_UP_MS)
    writeFileSync(join(root, 'node_modules', 'x.js'), '1')
    await vi.waitFor(() => expect(onChange).toHaveBeenCalled(), {
      timeout: FIRST_CALL_TIMEOUT_MS
    })
    await stop()
  })

  it('does not fire after being stopped, even with a change pending', async () => {
    const onChange = vi.fn()
    const stop = await watchTree(root, onChange, 100)
    writeFileSync(join(root, 'a.txt'), '1')
    await new Promise((r) => setTimeout(r, 30))
    await stop()
    await new Promise((r) => setTimeout(r, 200))
    expect(onChange).not.toHaveBeenCalled()
  })

  it('uses the default debounce when none is given', async () => {
    const onChange = vi.fn()
    const stop = await watchTree(root, onChange)
    await sleep(WARM_UP_MS)
    writeFileSync(join(root, 'a.txt'), '1')
    await vi.waitFor(() => expect(onChange).toHaveBeenCalledTimes(1), {
      timeout: FIRST_CALL_TIMEOUT_MS
    })
    await stop()
  })
})

describe('createProjectWatcher', () => {
  const deferred = () => {
    let resolve!: () => void
    const promise = new Promise<void>((r) => {
      resolve = r
    })
    return { promise, resolve }
  }

  it('closes the previous watcher when switching', async () => {
    const stops: Record<string, ReturnType<typeof vi.fn>> = {}
    const start = vi.fn(async (path: string) => {
      stops[path] = vi.fn(async () => undefined)
      return stops[path] as () => Promise<void>
    })
    const watcher = createProjectWatcher(() => undefined, start)
    await watcher.watch('/a')
    await watcher.watch('/b')
    expect(stops['/a']).toHaveBeenCalledTimes(1)
    expect(stops['/b']).not.toHaveBeenCalled()
    await watcher.close()
    expect(stops['/b']).toHaveBeenCalledTimes(1)
  })

  it('does not leak a watcher that is still starting when another project opens', async () => {
    const gate = deferred()
    const stops: Record<string, ReturnType<typeof vi.fn>> = {}
    const start = vi.fn(async (path: string) => {
      if (path === '/a') await gate.promise
      stops[path] = vi.fn(async () => undefined)
      return stops[path] as () => Promise<void>
    })
    const watcher = createProjectWatcher(() => undefined, start)
    const first = watcher.watch('/a')
    await vi.waitFor(() => expect(start).toHaveBeenCalledWith('/a', expect.any(Function)))
    const second = watcher.watch('/b')
    gate.resolve()
    await Promise.all([first, second])
    expect(stops['/a']).toHaveBeenCalledTimes(1)
    expect(stops['/b']).not.toHaveBeenCalled()
    await watcher.close()
    expect(stops['/b']).toHaveBeenCalledTimes(1)
  })

  it('skips projects replaced before their turn and keeps going after a failed start', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined)
    const stop = vi.fn(async () => undefined)
    const start = vi.fn(async (path: string) => {
      if (path === '/bad') throw new Error('boom')
      return stop
    })
    const watcher = createProjectWatcher(() => undefined, start)
    void watcher.watch('/bad')
    void watcher.watch('/skipped')
    await watcher.watch('/c')
    expect(start.mock.calls.map(([path]) => path)).toEqual(['/c'])
    await watcher.close()
    expect(stop).toHaveBeenCalledTimes(1)

    await watcher.watch('/bad')
    await watcher.watch('/d')
    expect(start.mock.calls.map(([path]) => path)).toEqual(['/c', '/bad', '/d'])
    error.mockRestore()
  })
})
