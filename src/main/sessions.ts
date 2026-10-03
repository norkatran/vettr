import type { SessionState } from '../shared/session'
import type { SessionInfo } from '../shared/sessions'
import { replaySession, type StoredMessage } from './replay'

/** The part of the Agent SDK's `listSessions` this module uses. */
export interface SdkSessionInfo {
  sessionId: string
  summary: string
  lastModified: number
}

export interface SessionsDeps {
  listSessions(options: { dir: string }): Promise<SdkSessionInfo[]>
  /** The environment the SDK reads `CLAUDE_CONFIG_DIR` from (`process.env`). */
  env: Record<string, string | undefined>
}

/** SDK calls read the config dir from the environment, so they must not overlap. */
let queue: Promise<unknown> = Promise.resolve()

/**
 * Runs `fn` with `CLAUDE_CONFIG_DIR` pointing at a project's transcripts folder, one call at a
 * time, and restores the previous value afterwards.
 */
export function withConfigDir<T>(
  env: SessionsDeps['env'],
  configDir: string,
  fn: () => Promise<T>
): Promise<T> {
  const run = async (): Promise<T> => {
    const previous = env.CLAUDE_CONFIG_DIR
    env.CLAUDE_CONFIG_DIR = configDir
    try {
      return await fn()
    } finally {
      if (previous === undefined) delete env.CLAUDE_CONFIG_DIR
      else env.CLAUDE_CONFIG_DIR = previous
    }
  }
  const result = queue.then(run)
  queue = result.catch(() => {})
  return result
}

/** The project's stored sessions, newest first. Empty when there are none or they are unreadable. */
export async function listProjectSessions(
  deps: SessionsDeps,
  configDir: string,
  project: string
): Promise<SessionInfo[]> {
  try {
    const sessions = await withConfigDir(deps.env, configDir, () =>
      deps.listSessions({ dir: project })
    )
    return sessions
      .map((s) => ({ id: s.sessionId, title: s.summary, lastModified: s.lastModified }))
      .sort((a, b) => b.lastModified - a.lastModified)
  } catch {
    return []
  }
}

export interface LoadDeps {
  getSessionMessages(id: string, options: { dir: string }): Promise<StoredMessage[]>
  env: SessionsDeps['env']
}

/** A stored session rebuilt as Session view state, or null when it cannot be read. */
export async function loadSession(
  deps: LoadDeps,
  configDir: string,
  project: string,
  id: string
): Promise<SessionState | null> {
  try {
    const messages = await withConfigDir(deps.env, configDir, () =>
      deps.getSessionMessages(id, { dir: project })
    )
    if (messages.length === 0) return null
    return { ...replaySession(messages), sessionId: id }
  } catch {
    return null
  }
}
