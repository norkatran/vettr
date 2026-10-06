import type { AgentEvent } from '@shared/agent'
import type { Readiness } from '@shared/readiness'
import type { DockerProblem } from '@shared/sandbox'
import { describe, expect, it, vi } from 'vitest'
import { AgentManager, type AgentManagerDeps, type AgentPort } from './agentManager'

function setup(
  options: {
    docker?: string | null
    key?: boolean
    checkDocker?: () => Promise<DockerProblem | null>
    buildImage?: AgentManagerDeps['buildImage']
    hasKey?: () => Promise<boolean>
  } = {}
) {
  let listener: (event: AgentEvent) => void = () => {}
  const log: string[] = []
  const agent = {
    warm: vi.fn(async (cwd: string, resume?: string) => {
      log.push(`warm ${cwd}${resume ? ` ${resume}` : ''}`)
    }),
    start: vi.fn(async () => {}),
    send: vi.fn(async () => {}),
    interrupt: vi.fn(async () => {}),
    stop: vi.fn(async () => {
      log.push('stop')
    }),
    onEvent: (l: (event: AgentEvent) => void) => {
      listener = l
      return () => {}
    }
  }
  const state = { docker: options.docker ?? null, key: options.key ?? true }
  const manager = new AgentManager({
    agent: agent as AgentPort,
    checkDocker:
      options.checkDocker ??
      (async () => (state.docker ? { kind: 'docker', message: state.docker } : null)),
    buildImage: options.buildImage,
    hasKey: options.hasKey ?? (async () => state.key)
  })
  const states: Readiness[] = []
  manager.onReadiness((r) => states.push(r))
  return { manager, agent, log, state, states, emit: (e: AgentEvent) => listener(e) }
}

describe('AgentManager launch', () => {
  it('prewarms the agent for the project and becomes ready', async () => {
    const { manager, log, states } = setup()
    await manager.setProject('/p')
    expect(log).toEqual(['stop', 'warm /p'])
    expect(states.map((s) => s.status)).toEqual(['starting', 'ready'])
    expect(manager.readiness).toEqual({ status: 'ready' })
  })

  it('reports Docker problems as an error and does not warm', async () => {
    const { manager, agent } = setup({ docker: 'Docker is not available.' })
    await manager.setProject('/p')
    expect(manager.readiness).toEqual({
      status: 'error',
      reason: 'docker-unavailable',
      message: 'Docker is not available.'
    })
    expect(agent.warm).not.toHaveBeenCalled()
  })

  it('waits for a key, then warms when the key is saved', async () => {
    const { manager, state, agent } = setup({ key: false })
    await manager.setProject('/p')
    expect(manager.readiness).toEqual({ status: 'idle', reason: 'no-key' })
    state.key = true
    await manager.keyChanged()
    expect(manager.readiness).toEqual({ status: 'ready' })
    expect(agent.warm).toHaveBeenCalledOnce()
  })

  it('reports a failed warm', async () => {
    const { manager, agent } = setup()
    agent.warm.mockRejectedValueOnce(new Error('boom'))
    await manager.setProject('/p')
    expect(manager.readiness).toEqual({ status: 'error', reason: 'error', message: 'boom' })
    agent.warm.mockRejectedValueOnce('plain')
    await manager.keyChanged()
    expect(manager.readiness.message).toBe('plain')
  })

  it('goes idle with no project', async () => {
    const { manager } = setup()
    await manager.setProject('/p')
    await manager.setProject(null)
    expect(manager.readiness).toEqual({ status: 'idle' })
  })
})

