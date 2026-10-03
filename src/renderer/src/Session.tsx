import { useState } from 'react'

interface SessionProps {
  project: string | null
  onSubmit: (prompt: string) => void
}

// Until the agent adapter exists there is never an active session, so this only renders the
// empty state: the prompt that starts one.
export function Session({ project, onSubmit }: SessionProps): React.JSX.Element {
  const [prompt, setPrompt] = useState('')
  const canSubmit = project !== null && prompt.trim() !== ''

  const submit = (): void => {
    if (!canSubmit) return
    onSubmit(prompt.trim())
    setPrompt('')
  }

  return (
    <main className="session">
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
        // biome-ignore lint/a11y/noAutofocus: the prompt is the only input on the empty session screen
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
