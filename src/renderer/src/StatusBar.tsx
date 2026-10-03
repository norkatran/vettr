import type { RepoStatus } from '@shared/repoStatus'
import { otherTheme, type Theme } from '@shared/theme'
import { useEffect, useState } from 'react'
import { useNotify } from './Notifications'

interface StatusBarProps {
  project: string | null
  theme: Theme
  onToggleTheme: () => void
  /** Bump to reload the status, for git changes the file watcher cannot see (fetch, pull, push). */
  refreshKey: number
}

function useRepoStatus(
  project: string | null,
  refreshKey: number
): [RepoStatus | null, () => void] {
  const [status, setStatus] = useState<RepoStatus | null>(null)
  const [refreshTick, setRefreshTick] = useState(0)

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
  }, [project, refreshTick, refreshKey])

  return [status, () => setRefreshTick((n) => n + 1)]
}

function repoName(project: string): string {
  return project.split(/[\\/]/).filter(Boolean).pop() ?? project
}

export function StatusBar({
  project,
  theme,
  onToggleTheme,
  refreshKey
}: StatusBarProps): React.JSX.Element {
  const [status, refreshStatus] = useRepoStatus(project, refreshKey)
  const [pushing, setPushing] = useState(false)
  const notify = useNotify()
  const push = async (): Promise<void> => {
    if (!project || pushing) return
    setPushing(true)
    const failure = await window.agentide.push(project)
    setPushing(false)
    refreshStatus()
    if (failure) notify('Push failed', failure)
  }
  // Remotes to choose from when publishing a branch with several; null when the menu is closed
  const [remotes, setRemotes] = useState<string[] | null>(null)
  const publishTo = async (remote: string): Promise<void> => {
    if (!project || pushing) return
    setRemotes(null)
    setPushing(true)
    const failure = await window.agentide.publish(project, remote)
    setPushing(false)
    refreshStatus()
    if (failure) notify('Publish failed', failure)
  }
  // Like VS Code's "Publish Branch": one remote publishes straight away, several ask which
  const publish = async (): Promise<void> => {
    if (!project || pushing) return
    if (remotes) return setRemotes(null)
    const names = await window.agentide.listRemotes(project)
    if (names.length === 0) {
      notify(
        'Cannot publish branch',
        'This repository has no remotes. Add one with `git remote add`.'
      )
    } else if (names.length === 1) {
      await publishTo(names[0] as string)
    } else {
      setRemotes(names)
    }
  }
  const target = otherTheme(theme)

  return (
    <footer className="statusbar">
      <div className="status-group">
        {project && <span title={project}>{repoName(project)}</span>}
        {status && (
          <>
            {(() => {
              const content = (
                <>
                  <span>{status.branch ?? `${status.sha ?? 'no commits'} (detached)`}</span>
                  {status.upstream ? (
                    (status.behind > 0 || status.ahead > 0) && (
                      <span>
                        {status.behind > 0 && `↓${status.behind}`}
                        {status.behind > 0 && status.ahead > 0 && ' '}
                        {status.ahead > 0 && `↑${status.ahead}`}
                      </span>
                    )
                  ) : (
                    <span className="muted">no upstream</span>
                  )}
                  {pushing && <span className="spinner" aria-hidden="true" />}
                </>
              )
              if (!status.upstream && status.branch) {
                return (
                  <span className="publish-wrap">
                    <button
                      type="button"
                      className="status-button status-branch"
                      disabled={pushing}
                      onClick={() => void publish()}
                      title="Publish this branch to a remote and set its upstream"
                    >
                      {content}
                      <span>Publish Branch</span>
                    </button>
                    {remotes && (
                      <ul className="remote-menu" aria-label="Publish to remote">
                        {remotes.map((name) => (
                          <li key={name}>
                            <button type="button" onClick={() => void publishTo(name)}>
                              {name}
                            </button>
                          </li>
                        ))}
                      </ul>
                    )}
                  </span>
                )
              }
              return status.upstream && status.ahead > 0 ? (
                <button
                  type="button"
                  className="status-button status-branch"
                  disabled={pushing}
                  onClick={() => void push()}
                  title={`Click to push to ${status.upstream}`}
                >
                  {content}
                </button>
              ) : (
                <span
                  className="status-branch"
                  title={status.upstream ? `Compared with ${status.upstream}` : 'Current branch'}
                >
                  {content}
                </span>
              )
            })()}
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
