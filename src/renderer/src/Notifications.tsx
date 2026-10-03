import { createContext, useCallback, useContext, useMemo, useState } from 'react'

interface Notification {
  id: number
  title: string
  detail: string
}

type NotifyError = (title: string, detail: string) => void

const NotifyContext = createContext<NotifyError>(() => {})

// Report a failure as a dismissable popup in the top-right. Errors stay until dismissed so the
// user can read git's output.
export const useNotify = (): NotifyError => useContext(NotifyContext)

let nextId = 1

export function NotificationProvider({
  children
}: {
  children: React.ReactNode
}): React.JSX.Element {
  const [items, setItems] = useState<Notification[]>([])
  const notify = useCallback<NotifyError>((title, detail) => {
    setItems((prev) => [...prev, { id: nextId++, title, detail }])
  }, [])
  const dismiss = (id: number): void => setItems((prev) => prev.filter((n) => n.id !== id))
  const value = useMemo(() => notify, [notify])

  return (
    <NotifyContext.Provider value={value}>
      {children}
      <div className="notifications" aria-live="polite">
        {items.map((n) => (
          <div className="notification" role="alert" key={n.id}>
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
