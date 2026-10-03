import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { getSessionMessages } from '@anthropic-ai/claude-agent-sdk'
import { describe, expect, it } from 'vitest'
import { replaySession } from './replay'
import { loadSession } from './sessions'

const msg = (type: string, message: unknown) => ({ type, session_id: 's1', message })
const user = (content: unknown) => msg('user', { role: 'user', content })
const assistant = (content: unknown[]) => msg('assistant', { role: 'assistant', content })

describe('replaySession', () => {
  it('rebuilds prompts, text, tools, edits and interrupts', () => {
    const state = replaySession([
      user('fix it'),
      assistant([
        { type: 'text', text: 'On it' },
        { type: 'tool_use', id: 't1', name: 'Edit', input: { file_path: '/p/a.ts' } },
        { type: 'tool_use', id: 't2', name: 'Bash', input: { command: 'ls' } }
      ]),
      user([{ type: 'tool_result', tool_use_id: 't1', content: 'ok' }]),
      user([{ type: 'text', text: '[Request interrupted by user]' }]),
      user('<command-name>/clear</command-name>'),
      user([{ type: 'image' }]),
      user({ nothing: true }),
      msg('system', null),
      msg('user', null),
      user([{ type: 'text', text: 'again' }])
    ])
    expect(state.status).toBe('waiting')
    expect(state.sessionId).toBe('s1')
    expect(state.items.map((i) => i.kind)).toEqual([
      'user',
      'text',
      'tool',
      'tool',
      'edit',
      'notice',
      'user'
    ])
    expect(state.items[3]).toMatchObject({ id: 't2', status: 'stopped' })
    expect(state.items[2]).toMatchObject({ id: 't1', status: 'done', output: 'ok' })
  })

  it('returns an empty waiting session for no messages', () => {
    expect(replaySession([]).items).toEqual([])
  })
})

describe('loadSession', () => {
  const env = {}
  it('returns null when the transcript is missing or the SDK throws', async () => {
    expect(
      await loadSession({ env, getSessionMessages: async () => [] }, '/c', '/p', 'x')
    ).toBeNull()
    expect(
      await loadSession(
        {
          env,
          getSessionMessages: async () => {
            throw new Error('x')
          }
        },
        '/c',
        '/p',
        'x'
      )
    ).toBeNull()
  })

  it('reads a real SDK transcript file', async () => {
    const config = mkdtempSync(join(tmpdir(), 'agentide-cfg-'))
    const project = '/work/demo'
    const dir = join(config, 'projects', '-work-demo')
    mkdirSync(dir, { recursive: true })
    const id = '11111111-1111-4111-8111-111111111111'
    const base = { sessionId: id, cwd: project, version: '2.0.0', isSidechain: false }
    const lines = [
      {
        ...base,
        type: 'user',
        uuid: 'u1',
        parentUuid: null,
        timestamp: '2026-01-01T00:00:00Z',
        message: { role: 'user', content: 'say hi' }
      },
      {
        ...base,
        type: 'assistant',
        uuid: 'a1',
        parentUuid: 'u1',
        timestamp: '2026-01-01T00:00:01Z',
        message: { role: 'assistant', content: [{ type: 'text', text: 'hi' }] }
      }
    ]
    writeFileSync(join(dir, `${id}.jsonl`), `${lines.map((l) => JSON.stringify(l)).join('\n')}\n`)
    const state = await loadSession(
      {
        env: process.env,
        getSessionMessages: async (sid, options) => {
          return (await getSessionMessages(sid, options)) as never
        }
      },
      config,
      project,
      id
    )
    expect(state?.sessionId).toBe(id)
    expect(state?.items).toEqual([
      { kind: 'user', text: 'say hi' },
      { kind: 'text', text: 'hi' }
    ])
  })
})
