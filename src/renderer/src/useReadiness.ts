import { describeTransition, INITIAL_READINESS, type Readiness } from '@shared/readiness'
import { useEffect, useRef, useState } from 'react'
import { useNotify } from './Notifications'

/**
 * The agent's readiness, pushed from the main process. Every input that directs the agent
 * derives its enabled state from this and nothing else. Also announces transitions the user
 * should know about (Docker problems, crash restarts).
 */
export function useReadiness(): Readiness {
  const [readiness, setReadiness] = useState<Readiness>(INITIAL_READINESS)
  const notify = useNotify()
  const previous = useRef<Readiness>(INITIAL_READINESS)
  // A sandbox image build was announced and has not finished yet
  const building = useRef(false)

  useEffect(() => {
    let live = true
    const apply = (next: Readiness): void => {
      const result = describeTransition(previous.current, next, building.current)
      previous.current = next
      building.current = result.building
      setReadiness(next)
      if (result.notice) notify(result.notice.title, result.notice.detail, result.notice.level)
    }
    const unsubscribe = window.vettr.onReadiness(apply)
    void window.vettr.getReadiness().then((current) => {
      if (live && previous.current === INITIAL_READINESS) apply(current)
    })
    return () => {
      live = false
      unsubscribe()
    }
  }, [notify])

  return readiness
}
