import { useState } from 'react'

export function App(): React.JSX.Element {
  const [project, setProject] = useState<string | null>(null)
  const [prompt, setPrompt] = useState('')

  async function openProject(): Promise<void> {
    const path = await window.agentide.openProject()
    if (path) setProject(path)
  }

  return (
    <div className="layout">
      <header className="titlebar">
        <span>agentide</span>
        <span className="project">{project ?? 'No project open'}</span>
        <button type="button" onClick={() => void openProject()}>
          Open project
        </button>
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
