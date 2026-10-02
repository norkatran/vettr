import { useState } from 'react'

interface HomeProps {
  project: string | null
  onSubmit: (prompt: string) => void
}

export function Home({ project, onSubmit }: HomeProps): React.JSX.Element {
  const [prompt, setPrompt] = useState('')
  const canSubmit = project !== null && prompt.trim() !== ''

  const submit = (): void => {
    if (!canSubmit) return
    onSubmit(prompt.trim())
    setPrompt('')
  }

  return (
    <main className="home">
      <h1>{project ? 'What should the agent do?' : 'Open a project to get started'}</h1>
      {!project && <p className="hint">Use File &gt; Open Project (Ctrl+O).</p>}
      <textarea
        placeholder="Describe what you want built or changed"
        value={prompt}
        onChange={(e) => setPrompt(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
            e.preventDefault()
            submit()
          }
        }}
        disabled={!project}
        // biome-ignore lint/a11y/noAutofocus: the prompt is the only input on the home screen
        autoFocus
      />
      <div className="actions">
        <span className="hint">Ctrl+Enter to start</span>
        <button type="button" disabled={!canSubmit} onClick={submit}>
          Start
        </button>
      </div>
    </main>
  )
}
