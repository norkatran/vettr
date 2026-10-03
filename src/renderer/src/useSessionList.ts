import type { SessionInfo } from '@shared/sessions'
import { useEffect, useState } from 'react'

/**
 * The open project's stored sessions. Reloads when the project changes and whenever `refreshKey`
 * does (the live session's status or ID), so a new or finished session shows up.
 */
export function useSessionList(project: string | null, refreshKey: string): SessionInfo[] {
  const [sessions, setSessions] = useState<SessionInfo[]>([])
  useEffect(() => {
    if (!project) {
      setSessions([])
      return
    }
    let current = true
    void window.agentide.listSessions().then((list) => {
      if (current) setSessions(list)
    })
    return () => {
      current = false
    }
  }, [project, refreshKey])
  return sessions
}
