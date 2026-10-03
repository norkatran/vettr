import type { FileChange } from './diff'
import type { RepoStatus } from './repoStatus'

/** Typed IPC contract shared by main, preload and renderer. */
export interface AgentideApi {
  /**
   * Subscribe to projects chosen via File > Open Project or Open Recent. The callback gets
   * the chosen folder path; returns an unsubscribe function.
   */
  onProjectOpened(callback: (path: string) => void): () => void
  /** The persisted current project from the last session, or null. */
  getCurrentProject(): Promise<string | null>
  /** Branch, upstream divergence and change count for the repo at `project`, or null if unreadable. */
  /** Subscribe to working-tree or git changes in the open project; returns an unsubscribe function. */
  onRepoChanged(callback: () => void): () => void
  getRepoStatus(project: string): Promise<RepoStatus | null>
  /** Working tree against HEAD, untracked files included, or null if it cannot be read. */
  getChanges(project: string): Promise<FileChange[] | null>
}

export const IpcChannel = {
  projectOpened: 'project:opened',
  getCurrentProject: 'project:current',
  repoChanged: 'repo:changed',
  getRepoStatus: 'repo:status',
  getChanges: 'repo:changes'
} as const
