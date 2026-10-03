import { describe, expect, it, vi } from 'vitest'
import type { ApiKeyStore } from './apiKey'
import { checkApiKey, type FetchStatus, saveApiKey } from './apiKeyCheck'

const respond = (status: number): FetchStatus => vi.fn().mockResolvedValue({ status })
const store = (): ApiKeyStore => ({ get: vi.fn(), set: vi.fn(), clear: vi.fn() })

describe('checkApiKey', () => {
  it('sends the key to the models endpoint', async () => {
    const fetchStatus = respond(200)
    expect(await checkApiKey('sk-1', fetchStatus)).toBeNull()
    const [url, init] = vi.mocked(fetchStatus).mock.calls[0] as Parameters<FetchStatus>
    expect(url).toBe('https://api.anthropic.com/v1/models')
    expect(init.headers).toMatchObject({ 'x-api-key': 'sk-1' })
    expect(init.signal).toBeInstanceOf(AbortSignal)
  })

  it('reports a rejected key', async () => {
    expect(await checkApiKey('k', respond(401))).toContain('rejected')
    expect(await checkApiKey('k', respond(403))).toContain('rejected')
  })

  it('does not blame the key for other statuses or network failures', async () => {
    expect(await checkApiKey('k', respond(500))).toBeNull()
    expect(await checkApiKey('k', respond(429))).toBeNull()
    expect(await checkApiKey('k', vi.fn().mockRejectedValue(new Error('offline')))).toBeNull()
  })
})

describe('saveApiKey', () => {
  it('saves a trimmed key that passes the check', async () => {
    const s = store()
    await saveApiKey(s, '  sk-1\n', respond(200))
    expect(s.set).toHaveBeenCalledWith('sk-1')
  })

  it('saves when the check cannot reach the API', async () => {
    const s = store()
    await saveApiKey(s, 'sk-1', vi.fn().mockRejectedValue(new Error('offline')))
    expect(s.set).toHaveBeenCalledWith('sk-1')
  })

  it('does not save a rejected key', async () => {
    const s = store()
    await expect(saveApiKey(s, 'bad', respond(401))).rejects.toThrow('rejected')
    expect(s.set).not.toHaveBeenCalled()
  })

  it('does not save an empty key or call the API for it', async () => {
    const s = store()
    const fetchStatus = respond(200)
    await expect(saveApiKey(s, '   ', fetchStatus)).rejects.toThrow('empty')
    expect(fetchStatus).not.toHaveBeenCalled()
    expect(s.set).not.toHaveBeenCalled()
  })
})
