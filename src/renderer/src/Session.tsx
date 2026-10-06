import { type Readiness, readinessBlockReason } from '@shared/readiness'
import { describeTool, relativePath, type TranscriptItem } from '@shared/session'
import { useEffect, useRef, useState } from 'react'
import { ApiKeyForm } from './ApiKeyForm'
import type { AgentSession } from './useAgentSession'
import type { ApiKey } from './useApiKey'

interface SessionProps {
  project: string | null
  session: AgentSession
  readiness: Readiness
  apiKey: ApiKey
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

function Transcript({
  session,
  project,
  readiness
}: {
  session: AgentSession
  project: string
  readiness: Readiness
}) {
  const { state } = session
  const block = readinessBlockReason(readiness)
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
              hint={block ?? 'Ctrl+Enter to send'}
              submitLabel="Send"
              onSubmit={session.send}
              disabled={block !== null}
            />
          )}
        </div>
      )}
    </main>
  )
}

export function Session({ project, session, readiness, apiKey }: SessionProps): React.JSX.Element {
  const { state } = session
  const block = readinessBlockReason(readiness)

  if (state.status !== 'idle' && project)
    return <Transcript session={session} project={project} readiness={readiness} />

  const noKey = readiness.reason === 'no-key' || apiKey.hasKey === false
  return (
    <main className="session">
      <h1>{project ? 'What should the agent do?' : 'Open a project to get started'}</h1>
      {!project && <p className="hint">Use File &gt; Open Project (Ctrl+O).</p>}
      {project && noKey && (
        <ApiKeyForm
          onSave={apiKey.save}
          explanation="The agent needs a key or token before you can write a prompt. You can change or remove it later in Settings."
        />
      )}
      {state.startError && <p className="error-text">{state.startError}</p>}
      {project && !noKey ? (
        <Composer
          placeholder="Describe what you want built or changed"
          hint={block ?? 'Ctrl+Enter to start'}
          submitLabel="Start"
          initial={state.draft}
          onSubmit={session.start}
          disabled={block !== null}
        />
      ) : project ? null : (
        <textarea disabled placeholder="Describe what you want built or changed" />
      )}
    </main>
  )
}
