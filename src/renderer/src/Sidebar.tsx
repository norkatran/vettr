import { changedFileCount } from '@shared/diff'
import { FileTitle, fileAnchor } from './Changes'
import type { ChangesState } from './useChanges'

export type View = 'session' | 'changes'

interface SidebarProps {
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
  view,
  expanded,
  onSelect,
  onNewSession,
  changes,
  sessionStarted
}: SidebarProps): React.JSX.Element {
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
          {view === 'changes' && changes.files && changes.files.length > 0 ? (
            <ul className="file-list">
              {changes.files.map((file, i) => (
                <li key={`${i}:${file.oldPath ?? ''}>${file.path}`}>
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
                </li>
              ))}
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