describe('AgentManager project changes', () => {
  it('does nothing for the same project', async () => {
    const { manager, agent } = setup()
    await manager.setProject('/p')
    await manager.setProject('/p')
    expect(agent.warm).toHaveBeenCalledOnce()
  })

  it('stops the old agent before warming for the new project', async () => {
    const { manager, log, states } = setup()
    await manager.setProject('/a')
    await manager.setProject('/b')
    expect(log).toEqual(['stop', 'warm /a', 'stop', 'warm /b'])
    expect(states.map((s) => s.status)).toContain('stopping')
  })

  it('discards work for a project replaced while it was starting', async () => {
    const { manager, log } = setup()
    const first = manager.setProject('/a')
    const second = manager.setProject('/b')
    await Promise.all([first, second])
    expect(log).toEqual(['stop', 'stop', 'warm /b'])
    expect(manager.readiness).toEqual({ status: 'ready' })
  })

  it('discards a warm that finishes after the project changed', async () => {
    const { manager, agent, log } = setup()
    let finish: () => void = () => {}
    agent.warm.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve
        })
    )
    const first = manager.setProject('/a')
    await vi.waitFor(() => expect(agent.warm).toHaveBeenCalled())
    const second = manager.setProject('/b')
    finish()
    await Promise.all([first, second])
    expect(log).toEqual(['stop', 'stop', 'warm /b'])
  })

  it('drops a failed warm for a replaced project', async () => {
    const { manager, agent } = setup()
    let fail: (e: Error) => void = () => {}
    agent.warm.mockImplementationOnce(
      () =>
        new Promise<void>((_, reject) => {
          fail = reject
        })
    )
    const first = manager.setProject('/a')
    await vi.waitFor(() => expect(agent.warm).toHaveBeenCalled())
    const second = manager.setProject('/b')
    fail(new Error('late'))
    await Promise.all([first, second])
    expect(manager.readiness).toEqual({ status: 'ready' })
  })

  it('drops a stale result after the Docker check', async () => {
    let release: (problem: DockerProblem | null) => void = () => {}
    const checkDocker = vi.fn(
      () =>
        new Promise<DockerProblem | null>((resolve) => {
          release = resolve
        })
    )
    const { manager } = setup({ checkDocker })
    const first = manager.setProject('/a')
    await vi.waitFor(() => expect(checkDocker).toHaveBeenCalled())
    const second = manager.setProject(null)
    release({ kind: 'docker', message: 'Docker is not available.' })
    await Promise.all([first, second])
    expect(manager.readiness).toEqual({ status: 'idle' })
  })

  it('drops a stale result after the key check', async () => {
    let release: (has: boolean) => void = () => {}
    const hasKey = vi.fn(
      () =>
        new Promise<boolean>((resolve) => {
          release = resolve
        })
    )
    const { manager } = setup({ hasKey })
    const first = manager.setProject('/a')
    await vi.waitFor(() => expect(hasKey).toHaveBeenCalled())
    const second = manager.setProject(null)
    release(false)
    await Promise.all([first, second])
    expect(manager.readiness).toEqual({ status: 'idle' })
  })
})

describe('AgentManager gating', () => {
  it('rejects agent-directed calls until ready', async () => {
    const { manager, agent } = setup({ key: false })
    await manager.setProject('/p')
    await expect(manager.start('go')).rejects.toThrow('API key')
    await expect(manager.send('go')).rejects.toThrow('API key')
    await expect(manager.interrupt()).rejects.toThrow('API key')
    expect(agent.start).not.toHaveBeenCalled()
  })

  it('starts on the warm agent without restarting it', async () => {
    const { manager, agent, log } = setup()
    await manager.setProject('/p')
    await manager.start('go')
    expect(agent.start).toHaveBeenCalledWith('go', '/p', undefined)
    expect(log).toEqual(['stop', 'warm /p'])
    expect(manager.isBusy).toBe(true)
  })

  it('restarts to resume a stored session', async () => {
    const { manager, agent, log } = setup()
    await manager.setProject('/p')
    await manager.start('go', 's1')
    expect(log).toEqual(['stop', 'warm /p', 'stop', 'warm /p s1'])
    expect(agent.start).toHaveBeenCalledWith('go', '/p', 's1')
  })

  it('restarts for a new session once the warm agent was used', async () => {
    const { manager, log } = setup()
    await manager.setProject('/p')
    await manager.start('one')
    await manager.start('two')
    expect(log).toEqual(['stop', 'warm /p', 'stop', 'warm /p'])
  })

  it('fails the start when the restart does not leave it ready', async () => {
    const { manager, agent } = setup()
    await manager.setProject('/p')
    agent.warm.mockRejectedValueOnce(new Error('no'))
    await expect(manager.start('go', 's1')).rejects.toThrow('no')
  })

  it('clears busy when start or send fail', async () => {
    const { manager, agent } = setup()
    await manager.setProject('/p')
    agent.start.mockRejectedValueOnce(new Error('x'))
    await expect(manager.start('go')).rejects.toThrow('x')
    expect(manager.isBusy).toBe(false)
    agent.send.mockRejectedValueOnce(new Error('y'))
    await expect(manager.send('go')).rejects.toThrow('y')
    expect(manager.isBusy).toBe(false)
  })

  it('sends follow-ups and interrupts when ready', async () => {
    const { manager, agent, emit } = setup()
    await manager.setProject('/p')
    await manager.send('more')
    expect(agent.send).toHaveBeenCalledWith('more')
    expect(manager.isBusy).toBe(true)
    await manager.interrupt()
    expect(agent.interrupt).toHaveBeenCalled()
    emit({ type: 'turn-finished' })
    expect(manager.isBusy).toBe(false)
  })
})

