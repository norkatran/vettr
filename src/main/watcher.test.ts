import { mkdirSync, mkdtempSync, realpathSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { isIgnored, watchTree } from './watcher'

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
  it('ignores dependencies at any depth', () => {
    expect(isIgnored(root, join(root, 'node_modules', 'x.js'))).toBe(true)
    expect(isIgnored(root, join(root, 'pkg', 'node_modules'))).toBe(true)
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

  it('does not fire for ignored paths', async () => {
    mkdirSync(join(root, 'node_modules'))
    const onChange = vi.fn()
    const stop = await watchTree(root, onChange, 50)
    writeFileSync(join(root, 'node_modules', 'x.js'), '1')
    await new Promise((r) => setTimeout(r, 200))
    expect(onChange).not.toHaveBeenCalled()
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
