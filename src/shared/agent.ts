/** Events an agent session emits; the UI depends only on these, never on a specific agent. */
export type AgentEvent =
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
  /** Begin a session in `cwd` with the first prompt. */
  start(prompt: string, cwd: string): Promise<void>
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
  /** Always first. The key travels over stdin so it never shows up in `docker inspect`. */
  | { type: 'init'; apiKey: string; cwd: string }
  | { type: 'prompt'; text: string }
  | { type: 'interrupt' }

const EVENT_TYPES = new Set([
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

/** Parse one line from the runner into an event, or null if it is not a well-formed event. */
export function parseEventLine(line: string): AgentEvent | null {
  let value: unknown
  try {
    value = JSON.parse(line)
  } catch {
    return null
  }
  if (typeof value !== 'object' || value === null) return null
  const type = (value as { type?: unknown }).type
  return typeof type === 'string' && EVENT_TYPES.has(type) ? (value as AgentEvent) : null
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
