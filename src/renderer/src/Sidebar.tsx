export type View = 'session' | 'changes'

interface SidebarProps {
  view: View
  expanded: boolean
  onSelect: (view: View) => void
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

const PANEL_TEXT: Record<View, string> = {
  session: 'No agent session yet. Describe a task to start one.',
  changes: 'No changes to review.'
}

export function Sidebar({ view, expanded, onSelect }: SidebarProps): React.JSX.Element {
  const active = ITEMS.find((i) => i.view === view)
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
          </button>
        ))}
      </nav>
      {expanded && (
        <aside className="sidebar">
          <h2>{active?.label}</h2>
          <p className="hint">{PANEL_TEXT[view]}</p>
        </aside>
      )}
    </>
  )
}
