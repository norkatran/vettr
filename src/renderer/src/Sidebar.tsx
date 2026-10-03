import { changedFileCount, type FileChange } from '@shared/diff'
import { useState } from 'react'
import { FileTitle, fileAnchor, filePaths } from './Changes'
import type { ChangesState } from './useChanges'

export type View = 'session' | 'changes'

interface SidebarProps {
  project: string | null
  view: View
  expanded: boolean
  onSelect: (view: View) => void
  onNewSession: () => void
  changes: ChangesState
  /** A session has started, so the empty-state hint no longer applies. */
  sessionStarted: boolean
}

const ITEMS: { view: View; label: string; icon: React.JSX.Element }[] = [
  {
    view: 'session',
    label: 'Session',
    icon: <path d="M4 5h16v11H9l-5 4z" />
  },
  {
    view: 'changes',
    label: 'Changes',
    icon: (
      <path d="M6 3v12M6 15a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 9v3a3 3 0 0 1-3 3H9" />
    )
  }
]

const STATUS_LETTER = { added: 'A', modified: 'M', deleted: 'D', renamed: 'R' } as const

const PANEL_TEXT: Record<View, string> = {
  session: 'No agent session yet. Describe a task to start one.',
  changes: 'No changes to review.'
}

const BADGE_MAX = 99

function formatBadge(count: number): string {
  return count > BADGE_MAX ? `${BADGE_MAX}+` : String(count)
}

const SESSION_STARTED_TEXT = 'Session in progress. Start a new one to clear it.'

export function Sidebar({
  project,
  view,
  expanded,
  onSelect,
  onNewSession,
  changes,
  sessionStarted
}: SidebarProps): React.JSX.Element {
  // Interim until the notification system exists
  const [error, setError] = useState<string | null>(null)
  const [message, setMessage] = useState('')
  const [committing, setCommitting] = useState(false)
  const stagedCount = changes.changes?.staged.length ?? 0
  const move = async (staged: boolean, file: FileChange): Promise<void> => {
    if (!project) return
    const paths = filePaths(file)
    setError(
      await (staged
        ? window.agentide.unstageFiles(project, paths)
        : window.agentide.stageFiles(project, paths))
    )
  }
  const canCommit = !!project && stagedCount > 0 && message.trim() !== '' && !committing
  const commit = async (): Promise<void> => {
    if (!project || !canCommit) return
    setCommitting(true)
    const failure = await window.agentide.commitStaged(project, message)
    setCommitting(false)
    setError(failure)
    // Keep the typed message on failure so nothing is lost
    if (failure === null) setMessage('')
  }
  const active = ITEMS.find((i) => i.view === view)
  const changeCount = changes.changes ? changedFileCount(changes.changes) : 0
  return (
    <>
      <nav className="activitybar" aria-label="Views">
        {ITEMS.map((item) => (
          <button
            key={item.view}
            type="button"
            className={item.view === view && expanded ? 'activity active' : 'activity'}
            title={item.label}
            aria-label={item.label}
            aria-pressed={item.view === view}
            onClick={() => onSelect(item.view)}
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              {item.icon}
            </svg>
            {item.view === 'changes' && changeCount > 0 && (
              <span
                className="activity-badge"
                role="img"
                aria-label={`${changeCount} changed files`}
              >
                {formatBadge(changeCount)}
              </span>
            )}
          </button>
        ))}
      </nav>
      {expanded && (
        <aside className="sidebar">
          <h2>{active?.label}</h2>
          {view === 'changes' && (
            <div className="commit-box">
              <textarea
                placeholder="Commit message (Ctrl+Enter to commit)"
                value={message}
                disabled={committing || !project}
                onChange={(e) => setMessage(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
                    e.preventDefault()
                    void commit()
                  }
                }}
              />
              <button type="button" disabled={!canCommit} onClick={() => void commit()}>
                {committing ? (
                  <>
                    Committing <span className="spinner" aria-hidden="true" />
                  </>
                ) : (
                  'Commit'
                )}
              </button>
            </div>
          )}
          {view === 'changes' && error && (
            <p className="diff-note error" role="alert">
              {error}
            </p>
          )}
          {view === 'changes' && changes.files && changes.files.length > 0 ? (
            <ul className="file-list">
              {changes.files.map((file, i) => {
                const staged = i < stagedCount
                return (
                  <li className="file-row" key={`${i}:${file.oldPath ?? ''}>${file.path}`}>
                    <button
                      type="button"
                      className="file-link"
                      title={file.path}
                      onClick={() =>
                        document.getElementById(fileAnchor(i))?.scrollIntoView({ block: 'start' })
                      }
                    >
                      <span className={`status-letter ${file.status}`}>
                        {STATUS_LETTER[file.status]}
                      </span>
                      <span className="file-name">
                        <FileTitle file={file} />
                      </span>
                    </button>
                    <button
                      type="button"
                      className="file-stage"
                      title={staged ? 'Unstage' : 'Stage'}
                      aria-label={`${staged ? 'Unstage' : 'Stage'} ${file.path}`}
                      onClick={() => void move(staged, file)}
                    >
                      {staged ? '−' : '+'}
                    </button>
                  </li>
                )
              })}
            </ul>
          ) : (
            <p className="hint">
              {view === 'session' && sessionStarted ? SESSION_STARTED_TEXT : PANEL_TEXT[view]}
            </p>
          )}
          {view === 'session' && (
            <button type="button" className="new-session" onClick={onNewSession}>
              New session
            </button>
          )}
        </aside>
      )}
    </>
  )
}
