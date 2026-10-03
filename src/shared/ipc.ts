import type { AgentEvent } from './agent'
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
  /**
   * Start a session in the current project with a first prompt. Resolves to null on success or
   * to a message for the user (Docker missing, no API key, ...).
   */
  agentStart(prompt: string): Promise<string | null>
  /** Send a follow-up in the running session; resolves to null or an error message. */
  agentSend(message: string): Promise<string | null>
  agentInterrupt(): Promise<void>
  /** End the session and stop its container. */
  agentStop(): Promise<void>
  /** Subscribe to events from the running session; returns an unsubscribe function. */
  onAgentEvent(callback: (event: AgentEvent) => void): () => void
  /** Whether an Anthropic API key is saved. The key itself never reaches the renderer. */
  hasApiKey(): Promise<boolean>
  /** Save the API key (encrypted on the host); resolves to null or an error message. */
  setApiKey(key: string): Promise<string | null>
}

export const IpcChannel = {
  projectOpened: 'project:opened',
  getCurrentProject: 'project:current',
  repoChanged: 'repo:changed',
  getRepoStatus: 'repo:status',
  getChanges: 'repo:changes',
  agentStart: 'agent:start',
  agentSend: 'agent:send',
  agentInterrupt: 'agent:interrupt',
  agentStop: 'agent:stop',
  agentEvent: 'agent:event',
  hasApiKey: 'apikey:status',
  setApiKey: 'apikey:set'
} as const
