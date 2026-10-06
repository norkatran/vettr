import { useCallback, useEffect, useState } from 'react'

export interface ApiKey {
  /** Whether a key or token is saved; null until that is known. The value never reaches the renderer. */
  hasKey: boolean | null
  /** Resolves to null once saved, or to an error message. */
  save(key: string): Promise<string | null>
  /** Remove the saved key; resolves to null once removed, or to an error message. */
  clear(): Promise<string | null>
}

/** The saved API key or OAuth token's status, shared by the Session and Settings views. */
export function useApiKey(): ApiKey {
  const [hasKey, setHasKey] = useState<boolean | null>(null)
  useEffect(() => {
    void window.vettr.hasApiKey().then(setHasKey)
  }, [])

  const save = useCallback(async (key: string) => {
    const error = await window.vettr.setApiKey(key)
    if (!error) setHasKey(true)
    return error
  }, [])

  const clear = useCallback(async () => {
    const error = await window.vettr.clearApiKey()
    if (!error) setHasKey(false)
    return error
  }, [])

  return { hasKey, save, clear }
}
