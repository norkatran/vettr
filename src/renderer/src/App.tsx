import { otherTheme, parseThemeChoice, resolveTheme, type Theme } from '@shared/theme'
import { useEffect, useState } from 'react'
import { Session } from './Session'
import { Sidebar, type View } from './Sidebar'
import { StatusBar } from './StatusBar'

// Placeholder until the agent adapter and Session view exist (milestone 2).
function startSession(_prompt: string): void {}

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

  // Clicking the active view collapses the side panel, as in VS Code.
  const select = (next: View): void => {
    if (next === view) setExpanded((e) => !e)
    else {
      setView(next)
      setExpanded(true)
    }
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
      </header>
      <div className="body">
        <Sidebar view={view} expanded={expanded} onSelect={select} />
        {view === 'session' ? (
          <Session project={project} onSubmit={startSession} />
        ) : (
          <main className="placeholder">
            <p className="hint">The changes view is not built yet.</p>
          </main>
        )}
      </div>
      <StatusBar project={project} theme={theme} onToggleTheme={toggleTheme} />
    </div>
  )
}
