import { describe, expect, it, vi } from 'vitest'
import type { ApiKeyStore } from './apiKey'
import { checkApiKey, checkOAuthToken, type FetchStatus, saveApiKey } from './apiKeyCheck'

const respond = (status: number): FetchStatus => vi.fn().mockResolvedValue({ status })
const respondWith = (status: number, body: string): FetchStatus =>
  vi.fn().mockResolvedValue({ status, text: async () => body })
const authError = (message: string): string =>
  JSON.stringify({ type: 'error', error: { type: 'authentication_error', message } })
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

describe('checkOAuthToken', () => {
  it('sends the token as a bearer token with the OAuth beta header', async () => {
    const fetchStatus = respond(200)
    expect(await checkOAuthToken('sk-ant-oat01-x', fetchStatus)).toBeNull()
    const [, init] = vi.mocked(fetchStatus).mock.calls[0] as Parameters<FetchStatus>
    expect(init.headers).toMatchObject({
      authorization: 'Bearer sk-ant-oat01-x',
      'anthropic-beta': 'oauth-2025-04-20'
    })
    expect(init.headers).not.toHaveProperty('x-api-key')
  })

  it('rejects a 401 that says the token is invalid or expired', async () => {
    const invalid = respondWith(401, authError('OAuth access token is invalid.'))
    expect(await checkOAuthToken('t', invalid)).toContain('rejected')
    const expired = respondWith(401, authError('OAuth token has expired.'))
    expect(await checkOAuthToken('t', expired)).toContain('expired')
  })

  it('does not reject a 401 for another reason, or an unreadable one', async () => {
    expect(await checkOAuthToken('t', respondWith(401, authError('Missing scope')))).toBeNull()
    expect(await checkOAuthToken('t', respondWith(401, '{}'))).toBeNull()
    expect(await checkOAuthToken('t', respondWith(401, '{"error":{"message":5}}'))).toBeNull()
    expect(await checkOAuthToken('t', respondWith(401, 'not json'))).toBeNull()
    expect(await checkOAuthToken('t', respond(401))).toBeNull()
  })

  it('lets every other outcome through', async () => {
    for (const status of [200, 403, 429, 500]) {
      expect(await checkOAuthToken('t', respondWith(status, authError('invalid')))).toBeNull()
    }
    expect(await checkOAuthToken('t', vi.fn().mockRejectedValue(new Error('offline')))).toBeNull()
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

  it('saves an OAuth token that is not rejected', async () => {
    const s = store()
    await saveApiKey(s, 'sk-ant-oat01-abc', respond(200))
    expect(s.set).toHaveBeenCalledWith('sk-ant-oat01-abc')
  })

  it('does not save an OAuth token Anthropic says is invalid', async () => {
    const s = store()
    const bad = respondWith(401, authError('OAuth access token is invalid.'))
    await expect(saveApiKey(s, 'sk-ant-oat01-abc', bad)).rejects.toThrow('OAuth token')
    expect(s.set).not.toHaveBeenCalled()
  })

  it('does not save a rejected key', async () => {
    const s = store()
    await expect(saveApiKey(s, 'bad', respond(401))).rejects.toThrow('rejected')
    expect(s.set).not.toHaveBeenCalled()
  })

  it('runs the pre-save hook after validation and does not save if it throws', async () => {
    const s = store()
    const order: string[] = []
    const hook = async (): Promise<void> => {
      order.push('hook')
      throw new Error('declined')
    }
    const fetchStatus: FetchStatus = async () => {
      order.push('check')
      return { status: 200 }
    }
    await expect(saveApiKey(s, 'sk-1', fetchStatus, hook)).rejects.toThrow('declined')
    expect(order).toEqual(['check', 'hook'])
    expect(s.set).not.toHaveBeenCalled()
  })

  it('does not run the pre-save hook for a rejected key', async () => {
    const hook = vi.fn()
    await expect(saveApiKey(store(), 'bad', respond(401), hook)).rejects.toThrow('rejected')
    expect(hook).not.toHaveBeenCalled()
  })

  it('saves after the pre-save hook completes', async () => {
    const s = store()
    const hook = vi.fn().mockResolvedValue(undefined)
    await saveApiKey(s, 'sk-1', respond(200), hook)
    expect(hook).toHaveBeenCalledOnce()
    expect(s.set).toHaveBeenCalledWith('sk-1')
  })

  it('does not save an empty key or call the API for it', async () => {
    const s = store()
    const fetchStatus = respond(200)
    await expect(saveApiKey(s, '   ', fetchStatus)).rejects.toThrow('empty')
    expect(fetchStatus).not.toHaveBeenCalled()
    expect(s.set).not.toHaveBeenCalled()
  })
})