describe('AgentManager sessions', () => {
  it('keeps a never-used warm agent on new session', async () => {
    const { manager, agent } = setup()
    await manager.setProject('/p')
    await manager.newSession()
    expect(agent.warm).toHaveBeenCalledOnce()
  })

  it('replaces a used agent on new session', async () => {
    const { manager, agent } = setup()
    await manager.setProject('/p')
    await manager.send('x')
    await manager.newSession()
    expect(agent.warm).toHaveBeenCalledTimes(2)
  })

  it('warms again for a new session when the agent is not ready', async () => {
    const { manager, agent, state } = setup({ key: false })
    await manager.setProject('/p')
    state.key = true
    await manager.newSession()
    expect(agent.warm).toHaveBeenCalledOnce()
  })

  it('stops the agent for good on shutdown', async () => {
    const { manager, agent } = setup()
    await manager.setProject('/p')
    await manager.shutdown()
    expect(agent.stop).toHaveBeenCalledTimes(2)
    expect(manager.readiness).toEqual({ status: 'idle' })
  })

  it('stops listening to nothing: unsubscribes readiness listeners', async () => {
    const { manager } = setup()
    const seen: Readiness[] = []
    const off = manager.onReadiness((r) => seen.push(r))
    off()
    await manager.setProject('/p')
    expect(seen).toEqual([])
  })
})

describe('AgentManager crash recovery', () => {
  it('restarts a crashed agent resuming the stored session', async () => {
    const { manager, log, states, emit } = setup()
    await manager.setProject('/p')
    emit({ type: 'session-started', sessionId: 's1' })
    emit({ type: 'exited', code: 1 })
    expect(manager.readiness).toEqual({ status: 'starting', reason: 'crashed-restarting' })
    await vi.waitFor(() => expect(manager.readiness.status).toBe('ready'))
    expect(log.slice(-2)).toEqual(['stop', 'warm /p s1'])
    expect(states.some((s) => s.reason === 'crashed-restarting')).toBe(true)
  })

  it('ignores exits the manager asked for and exits while not ready', async () => {
    const { manager, agent, emit } = setup({ key: false })
    await manager.setProject('/p')
    emit({ type: 'exited', code: 0 })
    expect(agent.warm).not.toHaveBeenCalled()
  })

  it('does not treat its own stop as a crash', async () => {
    const { manager, agent, emit } = setup()
    agent.stop.mockImplementation(async () => emit({ type: 'exited', code: 143 }))
    await manager.setProject('/a')
    await manager.setProject('/b')
    expect(agent.warm).toHaveBeenCalledTimes(2)
  })

  it('gives up after repeated crashes without a finished turn', async () => {
    const { manager, emit } = setup()
    await manager.setProject('/p')
    for (let i = 0; i < 3; i++) {
      emit({ type: 'exited', code: 1 })
      await vi.waitFor(() => expect(manager.readiness.status).toBe('ready'))
    }
    emit({ type: 'exited', code: 1 })
    expect(manager.readiness).toEqual({
      status: 'error',
      reason: 'error',
      message: 'The agent keeps stopping unexpectedly.'
    })
  })

  it('resets the crash count when a turn finishes', async () => {
    const { manager, emit } = setup()
    await manager.setProject('/p')
    for (let i = 0; i < 5; i++) {
      emit({ type: 'exited', code: 1 })
      await vi.waitFor(() => expect(manager.readiness.status).toBe('ready'))
      emit({ type: 'turn-finished' })
    }
    expect(manager.readiness.status).toBe('ready')
  })
})

