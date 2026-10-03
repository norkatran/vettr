import { describe, expect, it } from 'vitest'
import { Translator } from './translate'

const assistant = (...content: unknown[]) => ({ type: 'assistant', message: { content } })
const user = (...content: unknown[]) => ({ type: 'user', message: { content } })

describe('Translator', () => {
  it('emits text blocks and skips empty or thinking blocks', () => {
    const events = new Translator().translate(
      assistant({ type: 'text', text: 'hi' }, { type: 'text', text: '' }, { type: 'thinking' })
    )
    expect(events).toEqual([{ type: 'text', text: 'hi' }])
  })

  it('emits tool-started for tool_use blocks', () => {
    const events = new Translator().translate(
      assistant({ type: 'tool_use', id: 't1', name: 'Bash', input: { command: 'ls' } })
    )
    expect(events).toEqual([
      { type: 'tool-started', id: 't1', name: 'Bash', input: { command: 'ls' } }
    ])
  })

  it('emits tool-finished with string output', () => {
    const events = new Translator().translate(
      user({ type: 'tool_result', tool_use_id: 't1', content: 'done' })
    )
    expect(events).toEqual([{ type: 'tool-finished', id: 't1', output: 'done', isError: false }])
  })

  it('joins text blocks of array output and ignores other blocks', () => {
    const events = new Translator().translate(
      user({
        type: 'tool_result',
        tool_use_id: 't1',
        is_error: true,
        content: [{ type: 'text', text: 'a' }, { type: 'image' }, { type: 'text', text: 'b' }]
      })
    )
    expect(events).toEqual([{ type: 'tool-finished', id: 't1', output: 'a\nb', isError: true }])
  })

  it('uses empty output for missing content', () => {
    const events = new Translator().translate(user({ type: 'tool_result', tool_use_id: 't1' }))
    expect(events).toEqual([{ type: 'tool-finished', id: 't1', output: '', isError: false }])
  })

  it('ignores non-tool_result blocks in user messages', () => {
    expect(new Translator().translate(user({ type: 'text', text: 'x' }))).toEqual([])
  })

  it('emits file-edited after a successful edit tool finishes', () => {
    const t = new Translator()
    t.translate(
      assistant(
        { type: 'tool_use', id: 'e1', name: 'Edit', input: { file_path: '/p/a.ts' } },
        { type: 'tool_use', id: 'n1', name: 'NotebookEdit', input: { notebook_path: '/p/n.ipynb' } }
      )
    )
    expect(t.translate(user({ type: 'tool_result', tool_use_id: 'e1', content: 'ok' }))).toEqual([
      { type: 'tool-finished', id: 'e1', output: 'ok', isError: false },
      { type: 'file-edited', path: '/p/a.ts' }
    ])
    expect(t.translate(user({ type: 'tool_result', tool_use_id: 'n1', content: 'ok' }))).toEqual([
      { type: 'tool-finished', id: 'n1', output: 'ok', isError: false },
      { type: 'file-edited', path: '/p/n.ipynb' }
    ])
  })

  it('emits file-edited only once per call', () => {
    const t = new Translator()
    t.translate(assistant({ type: 'tool_use', id: 'e1', name: 'Write', input: { file_path: 'a' } }))
    const result = user({ type: 'tool_result', tool_use_id: 'e1', content: '' })
    expect(t.translate(result)).toHaveLength(2)
    expect(t.translate(result)).toHaveLength(1)
  })

  it('does not emit file-edited for a failed edit', () => {
    const t = new Translator()
    t.translate(assistant({ type: 'tool_use', id: 'e1', name: 'Edit', input: { file_path: 'a' } }))
    const events = t.translate(
      user({ type: 'tool_result', tool_use_id: 'e1', content: 'no match', is_error: true })
    )
    expect(events.map((e) => e.type)).toEqual(['tool-finished'])
  })

  it('does not track an edit tool call without a usable path', () => {
    const t = new Translator()
    t.translate(
      assistant(
        { type: 'tool_use', id: 'a', name: 'Edit', input: { file_path: 7 } },
        { type: 'tool_use', id: 'b', name: 'Write' }
      )
    )
    for (const id of ['a', 'b']) {
      const events = t.translate(user({ type: 'tool_result', tool_use_id: id, content: '' }))
      expect(events.map((e) => e.type)).toEqual(['tool-finished'])
    }
  })

  it('does not treat other tools as edits', () => {
    const t = new Translator()
    t.translate(assistant({ type: 'tool_use', id: 'r', name: 'Read', input: { file_path: 'a' } }))
    const events = t.translate(user({ type: 'tool_result', tool_use_id: 'r', content: 'x' }))
    expect(events.map((e) => e.type)).toEqual(['tool-finished'])
  })

  it('emits turn-finished for a successful result', () => {
    const events = new Translator().translate({ type: 'result', subtype: 'success' })
    expect(events).toEqual([{ type: 'turn-finished' }])
  })

  it('emits an error before turn-finished for a failed result', () => {
    const t = new Translator()
    expect(t.translate({ type: 'result', subtype: 'error_max_turns', errors: ['a', 'b'] })).toEqual(
      [{ type: 'error', message: 'The agent stopped: a; b' }, { type: 'turn-finished' }]
    )
    expect(
      t.translate({ type: 'result', subtype: 'success', is_error: true, result: 'bad key' })
    ).toEqual([{ type: 'error', message: 'The agent stopped: bad key' }, { type: 'turn-finished' }])
    expect(t.translate({ type: 'result', subtype: 'error_during_execution' })).toEqual([
      { type: 'error', message: 'The agent stopped: error_during_execution' },
      { type: 'turn-finished' }
    ])
  })

  it('handles messages without content and ignores other message types', () => {
    const t = new Translator()
    expect(t.translate({ type: 'assistant' })).toEqual([])
    expect(t.translate({ type: 'user', message: { content: 'plain string' } })).toEqual([])
    expect(t.translate({ type: 'system', subtype: 'init' })).toEqual([])
  })
})

describe('Translator session id', () => {
  it('reports the session id once, before the message events', () => {
    const translator = new Translator()
    const message = { ...assistant({ type: 'text', text: 'hi' }), session_id: 's1' }
    expect(translator.translate(message)).toEqual([
      { type: 'session-started', sessionId: 's1' },
      { type: 'text', text: 'hi' }
    ])
    expect(translator.translate(message)).toEqual([{ type: 'text', text: 'hi' }])
  })

  it('reports a new id if it changes', () => {
    const translator = new Translator()
    translator.translate({ type: 'system', session_id: 's1' })
    expect(translator.translate({ type: 'system', session_id: 's2' })).toEqual([
      { type: 'session-started', sessionId: 's2' }
    ])
  })
})
