import { useEffect, useState } from 'react'
import { Home } from './Home'

// Placeholder until the agent adapter and Session view exist (milestone 2).
function startSession(_prompt: string): void {}

export function App(): React.JSX.Element {
  const [project, setProject] = useState<string | null>(null)

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
      <Home project={project} onSubmit={startSession} />
    </div>
  )
}
