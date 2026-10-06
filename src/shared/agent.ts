/** Events an agent session emits; the UI depends only on these, never on a specific agent. */
export type AgentEvent =
  /** The SDK session ID, reported once per session; it is what `resume` takes later. */
  | { type: 'session-started'; sessionId: string }
  | { type: 'text'; text: string }
  | { type: 'tool-started'; id: string; name: string; input: unknown }
  | { type: 'tool-finished'; id: string; output: string; isError: boolean }
  | { type: 'file-edited'; path: string }
  | { type: 'turn-finished' }
  | { type: 'error'; message: string }
  /** The agent process went away; `code` is null when it was killed by a signal. */
  | { type: 'exited'; code: number | null }

/** Adapter between the app and a concrete agent (the first one wraps the Claude Agent SDK). */
export interface AgentAdapter {
  /** Prewarm an idle agent for `cwd` (optionally resuming `resume`) so `start` is fast. */
  warm(cwd: string, resume?: string): Promise<void>
  /** Begin a session in `cwd` with the first prompt (using the warm agent when one matches), optionally resuming SDK session `resume`. */
  start(prompt: string, cwd: string, resume?: string): Promise<void>
  /** Send a follow-up in the same session, including batched review comments. */
  send(message: string): Promise<void>
  /** Stop the current turn without ending the session. */
  interrupt(): Promise<void>
  /** Subscribe to events; returns an unsubscribe function. */
  onEvent(listener: (event: AgentEvent) => void): () => void
  /** Kept for later: the MVP sandbox grants full permissions, so nothing requests approval yet. */
  respondToApproval(id: string, allow: boolean): Promise<void>
  /** End the session and release its resources (the container). */
  stop(): Promise<void>
}

/** Messages the main process writes to the runner's stdin, one JSON object per line. */
export type RunnerCommand =
  /**
   * Always first. The credential (an API key or an OAuth token) travels over stdin so it never
   * shows up in `docker inspect`.
   */
  | { type: 'init'; credential: string; cwd: string; resume?: string }
  | { type: 'prompt'; text: string }
  | { type: 'interrupt' }

const EVENT_TYPES = new Set([
  'session-started',
  'text',
  'tool-started',
  'tool-finished',
  'file-edited',
  'turn-finished',
  'error',
  'exited'
])

/** Serialise a message as one protocol line (JSON followed by a newline). */
export function encodeLine(message: RunnerCommand | AgentEvent): string {
  return `${JSON.stringify(message)}\n`
}

function parseObject(line: string): Record<string, unknown> | null {
  let value: unknown
  try {
    value = JSON.parse(line)
  } catch {
    return null
  }
  return typeof value === 'object' && value !== null ? (value as Record<string, unknown>) : null
}

/** Parse one line from the runner into an event, or null if it is not a well-formed event. */
export function parseEventLine(line: string): AgentEvent | null {
  const value = parseObject(line)
  return typeof value?.type === 'string' && EVENT_TYPES.has(value.type)
    ? (value as AgentEvent)
    : null
}

/** Parse one line from the main process into a command, or null if it is not a well-formed command. */
export function parseCommandLine(line: string): RunnerCommand | null {
  const value = parseObject(line)
  if (value?.type === 'interrupt') return { type: 'interrupt' }
  if (value?.type === 'prompt' && typeof value.text === 'string') {
    return { type: 'prompt', text: value.text }
  }
  if (
    value?.type === 'init' &&
    typeof value.credential === 'string' &&
    typeof value.cwd === 'string'
  ) {
    const init: RunnerCommand = { type: 'init', credential: value.credential, cwd: value.cwd }
    if (typeof value.resume === 'string' && value.resume) init.resume = value.resume
    return init
  }
  return null
}

/**
 * Reassembles protocol lines from stream chunks, which can split or merge lines anywhere.
 * Blank lines are skipped; a trailing partial line waits for its newline.
 */
export class LineBuffer {
  private pending = ''

  push(chunk: string): string[] {
    const parts = (this.pending + chunk).split('\n')
    this.pending = parts.pop() as string // split always yields at least one part
    return parts.filter((line) => line.trim() !== '')
  }
}
