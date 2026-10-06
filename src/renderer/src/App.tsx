import { formatReview, pendingComments } from '@shared/comments'
import { changedFileCount } from '@shared/diff'
import { readinessBlockReason } from '@shared/readiness'
import { repliesByComment, sentComments } from '@shared/replies'
import { sendReviewRound } from '@shared/reviewSend'
import { otherTheme, parseThemeChoice, resolveTheme, type Theme } from '@shared/theme'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Changes } from './Changes'
import { usePalette } from './CommandPalette'
import { runCommandPalette } from './commands'
import { useNotify } from './Notifications'
import { ResolutionContext } from './Replies'
import { Session } from './Session'
import { SettingsView } from './SettingsView'
import { Sidebar, type View } from './Sidebar'
import { StatusBar } from './StatusBar'
import { useAgentSession } from './useAgentSession'
import { useApiKey } from './useApiKey'
import { useChanges } from './useChanges'
import { useReadiness } from './useReadiness'
import { useResolvedComments } from './useResolvedComments'
import { useReviewComments } from './useReviewComments'
import { useSessionList } from './useSessionList'

const THEME_KEY = 'vettr.theme'
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
  const notify = useNotify()
  const palette = usePalette()
  const [statusRefresh, setStatusRefresh] = useState(0)
  const changes = useChanges(project)
  const count = changes.changes ? changedFileCount(changes.changes) : 0
  const readiness = useReadiness()
  const apiKey = useApiKey()
  const agentBlock = readinessBlockReason(readiness)
  const session = useAgentSession(project)
  const sessions = useSessionList(project, `${session.state.status}:${session.state.sessionId}`)
  const review = useReviewComments(project, changes.changes)
  const resolved = useResolvedComments(project)
  const pending = pendingComments(review.comments)
  const replies = useMemo(() => repliesByComment(session.state.items), [session.state.items])
  // Opening a stored session brings back the comments it sent, from its transcript
  const { restore } = review
  const { loads } = session
  const items = session.state.items
  const lastLoads = useRef(0)
  useEffect(() => {
    if (loads === lastLoads.current) return
    lastLoads.current = loads
    restore(sentComments(items))
  }, [loads, items, restore])
  const status = session.state.status
  const needsNewSession = status === 'idle' || status === 'ended'
  const sendBlocked =
    agentBlock ?? (status === 'running' ? 'Wait for the agent to finish its current turn' : null)
  const sendReview = async (): Promise<void> => {
    const message = formatReview(pending, review.round)
    const ids = pending.map((c) => c.id)
    const error = await sendReviewRound({
      // Record the tree before the agent touches it, so the next round can show what it changed
      snapshot: () => (project ? window.vettr.snapshotTree(project) : Promise.resolve(null)),
      // With no live session, the review becomes the prompt that starts a new one
      deliver: async () => {
        if (!needsNewSession) return session.send(message)
        if (status === 'ended' && !session.state.sessionId) await session.newSession()
        return status === 'ended' ? session.send(message) : session.start(message, false)
      },
      markSent: (baseline) => review.markSent(ids, baseline)
    })
    if (error) notify('Comments not sent', error)
    else setView('session')
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

  const openSession = (id: string): void => {
    void session.openSession(id)
    setView('session')
  }

  const stagedCount = changes.changes?.staged.length ?? 0
  const openPalette = (): void => {
    const focusCommit = (): void => {
      setView('changes')
      setExpanded(true)
      setTimeout(() => document.getElementById('commit-message')?.focus(), 50)
    }
    void runCommandPalette({
      ...palette.api,
      project,
      notify,
      stagedCount: () => stagedCount,
      focusCommit,
      newSession: () => {
        newSession()
        setExpanded(true)
      },
      openSession: (id) => {
        openSession(id)
        setExpanded(true)
      },
      showView: (next) => {
        setView(next)
        setExpanded(true)
      }
    }).finally(() => setStatusRefresh((n) => n + 1))
  }
  const openPaletteRef = useRef(openPalette)
  openPaletteRef.current = openPalette

  useEffect(() => {
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === 'b' && (e.ctrlKey || e.metaKey)) {
        e.preventDefault()
        setExpanded((v) => !v)
      } else if (e.key.toLowerCase() === 'p' && e.shiftKey && (e.ctrlKey || e.metaKey)) {
        e.preventDefault()
        openPaletteRef.current()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  useEffect(() => {
    // Subscribe first so a pick made while the saved project loads is not overwritten.
    let picked = false
    const unsubscribe = window.vettr.onProjectOpened((path) => {
      picked = true
      setProject(path)
    })
    void window.vettr.getCurrentProject().then((path) => {
      if (!picked) setProject(path)
    })
    return unsubscribe
  }, [])

  return (
    <div className="layout">
      <header className="titlebar">
        <span>vettr</span>
        <span className="project">{project ?? 'No project open'}</span>
        <button
          type="button"
          className="titlebar-button"
          onClick={openPalette}
          title="Command palette (Ctrl+Shift+P)"
        >
          Commands
        </button>
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
          onOpenSession={openSession}
          changes={changes}
          sessionStarted={session.state.status !== 'idle'}
          sessions={sessions}
        />
        <ResolutionContext.Provider value={resolved}>
          {view === 'settings' ? (
            <SettingsView apiKey={apiKey} />
          ) : view === 'session' ? (
            <Session project={project} session={session} readiness={readiness} apiKey={apiKey} />
          ) : (
            <Changes
              project={project}
              changes={changes}
              review={review}
              replies={replies}
              agentBlock={agentBlock}
              send={{ pending: pending.length, run: () => void sendReview(), blocked: sendBlocked }}
            />
          )}
        </ResolutionContext.Provider>
      </div>
      <StatusBar
        project={project}
        theme={theme}
        onToggleTheme={toggleTheme}
        refreshKey={statusRefresh}
      />
      {palette.element}
    </div>
  )
}
