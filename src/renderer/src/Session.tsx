import type { SlashCommandInfo } from '@shared/agent'
import { parseReview, type SentComment } from '@shared/comments'
import { type Readiness, readinessBlockReason } from '@shared/readiness'
import { type AgentReply, repliesByComment, replyOf } from '@shared/replies'
import { describeTool, relativePath, type TranscriptItem } from '@shared/session'
import { completeCommand, filterCommands, slashQuery } from '@shared/slashCommands'
import { useEffect, useMemo, useRef, useState } from 'react'
import { ApiKeyForm } from './ApiKeyForm'
import { ReplyBubble, ResolvableThread } from './Replies'
import type { AgentSession } from './useAgentSession'
import type { ApiKey } from './useApiKey'
import { useSlashCommands } from './useSlashCommands'

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
  disabled,
  commands
}: {
  placeholder: string
  hint: string
  submitLabel: string
  initial?: string
  onSubmit: (text: string) => void
  disabled: boolean
  commands: SlashCommandInfo[]
}): React.JSX.Element {
  const [text, setText] = useState(initial)
  const [selected, setSelected] = useState(0)
  const [dismissed, setDismissed] = useState(false)
  const query = slashQuery(text)
  const matches = query === null || dismissed ? [] : filterCommands(commands, query)
  const menuOpen = matches.length > 0
  const active = Math.min(selected, matches.length - 1)
  const activeItem = useRef<HTMLDivElement>(null)
  // Keep the selection visible: after arrowing past either edge or after scrolling by hand
  useEffect(() => {
    activeItem.current?.scrollIntoView({ block: 'nearest' })
  }, [active, query])
  const choose = (command: SlashCommandInfo): void => {
    setText(completeCommand(command))
    setSelected(0)
  }
  const submit = (): void => {
    if (disabled || text.trim() === '') return
    onSubmit(text.trim())
    setText('')
  }
  return (
    <>
      {menuOpen && (
        <div className="slash-menu" role="listbox" aria-label="Slash commands">
          {matches.map((command, i) => (
            <div
              key={command.name}
              ref={i === active ? activeItem : undefined}
              role="option"
              aria-selected={i === active}
              tabIndex={-1}
              className={`slash-item${i === active ? ' active' : ''}`}
              // mousedown, so the textarea keeps focus
              onMouseDown={(e) => {
                e.preventDefault()
                choose(command)
              }}
            >
              <span className="slash-name">/{command.name}</span>
              {command.argumentHint && <span className="slash-arg">{command.argumentHint}</span>}
              <span className="slash-desc">{command.description}</span>
            </div>
          ))}
        </div>
      )}
      <textarea
        placeholder={placeholder}
        value={text}
        onChange={(e) => {
          setText(e.target.value)
          setSelected(0)
          setDismissed(false)
        }}
        onKeyDown={(e) => {
          if (menuOpen) {
            if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
              e.preventDefault()
              const step = e.key === 'ArrowDown' ? 1 : -1
              setSelected((active + step + matches.length) % matches.length)
              return
            }
            if (e.key === 'Tab' || (e.key === 'Enter' && !e.ctrlKey && !e.metaKey)) {
              e.preventDefault()
              choose(matches[active])
              return
            }
            if (e.key === 'Escape') {
              e.preventDefault()
              setDismissed(true)
              return
            }
          }
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

/** A review round the user sent, rebuilt from the message so it survives reloads and resumes. */
function ReviewMessage({
  round,
  comments,
  threads
}: {
  round: number
  comments: SentComment[]
  threads: Map<string, AgentReply[]>
}): React.JSX.Element {
  return (
    <div className="review-message">
      <div className="review-title">
        Review, round {round}: {comments.length} {comments.length === 1 ? 'comment' : 'comments'}
      </div>
      {comments.map((c) => (
        <ResolvableThread
          key={c.id}
          id={c.id}
          className={`comment${c.outdated ? ' outdated' : ''}`}
          summary={`${c.file}:${c.start === c.end ? c.start : `${c.start}-${c.end}`}`}
        >
          <div className="comment-meta">
            <span>
              {c.file}:{c.start === c.end ? c.start : `${c.start}-${c.end}`}
              {c.side === 'old' ? ' (removed code)' : ''}
            </span>
            {c.outdated && <span>outdated</span>}
          </div>
          {c.snapshot.length > 0 && <pre className="comment-snapshot">{c.snapshot.join('\n')}</pre>}
          <p>{c.text}</p>
          {(threads.get(c.id) ?? []).map((reply, i) => (
            <ReplyBubble key={i} reply={reply} />
          ))}
        </ResolvableThread>
      ))}
    </div>
  )
}

function Item({
  item,
  project,
  threads,
  where
}: {
  item: TranscriptItem
  project: string
  threads: Map<string, AgentReply[]>
  where: Map<string, string>
}): React.JSX.Element {
  switch (item.kind) {
    case 'user': {
      const review = parseReview(item.text)
      if (review && review.comments.length > 0)
        return <ReviewMessage {...review} threads={threads} />
      return <div className="msg user">{item.text}</div>
    }
    case 'text':
      return <div className="msg assistant">{item.text}</div>
    case 'edit':
      return <div className="edit-line">Edited {relativePath(project, item.path)}</div>
    case 'error':
      return <div className="msg error">{item.message}</div>
    case 'notice':
      return <div className="notice">{item.text}</div>
    case 'tool': {
      const reply = replyOf(item)
      if (reply) return <ReplyBubble reply={reply} where={where.get(reply.commentId)} />
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
}

function Transcript({
  session,
  project,
  readiness,
  commands
}: {
  session: AgentSession
  project: string
  readiness: Readiness
  commands: SlashCommandInfo[]
}) {
  const { state } = session
  const block = readinessBlockReason(readiness)
  const end = useRef<HTMLDivElement>(null)
  const threads = useMemo(() => repliesByComment(state.items), [state.items])
  // Where each comment sent so far points, so a reply in the flow says what it answers
  const where = useMemo(() => {
    const found = new Map<string, string>()
    for (const item of state.items) {
      if (item.kind !== 'user') continue
      for (const c of parseReview(item.text)?.comments ?? [])
        found.set(c.id, `${c.file}:${c.start === c.end ? c.start : `${c.start}-${c.end}`}`)
    }
    return found
  }, [state.items])
  useEffect(() => {
    end.current?.scrollIntoView({ block: 'end' })
  }, [state.items])

  return (
    <main className="session active">
      <div className="transcript">
        {state.items.map((item, i) => (
          <Item key={i} item={item} project={project} threads={threads} where={where} />
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
              commands={commands}
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
  const commands = useSlashCommands()

  if (state.status !== 'idle' && project)
    return (
      <Transcript session={session} project={project} readiness={readiness} commands={commands} />
    )

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
          commands={commands}
        />
      ) : project ? null : (
        <textarea disabled placeholder="Describe what you want built or changed" />
      )}
    </main>
  )
}
