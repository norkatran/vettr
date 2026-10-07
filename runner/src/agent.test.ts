import { describe, expect, it } from 'vitest'
import { encodeLine, LineBuffer, parseCommandLine, parseEventLine } from './agent'

describe('encodeLine', () => {
  it('writes one JSON object per line', () => {
    expect(encodeLine({ type: 'interrupt' })).toBe('{"type":"interrupt"}\n')
  })

  it('round-trips events through parseEventLine', () => {
    const event = { type: 'tool-finished', id: 't1', output: 'ok', isError: false } as const
    expect(parseEventLine(encodeLine(event).trim())).toEqual(event)
  })
})

describe('parseEventLine', () => {
  it('rejects invalid JSON', () => {
    expect(parseEventLine('{nope')).toBeNull()
  })

  it('rejects values that are not objects', () => {
    expect(parseEventLine('null')).toBeNull()
    expect(parseEventLine('42')).toBeNull()
  })

  it('rejects unknown or missing types', () => {
    expect(parseEventLine('{"type":"bogus"}')).toBeNull()
    expect(parseEventLine('{"type":7}')).toBeNull()
    expect(parseEventLine('{}')).toBeNull()
  })

  it('accepts every event type', () => {
    for (const type of [
      'session-started',
      'text',
      'tool-started',
      'tool-finished',
      'file-edited',
      'turn-finished',
      'error',
      'exited'
    ]) {
      expect(parseEventLine(JSON.stringify({ type }))).toEqual({ type })
    }
  })
})

describe('parseCommandLine', () => {
  it('accepts well-formed commands', () => {
    for (const command of [
      { type: 'init', credential: 'k', cwd: '/p' },
      { type: 'init', credential: 'k', cwd: '/p', resume: 's1' },
      { type: 'prompt', text: 'hi' },
      { type: 'interrupt' }
    ] as const) {
      expect(parseCommandLine(encodeLine(command))).toEqual(command)
    }
  })

  it('drops unknown fields', () => {
    expect(parseCommandLine('{"type":"interrupt","extra":1}')).toEqual({ type: 'interrupt' })
  })

  it('rejects malformed input and commands with missing fields', () => {
    for (const line of [
      '{nope',
      'null',
      '{"type":"bogus"}',
      '{"type":"prompt"}',
      '{"type":"init","credential":"k"}',
      '{"type":"init","cwd":"/p"}'
    ]) {
      expect(parseCommandLine(line)).toBeNull()
    }
  })
})

describe('LineBuffer', () => {
  it('returns complete lines and holds back a partial one', () => {
    const buffer = new LineBuffer()
    expect(buffer.push('a\nb\nc')).toEqual(['a', 'b'])
    expect(buffer.push('d\n')).toEqual(['cd'])
  })

  it('handles a line split across many chunks', () => {
    const buffer = new LineBuffer()
    expect(buffer.push('{"ty')).toEqual([])
    expect(buffer.push('pe":"text"}')).toEqual([])
    expect(buffer.push('\n')).toEqual(['{"type":"text"}'])
  })

  it('skips blank lines', () => {
    expect(new LineBuffer().push('\n  \na\n')).toEqual(['a'])
  })
})

describe('parseCommandLine resume', () => {
  it('ignores an empty or non-string resume', () => {
    for (const resume of ['""', '7']) {
      const line = `{"type":"init","credential":"k","cwd":"/p","resume":${resume}}`
      expect(parseCommandLine(line)).toEqual({ type: 'init', credential: 'k', cwd: '/p' })
    }
  })
})
