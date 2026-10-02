import { useEffect, useState } from 'react'

export function App(): React.JSX.Element {
  const [project, setProject] = useState<string | null>(null)
  const [prompt, setPrompt] = useState('')

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
      <main className="home">
        <textarea
          placeholder="What should the agent do?"
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          disabled={!project}
        />
        <button type="button" disabled={!project || !prompt.trim()}>
          Start
        </button>
      </main>
    </div>
  )
}