describe('AgentManager queue', () => {
  it('keeps processing after a transition throws', async () => {
    const { manager, agent } = setup()
    agent.stop.mockRejectedValueOnce(new Error('stuck'))
    await expect(manager.setProject('/a')).rejects.toThrow('stuck')
    await manager.setProject('/b')
    expect(manager.readiness).toEqual({ status: 'ready' })
  })
})

describe('AgentManager image build', () => {
  const missing = async () => ({ kind: 'image' as const, message: 'image is missing' })

  it('builds a missing image, reports progress and then warms', async () => {
    let built = false
    const buildImage = vi.fn(async (onProgress: (line: string) => void) => {
      onProgress('step 1')
      built = true
      return null
    })
    const { manager, states, agent } = setup({
      checkDocker: async () => (built ? null : { kind: 'image', message: 'image is missing' }),
      buildImage
    })
    await manager.setProject('/p')
    expect(states.map((s) => s.reason)).toEqual([
      'building-image',
      'building-image',
      'starting',
      undefined
    ])
    expect(states[1]?.message).toBe('Building the sandbox image… step 1')
    expect(agent.warm).toHaveBeenCalledOnce()
  })

  it('reports a failed build', async () => {
    const { manager, agent } = setup({
      checkDocker: missing,
      buildImage: async () => 'build failed'
    })
    await manager.setProject('/p')
    expect(manager.readiness).toEqual({ status: 'error', reason: 'error', message: 'build failed' })
    expect(agent.warm).not.toHaveBeenCalled()
  })

  it('keeps the original problem when the app cannot build the image', async () => {
    const { manager } = setup({ checkDocker: missing, buildImage: async () => undefined })
    await manager.setProject('/p')
    expect(manager.readiness).toEqual({
      status: 'error',
      reason: 'error',
      message: 'image is missing'
    })
  })

  it('does not build when there is no builder', async () => {
    const { manager } = setup({ checkDocker: missing })
    await manager.setProject('/p')
    expect(manager.readiness.message).toBe('image is missing')
  })

  it('stops when the project changes during the build', async () => {
    let finish: (failure: string | null) => void = () => {}
    let report: (line: string) => void = () => {}
    const buildImage = vi.fn(
      (onProgress: (line: string) => void) =>
        new Promise<string | null>((resolve) => {
          finish = resolve
          report = onProgress
        })
    )
    const { manager, agent, states } = setup({ checkDocker: missing, buildImage })
    const first = manager.setProject('/a')
    await vi.waitFor(() => expect(buildImage).toHaveBeenCalled())
    const second = manager.setProject(null)
    report('late line')
    finish(null)
    await Promise.all([first, second])
    expect(manager.readiness).toEqual({ status: 'idle' })
    expect(states.some((s) => s.message === 'Building the sandbox image… late line')).toBe(false)
    expect(agent.warm).not.toHaveBeenCalled()
  })

  it('stops when the project changes after the rebuild check', async () => {
    let calls = 0
    let release: (problem: null) => void = () => {}
    const { manager, agent } = setup({
      checkDocker: () => {
        if (calls++ === 0) return missing()
        return new Promise((resolve) => {
          release = resolve
        })
      },
      buildImage: async () => null
    })
    const first = manager.setProject('/a')
    await vi.waitFor(() => expect(calls).toBe(2))
    const second = manager.setProject(null)
    release(null)
    await Promise.all([first, second])
    expect(manager.readiness).toEqual({ status: 'idle' })
    expect(agent.warm).not.toHaveBeenCalled()
  })
})
