import type { AgentEvent } from './agent'

export type ToolStatus = 'running' | 'done' | 'error' | 'stopped'

export type TranscriptItem =
  | { kind: 'user'; text: string }
  | { kind: 'text'; text: string }
  | { kind: 'tool'; id: string; name: string; input: unknown; status: ToolStatus; output: string }
  | { kind: 'edit'; path: string }
  | { kind: 'error'; message: string }
  | { kind: 'notice'; text: string }

/**
 * - idle: no session yet, the prompt is shown
 * - running: a turn is in progress
 * - waiting: the turn finished and a follow-up can be sent
 * - ended: the agent process is gone
 */
export type SessionStatus = 'idle' | 'running' | 'waiting' | 'ended'

export interface SessionState {
  status: SessionStatus
  items: TranscriptItem[]
  /** An interrupt was requested and the turn has not finished yet. */
  interrupting: boolean
  /** The prompt text to put back in the input after a failed start. */
  draft: string
  startError: string | null
}

export const initialSession: SessionState = {
  status: 'idle',
  items: [],
  interrupting: false,
  draft: '',
  startError: null
}

export type SessionAction =
  | { type: 'sent'; text: string }
  | { type: 'start-failed'; message: string; prompt: string }
  | { type: 'send-failed'; message: string }
  | { type: 'interrupt-requested' }
  | { type: 'event'; event: AgentEvent }
  | { type: 'reset' }

/** Tools still marked running when a turn ends or the agent exits will never report back. */
function stopRunning(items: TranscriptItem[]): TranscriptItem[] {
  return items.map((item) =>
    item.kind === 'tool' && item.status === 'running' ? { ...item, status: 'stopped' } : item
  )
}

function applyEvent(state: SessionState, event: AgentEvent): SessionState {
  const { items } = state
  switch (event.type) {
    case 'text':
      return { ...state, items: [...items, { kind: 'text', text: event.text }] }
    case 'tool-started':
      return {
        ...state,
        items: [
          ...items,
          {
            kind: 'tool',
            id: event.id,
            name: event.name,
            input: event.input,
            status: 'running',
            output: ''
          }
        ]
      }
    case 'tool-finished':
      return {
        ...state,
        items: items.map((item) =>
          item.kind === 'tool' && item.id === event.id
            ? { ...item, status: event.isError ? 'error' : 'done', output: event.output }
            : item
        )
      }
    case 'file-edited':
      return { ...state, items: [...items, { kind: 'edit', path: event.path }] }
    case 'turn-finished': {
      const stopped = stopRunning(items)
      return {
        ...state,
        status: 'waiting',
        interrupting: false,
        items: state.interrupting ? [...stopped, { kind: 'notice', text: 'Interrupted' }] : stopped
      }
    }
    case 'error':
      // Stopping a turn makes the SDK report it as an error; the user asked for that
      return state.interrupting
        ? state
        : { ...state, items: [...items, { kind: 'error', message: event.message }] }
    case 'exited':
      return { ...state, status: 'ended', interrupting: false, items: stopRunning(items) }
  }
}

export function sessionReducer(state: SessionState, action: SessionAction): SessionState {
  switch (action.type) {
    case 'sent':
      return {
        ...state,
        status: 'running',
        draft: '',
        startError: null,
        items: [...state.items, { kind: 'user', text: action.text }]
      }
    case 'start-failed':
      return { ...initialSession, draft: action.prompt, startError: action.message }
    case 'send-failed':
      return {
        ...state,
        status: 'waiting',
        items: [...state.items, { kind: 'error', message: action.message }]
      }
    case 'interrupt-requested':
      return { ...state, interrupting: true }
    case 'event':
      return applyEvent(state, action.event)
    case 'reset':
      return initialSession
  }
}

const SUMMARY_FIELDS = [
  'command',
  'file_path',
  'notebook_path',
  'pattern',
  'path',
  'url',
  'query',
  'description'
]

function firstLine(text: string, max: number): string {
  const line = text.split('\n')[0] as string
  return line.length > max ? `${line.slice(0, max)}…` : line
}

/** A one-line summary of a tool call's input, for the collapsed transcript row. */
export function describeTool(input: unknown): string {
  if (typeof input === 'object' && input !== null) {
    const record = input as Record<string, unknown>
    for (const field of SUMMARY_FIELDS) {
      const value = record[field]
      if (typeof value === 'string') return firstLine(value, 100)
    }
    return firstLine(JSON.stringify(input), 100)
  }
  return firstLine(String(input), 100)
}

/** `path` relative to `project` when it is inside it (the container uses host paths). */
export function relativePath(project: string, path: string): string {
  if (path === project) return '.'
  return path.startsWith(`${project}/`) ? path.slice(project.length + 1) : path
}
