import { EventEmitter } from 'node:events'
import { expect, it, vi } from 'vitest'

const fake = Object.assign(new EventEmitter(), { close: vi.fn(async () => undefined) })
vi.mock('chokidar', () => ({ default: { watch: () => fake } }))
vi.mock('./git', () => ({ listIgnored: async () => [] }))

const errorLog = vi.spyOn(console, 'error').mockImplementation(() => undefined)

async function start() {
  const { watchTree } = await import('./watcher')
  const pending = watchTree('/x', vi.fn())
  await vi.waitFor(() => expect(fake.listenerCount('error')).toBeGreaterThan(0))
  return { pending, ready: () => fake.emit('ready') }
}

it('survives and logs watcher errors instead of throwing', async () => {
  fake.close.mockClear()
  const { pending, ready } = await start()
  expect(() => fake.emit('error', new Error('boom'))).not.toThrow()
  expect(errorLog).toHaveBeenCalled()
  expect(fake.close).not.toHaveBeenCalled()
  ready()
  const stop = await pending
  await stop()
  expect(fake.close).toHaveBeenCalledTimes(1)
})

it.each(['EMFILE', 'ENOSPC'])(
  'closes the watcher when it runs out of handles (%s)',
  async (code) => {
    fake.removeAllListeners()
    fake.close.mockClear()
    const { pending, ready } = await start()
    fake.emit('error', Object.assign(new Error(code), { code }))
    expect(fake.close).toHaveBeenCalledTimes(1)
    ready()
    await pending
  }
)
