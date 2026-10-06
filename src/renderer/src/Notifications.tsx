import { createContext, useCallback, useContext, useMemo, useState } from 'react'

export type NotifyLevel = 'error' | 'info'

interface Notification {
  id: number
  title: string
  detail: string
  level: NotifyLevel
}

type NotifyError = (title: string, detail: string, level?: NotifyLevel) => void

const NotifyContext = createContext<NotifyError>(() => {})

// Report a failure (default) or a piece of information as a dismissable popup in the top-right.
// Errors stay until dismissed so the user can read git's output; information fades after a while.
export const useNotify = (): NotifyError => useContext(NotifyContext)

let nextId = 1
const INFO_MS = 6000

export function NotificationProvider({
  children
}: {
  children: React.ReactNode
}): React.JSX.Element {
  const [items, setItems] = useState<Notification[]>([])
  const dismiss = useCallback(
    (id: number): void => setItems((prev) => prev.filter((n) => n.id !== id)),
    []
  )
  const notify = useCallback<NotifyError>(
    (title, detail, level = 'error') => {
      const id = nextId++
      setItems((prev) => [...prev, { id, title, detail, level }])
      if (level === 'info') setTimeout(() => dismiss(id), INFO_MS)
    },
    [dismiss]
  )
  const value = useMemo(() => notify, [notify])

  return (
    <NotifyContext.Provider value={value}>
      {children}
      <div className="notifications" aria-live="polite">
        {items.map((n) => (
          <div
            className={`notification ${n.level}`}
            role={n.level === 'error' ? 'alert' : 'status'}
            key={n.id}
          >
            <div className="notification-body">
              <strong>{n.title}</strong>
              <pre>{n.detail}</pre>
            </div>
            <button
              type="button"
              className="notification-close"
              aria-label="Dismiss notification"
              onClick={() => dismiss(n.id)}
            >
              ×
            </button>
          </div>
        ))}
      </div>
    </NotifyContext.Provider>
  )
}
