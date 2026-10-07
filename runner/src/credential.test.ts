import { describe, expect, it } from 'vitest'
import { credentialEnv, credentialKind } from './credential'

describe('credentialKind', () => {
  it('recognises OAuth tokens by their prefix', () => {
    expect(credentialKind('sk-ant-oat01-abc')).toBe('oauthToken')
  })

  it('treats everything else as an API key', () => {
    expect(credentialKind('sk-ant-api03-abc')).toBe('apiKey')
    expect(credentialKind('whatever')).toBe('apiKey')
  })
})

describe('credentialEnv', () => {
  it('sets the OAuth variable for a token and only that one', () => {
    expect(credentialEnv('sk-ant-oat01-abc')).toEqual({
      CLAUDE_CODE_OAUTH_TOKEN: 'sk-ant-oat01-abc'
    })
  })

  it('sets the API key variable for a key and only that one', () => {
    expect(credentialEnv('sk-ant-api03-abc')).toEqual({ ANTHROPIC_API_KEY: 'sk-ant-api03-abc' })
  })
})
