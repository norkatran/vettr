import { formatReview, pendingComments } from '@shared/comments'
import { changedFileCount } from '@shared/diff'
import { otherTheme, parseThemeChoice, resolveTheme, type Theme } from '@shared/theme'
import { useEffect, useState } from 'react'
import { Changes } from './Changes'
import { Session } from './Session'
import { Sidebar, type View } from './Sidebar'
import { StatusBar } from './StatusBar'
import { useAgentSession } from './useAgentSession'
import { useChanges } from './useChanges'
import { useReviewComments } from './useReviewComments'

const THEME_KEY = 'agentide.theme'
const darkQuery = '(prefers-color-scheme: dark)'

function storedThemeChoice(): Theme | null {
  try {
    return parseThemeChoice(localStorage.getItem(THEME_KEY))
  } catch {
    return null
  }
}

// An explicit choice (the status bar toggle) is remembered; otherwise follow the OS.
function useTheme(): [Theme, () => void] {
  const [choice, setChoice] = useState<Theme | null>(storedThemeChoice)
  const [systemDark, setSystemDark] = useState(() => matchMedia(darkQuery).matches)
  const theme = resolveTheme(choice, systemDark)

  useEffect(() => {
    const query = matchMedia(darkQuery)
    const onChange = (): void => setSystemDark(query.matches)
    query.addEventListener('change', onChange)
    return () => query.removeEventListener('change', onChange)
  }, [])

  useEffect(() => {
    document.documentElement.dataset['theme'] = theme
  }, [theme])

  const toggle = (): void => {
    const next = otherTheme(theme)
    setChoice(next)
    try {
      localStorage.setItem(THEME_KEY, next)
    } catch {
      // Storage unavailable: the choice still applies for this session.
    }
  }
  return [theme, toggle]
}

export function App(): React.JSX.Element {
  const [project, setProject] = useState<string | null>(null)
  const [view, setView] = useState<View>('session')
  const [expanded, setExpanded] = useState(true)
  const [theme, toggleTheme] = useTheme()
  const changes = useChanges(project)
  const count = changes.changes ? changedFileCount(changes.changes) : 0
  const session = useAgentSession(project)
  const review = useReviewComments(project)
  const pending = pendingComments(review.comments)
  const status = session.state.status
  const needsNewSession = status === 'idle' || status === 'ended'
  const sendBlocked =
    status === 'running'
      ? 'Wait for the agent to finish its current turn'
      : needsNewSession && session.hasKey !== true
        ? 'Add an API key or token in the Session view to start a session'
        : null
  const sendReview = async (): Promise<void> => {
    const message = formatReview(pending)
    review.markSent(pending.map((c) => c.id))
    setView('session')
    // With no live session, the review becomes the prompt that starts a new one
    if (needsNewSession) {
      if (status === 'ended') await session.newSession()
      session.start(message)
    } else session.send(message)
  }

  // Clicking the active view collapses the side panel, as in VS Code.
  const select = (next: View): void => {
    if (next === view) setExpanded((e) => !e)
    else {
      setView(next)
      setExpanded(true)
    }
  }

  const newSession = (): void => {
    void session.newSession()
    setView('session')
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'b' && (e.ctrlKey || e.metaKey)) {
        e.preventDefault()
        setExpanded((v) => !v)
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  useEffect(() => {
    // Subscribe first so a pick made while the saved project loads is not overwritten.
    let picked = false
    const unsubscribe = window.agentide.onProjectOpened((path) => {
      picked = true
      setProject(path)
    })
    void window.agentide.getCurrentProject().then((path) => {
      if (!picked) setProject(path)
    })
    return unsubscribe
  }, [])

  return (
    <div className="layout">
      <header className="titlebar">
        <span>agentide</span>
        <span className="project">{project ?? 'No project open'}</span>
        {project && (
          <button
            type="button"
            className="titlebar-button"
            onClick={() => {
              setView('changes')
              setExpanded(true)
            }}
          >
            Changes{count > 0 ? ` (${count})` : ''}
          </button>
        )}
      </header>
      <div className="body">
        <Sidebar
          project={project}
          view={view}
          expanded={expanded}
          onSelect={select}
          onNewSession={newSession}
          changes={changes}
          sessionStarted={session.state.status !== 'idle'}
        />
        {view === 'session' ? (
          <Session project={project} session={session} />
        ) : (
          <Changes
            project={project}
            changes={changes}
            review={review}
            send={{ pending: pending.length, run: () => void sendReview(), blocked: sendBlocked }}
          />
        )}
      </div>
      <StatusBar project={project} theme={theme} onToggleTheme={toggleTheme} />
    </div>
  )
}
