/** Typed IPC contract shared by main, preload and renderer. */
export interface AgentideApi {
  /** Show a native folder picker; resolves to the chosen path or null. */
  openProject(): Promise<string | null>
}

export const IpcChannel = {
  openProject: 'project:open'
} as const
