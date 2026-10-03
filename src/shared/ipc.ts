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
  getRepoStatus(project: string): Promise<RepoStatus | null>
}

export const IpcChannel = {
  projectOpened: 'project:opened',
  getCurrentProject: 'project:current',
  getRepoStatus: 'repo:status'
} as const
