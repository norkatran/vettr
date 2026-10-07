import { credentialKind } from '@shared/credential'
import type { ApiKeyStore } from './apiKey'

export type FetchStatus = (
  url: string,
  init: { headers: Record<string, string>; signal: AbortSignal }
) => Promise<{ status: number; text?: () => Promise<string> }>

const MODELS_URL = 'https://api.anthropic.com/v1/models'
const TIMEOUT_MS = 10_000

/**
 * A message if Anthropic rejects the key, otherwise null. An unreachable API or an unexpected
 * status is not treated as a rejection, so a key can still be saved while offline.
 */
export async function checkApiKey(key: string, fetchStatus: FetchStatus): Promise<string | null> {
  try {
    const { status } = await fetchStatus(MODELS_URL, {
      headers: { 'x-api-key': key, 'anthropic-version': '2023-06-01' },
      signal: AbortSignal.timeout(TIMEOUT_MS)
    })
    return status === 401 || status === 403 ? 'Anthropic rejected that API key.' : null
  } catch {
    return null
  }
}

/** Wording of a 401 that says the token itself is bad (as opposed to, say, a missing scope). */
const BAD_TOKEN = /invalid|expired|revoked/i

/**
 * A message if Anthropic says the OAuth token is invalid or expired, otherwise null. Unlike an
 * API key, a valid long-lived token (from `claude setup-token`) is scoped for inference, so the
 * models endpoint might answer it with something other than 200 (this is unconfirmed against a
 * real token). So only a 401 whose error says the token is bad counts as a rejection; every other
 * outcome, including network failures, lets it be saved.
 */
export async function checkOAuthToken(
  token: string,
  fetchStatus: FetchStatus
): Promise<string | null> {
  try {
    const response = await fetchStatus(MODELS_URL, {
      headers: {
        authorization: `Bearer ${token}`,
        'anthropic-beta': 'oauth-2025-04-20',
        'anthropic-version': '2023-06-01'
      },
      signal: AbortSignal.timeout(TIMEOUT_MS)
    })
    if (response.status !== 401) return null
    const body = (await response.text?.()) ?? ''
    const message = (JSON.parse(body) as { error?: { message?: unknown } }).error?.message
    return typeof message === 'string' && BAD_TOKEN.test(message)
      ? 'Anthropic rejected that OAuth token. It may be invalid or expired.'
      : null
  } catch {
    return null
  }
}

/**
 * Check an API key or OAuth token with Anthropic and resolve to the trimmed value. Checking first
 * matters: the agent treats a bad key as retryable and keeps retrying for minutes, which looks like
 * a hang, so the user hears about it up front. OAuth tokens get the narrower check above.
 */
export async function validateApiKey(key: string, fetchStatus: FetchStatus): Promise<string> {
  const trimmed = key.trim()
  if (!trimmed) throw new Error('The API key is empty')
  const check = credentialKind(trimmed) === 'apiKey' ? checkApiKey : checkOAuthToken
  const problem = await check(trimmed, fetchStatus)
  if (problem) throw new Error(problem)
  return trimmed
}

/** Validate and save an API key or OAuth token. */
export async function saveApiKey(
  store: ApiKeyStore,
  key: string,
  fetchStatus: FetchStatus,
  /** Runs after validation and before storing; throw to abort (for example a declined prompt). */
  beforeSave?: () => Promise<void>
): Promise<void> {
  const trimmed = await validateApiKey(key, fetchStatus)
  await beforeSave?.()
  await store.set(trimmed)
}
