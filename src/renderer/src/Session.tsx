import { describeTool, relativePath, type TranscriptItem } from '@shared/session'
import { useEffect, useRef, useState } from 'react'
import type { AgentSession } from './useAgentSession'

interface SessionProps {
  project: string | null
  session: AgentSession
}

const TOOL_ICON = { running: '…', done: '✓', error: '✗', stopped: '–' } as const
const MAX_OUTPUT = 5000

function Composer({
  placeholder,
  hint,
  submitLabel,
  initial = '',
  onSubmit,
  disabled
}: {
  placeholder: string
  hint: string
  submitLabel: string
  initial?: string
  onSubmit: (text: string) => void
  disabled: boolean
}): React.JSX.Element {
  const [text, setText] = useState(initial)
  const submit = (): void => {
    if (disabled || text.trim() === '') return
    onSubmit(text.trim())
    setText('')
  }
  return (
    <>
      <textarea
        placeholder={placeholder}
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
            e.preventDefault()
            submit()
          }
        }}
        // biome-ignore lint/a11y/noAutofocus: this is the only input on the screen
        autoFocus
      />
      <div className="actions">
        <span className="hint">{hint}</span>
        <button type="button" disabled={disabled || text.trim() === ''} onClick={submit}>
          {submitLabel}
        </button>
      </div>
    </>
  )
}

function ApiKeyForm({
  onSave,
  onCancel
}: {
  onSave: (key: string) => Promise<string | null>
  onCancel?: () => void
}): React.JSX.Element {
  const [key, setKey] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  return (
    <form
      className="key-form"
      onSubmit={(e) => {
        e.preventDefault()
        setSaving(true)
        void onSave(key.trim()).then((message) => {
          setError(message)
          setSaving(false)
        })
      }}
    >
      <label htmlFor="api-key">Anthropic API key or Claude OAuth token</label>
      <div className="key-row">
        <input
          id="api-key"
          type="password"
          autoComplete="off"
          spellCheck={false}
          placeholder="sk-ant-api03-... or sk-ant-oat01-..."
          value={key}
          onChange={(e) => setKey(e.target.value)}
        />
        <button type="submit" disabled={saving || key.trim() === ''}>
          Save
        </button>
        {onCancel && (
          <button type="button" className="secondary" onClick={onCancel}>
            Cancel
          </button>
        )}
      </div>
      <p className="hint">
        Stored encrypted on this computer and sent to the sandbox when a session starts. Run{' '}
        <code>claude setup-token</code> to get an OAuth token.
      </p>
      {error && <p className="error-text">{error}</p>}
    </form>
  )
}

function Item({ item, project }: { item: TranscriptItem; project: string }): React.JSX.Element {
  switch (item.kind) {
    case 'user':
      return <div className="msg user">{item.text}</div>
    case 'text':
      return <div className="msg assistant">{item.text}</div>
    case 'edit':
      return <div className="edit-line">Edited {relativePath(project, item.path)}</div>
    case 'error':
      return <div className="msg error">{item.message}</div>
    case 'notice':
      return <div className="notice">{item.text}</div>
    case 'tool':
      return (
        <details className={`tool ${item.status}`}>
          <summary>
            <span className="tool-icon">{TOOL_ICON[item.status]}</span>
            <span className="tool-name">{item.name}</span>
            <span className="tool-input">{describeTool(item.input)}</span>
          </summary>
          <pre>{JSON.stringify(item.input, null, 2)}</pre>
          {item.output && (
            <pre>
              {item.output.length > MAX_OUTPUT
                ? `${item.output.slice(0, MAX_OUTPUT)}\n… (${item.output.length - MAX_OUTPUT} more characters)`
                : item.output}
            </pre>
          )}
        </details>
      )
  }
}

function Transcript({ session, project }: { session: AgentSession; project: string }) {
  const { state } = session
  const end = useRef<HTMLDivElement>(null)
  useEffect(() => {
    end.current?.scrollIntoView({ block: 'end' })
  }, [state.items])

  return (
    <main className="session active">
      <div className="transcript">
        {state.items.map((item, i) => (
          <Item key={i} item={item} project={project} />
        ))}
        {state.status === 'running' && <div className="working">Working…</div>}
        <div ref={end} />
      </div>
      {state.status === 'ended' && !state.sessionId ? (
        <div className="ended">
          <span>The session has ended.</span>
          <button type="button" onClick={() => void session.newSession()}>
            New session
          </button>
        </div>
      ) : (
        <div className="composer">
          {state.status === 'running' ? (
            <div className="actions">
              <span className="hint">
                {state.interrupting ? 'Stopping…' : 'The agent is working'}
              </span>
              <button type="button" disabled={state.interrupting} onClick={session.interrupt}>
                Stop
              </button>
            </div>
          ) : (
            <Composer
              placeholder={
                state.status === 'ended'
                  ? 'The agent exited. Send a message to resume this session'
                  : 'Send a follow-up'
              }
              hint="Ctrl+Enter to send"
              submitLabel="Send"
              onSubmit={session.send}
              disabled={false}
            />
          )}
        </div>
      )}
    </main>
  )
}

export function Session({ project, session }: SessionProps): React.JSX.Element {
  const { state, hasKey } = session
  const [editingKey, setEditingKey] = useState(false)

  if (state.status !== 'idle' && project) return <Transcript session={session} project={project} />

  const needKey = hasKey === false || editingKey
  return (
    <main className="session">
      <h1>{project ? 'What should the agent do?' : 'Open a project to get started'}</h1>
      {!project && <p className="hint">Use File &gt; Open Project (Ctrl+O).</p>}
      {project && needKey && (
        <ApiKeyForm
          onSave={async (key) => {
            const error = await session.saveKey(key)
            if (!error) setEditingKey(false)
            return error
          }}
          onCancel={editingKey ? () => setEditingKey(false) : undefined}
        />
      )}
      {state.startError && <p className="error-text">{state.startError}</p>}
      {project ? (
        <Composer
          placeholder="Describe what you want built or changed"
          hint="Ctrl+Enter to start"
          submitLabel="Start"
          initial={state.draft}
          onSubmit={session.start}
          disabled={hasKey !== true}
        />
      ) : (
        <textarea disabled placeholder="Describe what you want built or changed" />
      )}
      {project && hasKey && !editingKey && (
        <button type="button" className="link" onClick={() => setEditingKey(true)}>
          Change API key or token
        </button>
      )}
    </main>
  )
}
