import type { ChildProcessWithoutNullStreams } from 'node:child_process'
import { EventEmitter } from 'node:events'
import { PassThrough } from 'node:stream'
import type { AgentEvent } from '@shared/agent'
import { describe, expect, it, vi } from 'vitest'
import { attempt, ClaudeAdapter, type ClaudeAdapterDeps } from './claudeAdapter'

class FakeContainer extends EventEmitter {
  stdin = new PassThrough()
  stdout = new PassThrough()
  stderr = new PassThrough()
  written = ''
  constructor() {
    super()
    this.stdin.on('data', (chunk) => {
      this.written += chunk.toString()
    })
  }
  commands(): unknown[] {
    return this.written
      .split('\n')
      .filter(Boolean)
      .map((line) => JSON.parse(line))
  }
  runnerSays(...events: unknown[]): void {
    for (const event of events) this.stdout.write(`${JSON.stringify(event)}\n`)
  }
}

const tick = (): Promise<void> => new Promise((resolve) => setImmediate(resolve))

function setup(overrides: Partial<ClaudeAdapterDeps> = {}) {
  const container = new FakeContainer()
  const stopSandbox = vi.fn(() => container.emit('close', 143))
  const deps: ClaudeAdapterDeps = {
    checkDocker: async () => null,
    startSandbox: async () => container as unknown as ChildProcessWithoutNullStreams,
    getApiKey: async () => 'sk-key',
    stopSandbox,
    ...overrides
  }
  const adapter = new ClaudeAdapter(deps)
  const events: AgentEvent[] = []
  adapter.onEvent((event) => events.push(event))
  return { adapter, container, events, stopSandbox }
}

describe('ClaudeAdapter.start', () => {
  it('sends init with the key first, then the prompt', async () => {
    const { adapter, container } = setup()
    await adapter.start('build it', '/proj')
    expect(container.commands()).toEqual([
      { type: 'init', apiKey: 'sk-key', cwd: '/proj' },
      { type: 'prompt', text: 'build it' }
    ])
  })

  it('fails with the Docker problem and does not start a container', async () => {
    const startSandbox = vi.fn()
    const { adapter } = setup({ checkDocker: async () => 'Docker is not available.', startSandbox })
    await expect(adapter.start('p', '/proj')).rejects.toThrow('Docker is not available.')
    expect(startSandbox).not.toHaveBeenCalled()
  })

  it('fails when no API key is saved', async () => {
    const { adapter } = setup({ getApiKey: async () => null })
    await expect(adapter.start('p', '/proj')).rejects.toThrow('No Anthropic API key')
  })

  it('rejects a second start while a session is running', async () => {
    const { adapter } = setup()
    await adapter.start('p', '/proj')
    await expect(adapter.start('p', '/proj')).rejects.toThrow('already running')
  })

  it('rejects a start that loses a race with another start', async () => {
    const { adapter } = setup()
    const first = adapter.start('p', '/proj')
    await expect(adapter.start('p', '/proj')).rejects.toThrow('already running')
    await first
  })

  it('does not leave a session behind when the container cannot be started', async () => {
    let fail = true
    const { adapter, container } = setup({
      startSandbox: async () => {
        if (fail) throw new Error('not a repo')
        return container as unknown as ChildProcessWithoutNullStreams
      }
    })
    await expect(adapter.start('p', '/proj')).rejects.toThrow('not a repo')
    fail = false
    await adapter.start('p', '/proj')
  })
})

