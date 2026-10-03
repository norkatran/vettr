import type { AgentEvent } from './agent'
import type { FileChange, RepoChanges } from './diff'
import type { Branch, GitAction } from './gitActions'
import type { RepoStatus } from './repoStatus'
import type { SessionState } from './session'
import type { SessionInfo } from './sessions'
import type { Settings } from './settings'

/** Typed IPC contract shared by main, preload and renderer. */
export interface VettrApi {
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
  /** Changes split into staged (index against HEAD) and unstaged (working tree against index), or null if unreadable. */
  getChanges(project: string): Promise<RepoChanges | null>
  /** Record the working tree as a git tree and return its id (the round baseline), or null on failure. */
  snapshotTree(project: string): Promise<string | null>
  /** The working tree against a tree from `snapshotTree`, or null if unreadable. */
  getChangesSince(project: string, tree: string): Promise<FileChange[] | null>
  /** Stage whole files (paths relative to the repo root); resolves to null or git's error message. */
  stageFiles(project: string, paths: string[]): Promise<string | null>
  /** Unstage whole files (include a renamed file's old path); resolves to null or git's error message. */
  unstageFiles(project: string, paths: string[]): Promise<string | null>
  /** Commit the staged files with the user's message; resolves to null or git's error output. */
  commitStaged(project: string, message: string): Promise<string | null>
  /** Push the current branch to its upstream; resolves to null or git's error output. */
  push(project: string): Promise<string | null>
  /** Configured remote names, with `origin` first. */
  listRemotes(project: string): Promise<string[]>
  /** Publish the current branch to a remote with `push -u`, setting its upstream; resolves to null or git's error output. */
  publish(project: string, remote: string): Promise<string | null>
  /** Local and remote branches of the repo (local first), or an empty list on error. */
  listBranches(project: string): Promise<Branch[]>
  /** Run a command palette git action; resolves to null or git's error output. */
  runGitAction(project: string, action: GitAction): Promise<string | null>
  /**
   * Start a session in the current project with a first prompt. Resolves to null on success or
   * to a message for the user (Docker missing, no API key, ...).
   */
  agentStart(prompt: string, resume?: string): Promise<string | null>
  /** Send a follow-up in the running session; resolves to null or an error message. */
  agentSend(message: string): Promise<string | null>
  agentInterrupt(): Promise<void>
  /** End the session and stop its container. */
  agentStop(): Promise<void>
  /** The open project's stored sessions, newest first (empty when none or no project). */
  listSessions(): Promise<SessionInfo[]>
  /** A stored session of the open project rebuilt as Session view state, or null if unreadable. */
  loadSession(id: string): Promise<SessionState | null>
  /** Subscribe to events from the running session; returns an unsubscribe function. */
  onAgentEvent(callback: (event: AgentEvent) => void): () => void
  /** Whether an Anthropic API key is saved. The key itself never reaches the renderer. */
  hasApiKey(): Promise<boolean>
  /** Save the API key (encrypted on the host); resolves to null or an error message. */
  setApiKey(key: string): Promise<string | null>
  /** Open a project file in the user's editor at `line`; resolves to null or a message for the user. */
  openInEditor(project: string, path: string, line: number): Promise<string | null>
  getSettings(): Promise<Settings>
  /** Validate and persist settings; resolves to the settings now in effect. */
  setSettings(settings: Settings): Promise<Settings>
}

export const IpcChannel = {
  projectOpened: 'project:opened',
  getCurrentProject: 'project:current',
  repoChanged: 'repo:changed',
  getRepoStatus: 'repo:status',
  getChanges: 'repo:changes',
  snapshotTree: 'repo:snapshot',
  getChangesSince: 'repo:since',
  stageFiles: 'repo:stage',
  unstageFiles: 'repo:unstage',
  commitStaged: 'repo:commit',
  push: 'repo:push',
  listRemotes: 'repo:remotes',
  publish: 'repo:publish',
  listBranches: 'repo:branches',
  runGitAction: 'repo:action',
  agentStart: 'agent:start',
  agentSend: 'agent:send',
  agentInterrupt: 'agent:interrupt',
  agentStop: 'agent:stop',
  agentEvent: 'agent:event',
  listSessions: 'agent:sessions',
  loadSession: 'agent:session',
  hasApiKey: 'apikey:status',
  setApiKey: 'apikey:set',
  openInEditor: 'editor:open',
  getSettings: 'settings:get',
  setSettings: 'settings:set'
} as const
