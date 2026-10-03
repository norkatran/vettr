import { EventEmitter } from 'node:events'
import { expect, it, vi } from 'vitest'

const fake = Object.assign(new EventEmitter(), { close: vi.fn(async () => undefined) })
vi.mock('chokidar', () => ({ default: { watch: () => fake } }))

it('survives watcher errors instead of throwing', async () => {
  const { watchTree } = await import('./watcher')
  const pending = watchTree('/x', vi.fn())
  expect(() => fake.emit('error', new Error('ENOSPC'))).not.toThrow()
  fake.emit('ready')
  const stop = await pending
  await stop()
  expect(fake.close).toHaveBeenCalled()
})
