import { credentialKind } from '@shared/credential'
import type { ApiKeyStore } from './apiKey'

export type FetchStatus = (
  url: string,
  init: { headers: Record<string, string>; signal: AbortSignal }
) => Promise<{ status: number }>

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

/**
 * Validate and save an API key or OAuth token. Checking first matters: the agent treats a bad key
 * as retryable and keeps retrying for minutes, which looks like a hang, so the user hears about
 * it up front. OAuth tokens are saved unchecked: the models endpoint is not known to accept them.
 */
export async function saveApiKey(
  store: ApiKeyStore,
  key: string,
  fetchStatus: FetchStatus
): Promise<void> {
  const trimmed = key.trim()
  if (!trimmed) throw new Error('The API key is empty')
  if (credentialKind(trimmed) === 'apiKey') {
    const problem = await checkApiKey(trimmed, fetchStatus)
    if (problem) throw new Error(problem)
  }
  await store.set(trimmed)
}
