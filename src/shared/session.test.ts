import { describe, expect, it } from 'vitest'
import type { AgentEvent } from './agent'
import {
  describeTool,
  initialSession,
  relativePath,
  type SessionAction,
  type SessionState,
  sessionReducer
} from './session'

const run = (state: SessionState, ...actions: SessionAction[]): SessionState =>
  actions.reduce(sessionReducer, state)
const events = (state: SessionState, ...list: AgentEvent[]): SessionState =>
  run(state, ...list.map((event) => ({ type: 'event', event }) as const))
const running = run(initialSession, { type: 'sent', text: 'go' })
const started: AgentEvent = {
  type: 'tool-started',
  id: 't1',
  name: 'Bash',
  input: { command: 'ls' }
}

describe('sessionReducer', () => {
  it('starts idle and empty', () => {
    expect(initialSession).toMatchObject({ status: 'idle', items: [], startError: null })
  })

  it('adds the user message and runs when something is sent', () => {
    const state = run(initialSession, { type: 'sent', text: 'go' })
    expect(state.status).toBe('running')
    expect(state.items).toEqual([{ kind: 'user', text: 'go' }])
  })

  it('appends assistant text', () => {
    const state = events(running, { type: 'text', text: 'hello' })
    expect(state.items.at(-1)).toEqual({ kind: 'text', text: 'hello' })
  })

  it('tracks a tool call from started to finished', () => {
    const open = events(running, started)
    expect(open.items.at(-1)).toMatchObject({ kind: 'tool', status: 'running', output: '' })
    const done = events(open, { type: 'tool-finished', id: 't1', output: 'a b', isError: false })
    expect(done.items.at(-1)).toMatchObject({ status: 'done', output: 'a b' })
  })

  it('marks a failed tool call as an error and leaves other items alone', () => {
    const state = events(
      running,
      started,
      { type: 'text', text: 'between' },
      { type: 'tool-finished', id: 't1', output: 'nope', isError: true }
    )
    expect(state.items.map((item) => item.kind)).toEqual(['user', 'tool', 'text'])
    expect(state.items[1]).toMatchObject({ status: 'error', output: 'nope' })
  })

  it('ignores a result for an unknown tool', () => {
    const state = events(running, { type: 'tool-finished', id: 'zzz', output: '', isError: false })
    expect(state.items).toEqual(running.items)
  })

  it('records file edits', () => {
    const state = events(running, { type: 'file-edited', path: '/p/a.ts' })
    expect(state.items.at(-1)).toEqual({ kind: 'edit', path: '/p/a.ts' })
  })

  it('waits for a follow-up when the turn finishes, and stops tools still running', () => {
    const state = events(running, started, { type: 'turn-finished' })
    expect(state.status).toBe('waiting')
    expect(state.items.at(-1)).toMatchObject({ kind: 'tool', status: 'stopped' })
  })

  it('shows errors', () => {
    const state = events(running, { type: 'error', message: 'bad key' })
    expect(state.items.at(-1)).toEqual({ kind: 'error', message: 'bad key' })
  })

  it('treats the error from an interrupted turn as expected and says so instead', () => {
    const state = run(
      running,
      { type: 'interrupt-requested' },
      { type: 'event', event: { type: 'error', message: 'error_during_execution' } },
      { type: 'event', event: { type: 'turn-finished' } }
    )
    expect(state.interrupting).toBe(false)
    expect(state.status).toBe('waiting')
    expect(state.items).toEqual([
      { kind: 'user', text: 'go' },
      { kind: 'notice', text: 'Interrupted' }
    ])
  })

  it('ends the session when the agent exits, and clears a pending interrupt', () => {
    const state = run(
      running,
      { type: 'interrupt-requested' },
      { type: 'event', event: started },
      { type: 'event', event: { type: 'exited', code: 1 } }
    )
    expect(state).toMatchObject({ status: 'ended', interrupting: false })
    expect(state.items.at(-1)).toMatchObject({ status: 'stopped' })
  })

  it('returns to the prompt with the draft and error when a start fails', () => {
    const state = run(running, { type: 'start-failed', message: 'No Docker', prompt: 'go' })
    expect(state).toEqual({ ...initialSession, draft: 'go', startError: 'No Docker' })
  })

  it('clears the start error and draft on the next send', () => {
    const failed = run(running, { type: 'start-failed', message: 'x', prompt: 'go' })
    const state = run(failed, { type: 'sent', text: 'go' })
    expect(state).toMatchObject({ draft: '', startError: null })
  })

  it('shows a failed follow-up as an error and lets the user try again', () => {
    const state = run(running, { type: 'send-failed', message: 'No session is running' })
    expect(state.status).toBe('waiting')
    expect(state.items.at(-1)).toEqual({ kind: 'error', message: 'No session is running' })
  })

  it('resets to the initial state', () => {
    expect(run(running, { type: 'reset' })).toBe(initialSession)
  })
})

describe('describeTool', () => {
  it('summarises by the most telling input field', () => {
    expect(describeTool({ command: 'npm test', description: 'run' })).toBe('npm test')
    expect(describeTool({ file_path: '/p/a.ts', old_string: 'x' })).toBe('/p/a.ts')
    expect(describeTool({ pattern: 'TODO' })).toBe('TODO')
  })

  it('uses only the first line and truncates long ones', () => {
    expect(describeTool({ command: 'a\nb' })).toBe('a')
    expect(describeTool({ command: 'x'.repeat(150) })).toBe(`${'x'.repeat(100)}…`)
  })

  it('falls back to the JSON of an input with no known field', () => {
    expect(describeTool({ foo: 1, command: 5 })).toBe('{"foo":1,"command":5}')
  })

  it('handles inputs that are not objects', () => {
    expect(describeTool(null)).toBe('null')
    expect(describeTool('raw')).toBe('raw')
    expect(describeTool(undefined)).toBe('undefined')
  })
})

describe('relativePath', () => {
  it('strips the project prefix', () => {
    expect(relativePath('/p', '/p/src/a.ts')).toBe('src/a.ts')
  })

  it('names the project itself', () => {
    expect(relativePath('/p', '/p')).toBe('.')
  })

  it('leaves paths outside the project, including look-alike prefixes', () => {
    expect(relativePath('/p', '/other/a.ts')).toBe('/other/a.ts')
    expect(relativePath('/p', '/p2/a.ts')).toBe('/p2/a.ts')
  })
})
