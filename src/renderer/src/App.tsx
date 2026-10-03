import { useEffect, useState } from 'react'
import { Session } from './Session'
import { Sidebar, type View } from './Sidebar'

// Placeholder until the agent adapter and Session view exist (milestone 2).
function startSession(_prompt: string): void {}

export function App(): React.JSX.Element {
  const [project, setProject] = useState<string | null>(null)
  const [view, setView] = useState<View>('session')
  const [expanded, setExpanded] = useState(true)

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
    </div>
  )
}
