/** A slash command the agent offers (a built-in, skill, project or plugin command). */
export interface SlashCommandInfo {
  /** Without the leading slash. */
  name: string
  description: string
  /** Hint for the arguments, for example `<file>`; empty when it takes none. */
  argumentHint: string
  aliases?: string[]
}

/** Events an agent session emits; the UI depends only on these, never on a specific agent. */
export type AgentEvent =
  /** The SDK session ID, reported once per session; it is what `resume` takes later. */
  | { type: 'session-started'; sessionId: string }
  /** The full list of slash commands now available; replaces any earlier list. */
  | { type: 'commands'; commands: SlashCommandInfo[] }
  | { type: 'text'; text: string }
  | { type: 'tool-started'; id: string; name: string; input: unknown }
  | { type: 'tool-finished'; id: string; output: string; isError: boolean }
  | { type: 'file-edited'; path: string }
  | { type: 'turn-finished' }
  | { type: 'error'; message: string }
  /** The agent process went away; `code` is null when it was killed by a signal. */
  | { type: 'exited'; code: number | null }

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
  'commands',
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
