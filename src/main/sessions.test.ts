import { describe, expect, it } from 'vitest'
import { listProjectSessions, withConfigDir } from './sessions'

describe('withConfigDir', () => {
  it('sets the variable during the call and removes it afterwards', async () => {
    const env: Record<string, string | undefined> = {}
    const seen = await withConfigDir(env, '/cfg', async () => env.CLAUDE_CONFIG_DIR)
    expect(seen).toBe('/cfg')
    expect('CLAUDE_CONFIG_DIR' in env).toBe(false)
  })

  it('restores a previous value, also when the call fails', async () => {
    const env: Record<string, string | undefined> = { CLAUDE_CONFIG_DIR: '/old' }
    await expect(
      withConfigDir(env, '/cfg', async () => {
        throw new Error('boom')
      })
    ).rejects.toThrow('boom')
    expect(env.CLAUDE_CONFIG_DIR).toBe('/old')
  })

  it('runs overlapping calls one at a time', async () => {
    const env: Record<string, string | undefined> = {}
    const seen: (string | undefined)[] = []
    const slow = async () => {
      await new Promise((r) => setTimeout(r, 10))
      seen.push(env.CLAUDE_CONFIG_DIR)
    }
    await Promise.all([
      withConfigDir(env, '/a', slow),
      withConfigDir(env, '/b', async () => {
        seen.push(env.CLAUDE_CONFIG_DIR)
      })
    ])
    expect(seen).toEqual(['/a', '/b'])
  })
})

describe('listProjectSessions', () => {
  it('maps and sorts sessions newest first, using the config dir', async () => {
    const env: Record<string, string | undefined> = {}
    let dirSeen: string | undefined
    const result = await listProjectSessions(
      {
        env,
        listSessions: async ({ dir }) => {
          dirSeen = `${dir}|${env.CLAUDE_CONFIG_DIR}`
          return [
            { sessionId: 'a', summary: 'old', lastModified: 1 },
            { sessionId: 'b', summary: 'new', lastModified: 2 }
          ]
        }
      },
      '/cfg',
      '/proj'
    )
    expect(dirSeen).toBe('/proj|/cfg')
    expect(result).toEqual([
      { id: 'b', title: 'new', lastModified: 2 },
      { id: 'a', title: 'old', lastModified: 1 }
    ])
  })

  it('returns an empty list when the SDK fails', async () => {
    const result = await listProjectSessions(
      {
        env: {},
        listSessions: async () => {
          throw new Error('nope')
        }
      },
      '/cfg',
      '/proj'
    )
    expect(result).toEqual([])
  })
})
