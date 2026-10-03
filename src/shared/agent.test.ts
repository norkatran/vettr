import { describe, expect, it } from 'vitest'
import { encodeLine, LineBuffer, parseEventLine } from './agent'

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
