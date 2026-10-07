export type CredentialKind = 'apiKey' | 'oauthToken'

/**
 * What a pasted secret is. Long-lived Claude OAuth tokens (from `claude setup-token`) start with
 * `sk-ant-oat`; anything else is treated as an Anthropic API key.
 */
export function credentialKind(secret: string): CredentialKind {
  return secret.startsWith('sk-ant-oat') ? 'oauthToken' : 'apiKey'
}

/** The environment variable Claude Code reads for this kind of credential. */
export function credentialEnv(secret: string): Record<string, string> {
  return credentialKind(secret) === 'oauthToken'
    ? { CLAUDE_CODE_OAUTH_TOKEN: secret }
    : { ANTHROPIC_API_KEY: secret }
}
