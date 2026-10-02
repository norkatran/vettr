/** Typed IPC contract shared by main, preload and renderer. */
export interface AgentideApi {
  /**
   * Subscribe to projects chosen via File > Open Project. The callback gets
   * the chosen folder path; returns an unsubscribe function.
   */
  onProjectOpened(callback: (path: string) => void): () => void
}

export const IpcChannel = {
  projectOpened: 'project:opened'
} as const
