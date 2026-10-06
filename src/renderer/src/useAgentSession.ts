import { initialSession, type SessionState, sessionReducer } from '@shared/session'
import { useCallback, useEffect, useReducer, useRef } from 'react'

export interface AgentSession {
  state: SessionState
  /** Resolve to null once the agent has taken the prompt, or to an error message. */
  /**
   * Start a session with a prompt. If it fails the prompt is put back in the input, unless
   * `restoreDraft` is false (a review, whose comments stay pending and are sent again instead).
   */
  start(prompt: string, restoreDraft?: boolean): Promise<string | null>
  send(message: string): Promise<string | null>
  interrupt(): void
  /** Stop the running session, if any, and return to the prompt. */
  newSession(): Promise<void>
  /** Replace the current session with a stored one and rewrite the transcript. */
  openSession(id: string): Promise<void>
}

/**
 * The agent session for the open project. It lives above the views so switching to Changes and
 * back keeps the transcript, and it resets when another project is opened (the main process
 * stops the old session then).
 */
export function useAgentSession(project: string | null): AgentSession {
  const [state, dispatch] = useReducer(sessionReducer, initialSession)
  const stateRef = useRef(state)
  stateRef.current = state
  // Set while the shown session has no live agent: a stored session or one whose agent exited.
  const resumeId = useRef<string | null>(null)

  useEffect(() => window.vettr.onAgentEvent((event) => dispatch({ type: 'event', event })), [])
  useEffect(() => {
    if (state.status === 'ended' && state.sessionId) resumeId.current = state.sessionId
  }, [state.status, state.sessionId])
  useEffect(() => {
    resumeId.current = null
    dispatch({ type: 'reset' })
  }, [project])

  const start = useCallback(async (prompt: string, restoreDraft = true) => {
    dispatch({ type: 'sent', text: prompt })
    const message = await window.vettr.agentStart(prompt)
    if (message) dispatch({ type: 'start-failed', message, prompt: restoreDraft ? prompt : '' })
    return message
  }, [])

  const send = useCallback(async (message: string) => {
    const id = resumeId.current
    if (id) {
      dispatch({ type: 'resumed', text: message })
      // The main process restarts the agent to resume, so no stop is needed first
      const error = await window.vettr.agentStart(message, id)
      if (error) dispatch({ type: 'send-failed', message: error })
      else if (resumeId.current === id) resumeId.current = null
      return error
    }
    dispatch({ type: 'sent', text: message })
    const error = await window.vettr.agentSend(message)
    if (error) dispatch({ type: 'send-failed', message: error })
    return error
  }, [])

  const interrupt = useCallback(() => {
    dispatch({ type: 'interrupt-requested' })
    void window.vettr.agentInterrupt()
  }, [])

  const newSession = useCallback(async () => {
    await window.vettr.agentStop()
    resumeId.current = null
    dispatch({ type: 'reset' })
  }, [])

  const openSession = useCallback(async (id: string) => {
    if (stateRef.current.status === 'running') return
    const loaded = await window.vettr.loadSession(id)
    if (!loaded) return
    resumeId.current = loaded.sessionId ?? id
    dispatch({ type: 'load', state: loaded })
  }, [])

  return { state, start, send, interrupt, newSession, openSession }
}
