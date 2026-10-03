import { initialSession, type SessionState, sessionReducer } from '@shared/session'
import { useCallback, useEffect, useReducer, useState } from 'react'

export interface AgentSession {
  state: SessionState
  /** Whether an API key is saved; null until that is known. */
  hasKey: boolean | null
  start(prompt: string): void
  send(message: string): void
  interrupt(): void
  /** Stop the running session, if any, and return to the prompt. */
  newSession(): Promise<void>
  /** Resolves to null once saved, or to an error message. */
  saveKey(key: string): Promise<string | null>
}

/**
 * The agent session for the open project. It lives above the views so switching to Changes and
 * back keeps the transcript, and it resets when another project is opened (the main process
 * stops the old session then).
 */
export function useAgentSession(project: string | null): AgentSession {
  const [state, dispatch] = useReducer(sessionReducer, initialSession)
  const [hasKey, setHasKey] = useState<boolean | null>(null)

  useEffect(() => window.agentide.onAgentEvent((event) => dispatch({ type: 'event', event })), [])
  useEffect(() => {
    void window.agentide.hasApiKey().then(setHasKey)
  }, [])
  useEffect(() => {
    dispatch({ type: 'reset' })
  }, [project])

  const start = useCallback((prompt: string) => {
    dispatch({ type: 'sent', text: prompt })
    void window.agentide.agentStart(prompt).then((message) => {
      if (message) dispatch({ type: 'start-failed', message, prompt })
    })
  }, [])

  const send = useCallback((message: string) => {
    dispatch({ type: 'sent', text: message })
    void window.agentide.agentSend(message).then((error) => {
      if (error) dispatch({ type: 'send-failed', message: error })
    })
  }, [])

  const interrupt = useCallback(() => {
    dispatch({ type: 'interrupt-requested' })
    void window.agentide.agentInterrupt()
  }, [])

  const newSession = useCallback(async () => {
    await window.agentide.agentStop()
    dispatch({ type: 'reset' })
  }, [])

  const saveKey = useCallback(async (key: string) => {
    const error = await window.agentide.setApiKey(key)
    if (!error) setHasKey(true)
    return error
  }, [])

  return { state, hasKey, start, send, interrupt, newSession, saveKey }
}