describe('ClaudeAdapter events', () => {
  it('forwards runner events and skips malformed lines', async () => {
    const { adapter, container, events } = setup()
    await adapter.start('p', '/proj')
    container.stdout.write('garbage\n{"type":"bogus"}\n')
    container.runnerSays({ type: 'text', text: 'hi' }, { type: 'turn-finished' })
    await tick()
    expect(events).toEqual([{ type: 'text', text: 'hi' }, { type: 'turn-finished' }])
  })

  it('stops delivering events to an unsubscribed listener', async () => {
    const { adapter, container } = setup()
    const seen: AgentEvent[] = []
    const unsubscribe = adapter.onEvent((event) => seen.push(event))
    await adapter.start('p', '/proj')
    unsubscribe()
    container.runnerSays({ type: 'turn-finished' })
    await tick()
    expect(seen).toEqual([])
  })

  it('reports a clean exit without an error', async () => {
    const { adapter, container, events } = setup()
    await adapter.start('p', '/proj')
    container.emit('close', 0)
    expect(events).toEqual([{ type: 'exited', code: 0 }])
  })

  it('reports an unexpected exit with the stderr tail', async () => {
    const { adapter, container, events } = setup()
    await adapter.start('p', '/proj')
    container.stderr.write(Buffer.from('boom: image gone\n'))
    await tick()
    container.emit('close', 125)
    expect(events).toEqual([
      {
        type: 'error',
        message: 'The sandbox stopped unexpectedly (exit code 125).\nboom: image gone'
      },
      { type: 'exited', code: 125 }
    ])
  })

  it('reports a signal kill without an exit code', async () => {
    const { adapter, container, events } = setup()
    await adapter.start('p', '/proj')
    container.emit('close', null)
    expect(events[0]).toEqual({ type: 'error', message: 'The sandbox stopped unexpectedly.' })
  })

  it('keeps only the end of a very long stderr', async () => {
    const { adapter, container, events } = setup()
    await adapter.start('p', '/proj')
    container.stderr.write(`${'x'.repeat(5000)}END`)
    await tick()
    container.emit('close', 1)
    const message = (events[0] as { message: string }).message
    expect(message.endsWith('END')).toBe(true)
    expect(message.length).toBeLessThan(2100)
  })

  it('does not add a second error when the runner already reported one', async () => {
    const { adapter, container, events } = setup()
    await adapter.start('p', '/proj')
    container.runnerSays({ type: 'error', message: 'bad key' })
    await tick()
    container.emit('close', 1)
    expect(events.map((e) => e.type)).toEqual(['error', 'exited'])
  })

  it('reports Docker failing to run', async () => {
    const { adapter, container, events } = setup()
    await adapter.start('p', '/proj')
    container.emit('error', new Error('spawn docker ENOENT'))
    container.emit('close', null)
    expect(events).toEqual([
      { type: 'error', message: 'Could not run Docker: spawn docker ENOENT' },
      { type: 'exited', code: null }
    ])
  })

  it('allows a new session after the container exits', async () => {
    const { adapter, container } = setup()
    await adapter.start('p', '/proj')
    container.emit('close', 0)
    await adapter.start('again', '/proj')
    expect(container.commands().at(-1)).toEqual({ type: 'prompt', text: 'again' })
  })

  it('survives writing to a dead container', async () => {
    const { adapter, container } = setup()
    await adapter.start('p', '/proj')
    expect(() => container.stdin.emit('error', new Error('EPIPE'))).not.toThrow()
  })
})

describe('ClaudeAdapter commands', () => {
  it('sends follow-ups and interrupts', async () => {
    const { adapter, container } = setup()
    await adapter.start('p', '/proj')
    await adapter.send('and also this')
    await adapter.interrupt()
    expect(container.commands().slice(2)).toEqual([
      { type: 'prompt', text: 'and also this' },
      { type: 'interrupt' }
    ])
  })

  it('refuses to send or interrupt without a session', async () => {
    const { adapter } = setup()
    await expect(adapter.send('x')).rejects.toThrow('No session is running')
    await expect(adapter.interrupt()).rejects.toThrow('No session is running')
  })

  it('accepts approval responses as a no-op', async () => {
    await expect(setup().adapter.respondToApproval()).resolves.toBeUndefined()
  })
})

describe('ClaudeAdapter.stop', () => {
  it('does nothing without a session', async () => {
    const { adapter, stopSandbox } = setup()
    await adapter.stop()
    expect(stopSandbox).not.toHaveBeenCalled()
  })

  it('stops the container and reports a quiet exit', async () => {
    const { adapter, events, stopSandbox } = setup()
    await adapter.start('p', '/proj')
    await adapter.stop()
    expect(stopSandbox).toHaveBeenCalledOnce()
    expect(events).toEqual([{ type: 'exited', code: 143 }])
  })

  it('reports a later unexpected exit again after a stopped session', async () => {
    const { adapter, container, events } = setup()
    await adapter.start('p', '/proj')
    await adapter.stop()
    await adapter.start('p', '/proj')
    container.emit('close', 1)
    expect(events.at(-2)).toMatchObject({ type: 'error' })
  })
})

describe('attempt', () => {
  it('resolves to null on success', async () => {
    expect(await attempt(async () => {})).toBeNull()
  })

  it('resolves to the message of an Error', async () => {
    expect(
      await attempt(async () => {
        throw new Error('nope')
      })
    ).toBe('nope')
  })

  it('stringifies anything else that was thrown', async () => {
    expect(
      await attempt(async () => {
        throw 'plain'
      })
    ).toBe('plain')
  })
})
