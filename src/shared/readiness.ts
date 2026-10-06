/** Lifecycle of the sandbox agent, owned by the main process and pushed to the renderer. */
export type ReadinessStatus = 'idle' | 'starting' | 'ready' | 'stopping' | 'error'

/** Why the agent is not ready (absent when it is). */
export type ReadinessReason =
  | 'no-key'
  | 'docker-unavailable'
  | 'building-image'
  | 'starting'
  | 'crashed-restarting'
  | 'offline'
  | 'error'

export interface Readiness {
  status: ReadinessStatus
  reason?: ReadinessReason
  /** A user-facing explanation, for example Docker's own message. */
  message?: string
}

export const INITIAL_READINESS: Readiness = { status: 'idle' }

/** Whether prompts, review comments and any other input directed at the agent are allowed. */
export function canDirectAgent(readiness: Readiness): boolean {
  return readiness.status === 'ready'
}

const REASON_TEXT: Record<ReadinessReason, string> = {
  'no-key': 'Add an API key or token to start the agent.',
  'docker-unavailable': 'Docker is not available.',
  'building-image': 'Building the sandbox image…',
  starting: 'Starting the agent…',
  'crashed-restarting': 'The agent stopped unexpectedly and is restarting…',
  offline: 'You appear to be offline.',
  error: 'The agent could not be started.'
}

/** Short text saying why agent inputs are disabled, or null when they are enabled. */
export function readinessBlockReason(readiness: Readiness): string | null {
  if (canDirectAgent(readiness)) return null
  if (readiness.message) return readiness.message
  if (readiness.reason) return REASON_TEXT[readiness.reason]
  return readiness.status === 'stopping' ? 'Stopping the agent…' : 'The agent is not running.'
}

export interface ReadinessNotice {
  title: string
  detail: string
  level: 'error' | 'info'
}

export interface TransitionResult {
  notice: ReadinessNotice | null
  /** Whether a sandbox image build has been announced and has not finished (carry to the next call). */
  building: boolean
}

/**
 * What to tell the user when readiness goes from `before` to `next`, if anything. `building` is
 * the flag returned by the previous call: the image build ends in `starting` and then `ready`,
 * so "built" is announced when `ready` arrives while it is still set.
 */
export function describeTransition(
  before: Readiness,
  next: Readiness,
  building: boolean
): TransitionResult {
  const isBuilding =
    next.reason === 'building-image' ? true : next.status === 'starting' && building
  const keep = (notice: ReadinessNotice | null): TransitionResult => ({
    notice,
    building: isBuilding
  })
  if (next.reason === 'building-image') {
    return keep(
      before.reason === 'building-image'
        ? null
        : {
            title: 'Building the sandbox image',
            detail:
              'This happens once and can take a few minutes. The agent starts when it is done.',
            level: 'info'
          }
    )
  }
  if (next.status === 'ready') {
    if (building) {
      return keep({ title: 'Sandbox image built', detail: 'The agent is ready.', level: 'info' })
    }
    return keep(
      before.reason === 'crashed-restarting'
        ? {
            title: 'Agent restarted',
            detail: 'The session can be resumed by sending a message.',
            level: 'info'
          }
        : null
    )
  }
  if (next.reason === 'docker-unavailable') {
    return keep(
      before.reason === 'docker-unavailable'
        ? null
        : {
            title: 'Docker is not available',
            detail: next.message ?? 'The agent cannot start.',
            level: 'error'
          }
    )
  }
  if (next.status === 'error') {
    return keep({
      title: 'The agent could not be started',
      detail: next.message ?? 'Unknown error.',
      level: 'error'
    })
  }
  if (next.reason === 'crashed-restarting' && before.reason !== 'crashed-restarting') {
    return keep({
      title: 'Agent stopped',
      detail: 'The agent stopped unexpectedly. Restarting it…',
      level: 'info'
    })
  }
  return keep(null)
}
