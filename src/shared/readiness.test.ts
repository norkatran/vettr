import { describe, expect, it } from 'vitest'
import {
  canDirectAgent,
  describeTransition,
  INITIAL_READINESS,
  type Readiness,
  type ReadinessReason,
  readinessBlockReason
} from './readiness'

describe('canDirectAgent', () => {
  it('is true only when ready', () => {
    expect(canDirectAgent({ status: 'ready' })).toBe(true)
    for (const status of ['idle', 'starting', 'stopping', 'error'] as const) {
      expect(canDirectAgent({ status })).toBe(false)
    }
    expect(canDirectAgent(INITIAL_READINESS)).toBe(false)
  })
})

describe('readinessBlockReason', () => {
  it('is null when ready', () => {
    expect(readinessBlockReason({ status: 'ready' })).toBeNull()
  })

  it('prefers the explicit message', () => {
    expect(readinessBlockReason({ status: 'error', reason: 'error', message: 'boom' })).toBe('boom')
  })

  it('has text for every reason', () => {
    const reasons: ReadinessReason[] = [
      'no-key',
      'docker-unavailable',
      'building-image',
      'starting',
      'crashed-restarting',
      'offline',
      'error'
    ]
    for (const reason of reasons) {
      expect(readinessBlockReason({ status: 'error', reason })).toMatch(/\S/)
    }
  })

  it('falls back on the status when there is no reason', () => {
    expect(readinessBlockReason({ status: 'stopping' })).toBe('Stopping the agent…')
    expect(readinessBlockReason({ status: 'idle' })).toBe('The agent is not running.')
  })
})

describe('describeTransition', () => {
  const ready: Readiness = { status: 'ready' }
  const building: Readiness = { status: 'starting', reason: 'building-image' }
  const starting: Readiness = { status: 'starting', reason: 'starting' }

  it('announces an image build once and the end of it when ready', () => {
    const first = describeTransition(INITIAL_READINESS, building, false)
    expect(first.notice?.title).toBe('Building the sandbox image')
    expect(first.building).toBe(true)
    const progress = describeTransition(building, { ...building, message: 'step 2' }, true)
    expect(progress).toEqual({ notice: null, building: true })
    const warming = describeTransition(building, starting, true)
    expect(warming).toEqual({ notice: null, building: true })
    const done = describeTransition(starting, ready, true)
    expect(done.notice?.title).toBe('Sandbox image built')
    expect(done.building).toBe(false)
  })

  it('stops tracking a build that failed', () => {
    const failed = describeTransition(
      building,
      { status: 'error', reason: 'error', message: 'x' },
      true
    )
    expect(failed.notice?.level).toBe('error')
    expect(failed.building).toBe(false)
  })

  it('reports Docker being unavailable once', () => {
    const next: Readiness = { status: 'error', reason: 'docker-unavailable', message: 'no docker' }
    expect(describeTransition(INITIAL_READINESS, next, false).notice).toEqual({
      title: 'Docker is not available',
      detail: 'no docker',
      level: 'error'
    })
    expect(describeTransition(next, next, false).notice).toBeNull()
    expect(
      describeTransition(
        INITIAL_READINESS,
        { status: 'error', reason: 'docker-unavailable' },
        false
      ).notice?.detail
    ).toBe('The agent cannot start.')
  })

  it('reports other errors with their message or a fallback', () => {
    const withMessage = describeTransition(
      starting,
      { status: 'error', reason: 'error', message: 'boom' },
      false
    )
    expect(withMessage.notice?.detail).toBe('boom')
    expect(describeTransition(starting, { status: 'error' }, false).notice?.detail).toBe(
      'Unknown error.'
    )
  })

  it('reports a crash restart and its recovery', () => {
    const crashed: Readiness = { status: 'starting', reason: 'crashed-restarting' }
    expect(describeTransition(ready, crashed, false).notice?.title).toBe('Agent stopped')
    expect(describeTransition(crashed, crashed, false).notice).toBeNull()
    expect(describeTransition(crashed, ready, false).notice?.title).toBe('Agent restarted')
  })

  it('says nothing for ordinary transitions', () => {
    expect(describeTransition(INITIAL_READINESS, starting, false).notice).toBeNull()
    expect(describeTransition(starting, ready, false).notice).toBeNull()
  })
})
