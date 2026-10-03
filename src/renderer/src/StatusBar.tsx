import type { RepoStatus } from '@shared/repoStatus'
import { otherTheme, type Theme } from '@shared/theme'
import { useEffect, useState } from 'react'

interface StatusBarProps {
  project: string | null
  theme: Theme
  onToggleTheme: () => void
}

function useRepoStatus(project: string | null): RepoStatus | null {
  const [status, setStatus] = useState<RepoStatus | null>(null)

  useEffect(() => {
    if (!project) {
      setStatus(null)
      return
    }
    let stale = false
    const refresh = (): void => {
      void window.agentide.getRepoStatus(project).then((next) => {
        if (!stale) setStatus(next)
      })
    }
    refresh()
    // No polling: the main process pushes changes, and focus covers anything missed.
    window.addEventListener('focus', refresh)
    const unsubscribe = window.agentide.onRepoChanged(refresh)
    return () => {
      stale = true
      unsubscribe()
      window.removeEventListener('focus', refresh)
    }
  }, [project])

  return status
}

function repoName(project: string): string {
  return project.split(/[\\/]/).filter(Boolean).pop() ?? project
}

export function StatusBar({ project, theme, onToggleTheme }: StatusBarProps): React.JSX.Element {
  const status = useRepoStatus(project)
  const target = otherTheme(theme)

  return (
    <footer className="statusbar">
      <div className="status-group">
        {project && <span title={project}>{repoName(project)}</span>}
        {status && (
          <>
            <span title="Current branch">
              {status.branch ?? `${status.sha ?? 'no commits'} (detached)`}
            </span>
            {status.upstream ? (
              (status.behind > 0 || status.ahead > 0) && (
                <span title={`Compared with ${status.upstream}`}>
                  {status.behind > 0 && `↓${status.behind}`}
                  {status.behind > 0 && status.ahead > 0 && ' '}
                  {status.ahead > 0 && `↑${status.ahead}`}
                </span>
              )
            ) : (
              <span className="muted">no upstream</span>
            )}
            <span title="Changed files, including untracked">
              {status.changes === 0 ? 'clean' : `${status.changes} changed`}
            </span>
          </>
        )}
      </div>
      <button
        type="button"
        className="status-button"
        onClick={onToggleTheme}
        title={`Switch to ${target} theme`}
      >
        {theme === 'dark' ? 'Dark' : 'Light'}
      </button>
    </footer>
  )
}
