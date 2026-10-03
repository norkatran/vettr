import {
  initialSession,
  type SessionState,
  sessionReducer,
  type TranscriptItem
} from '@shared/session'
import { Translator } from '../runner/translate'

/** The parts of the SDK's `SessionMessage` the replay reads. */
export interface StoredMessage {
  type: string
  session_id: string
  message: unknown
}

const INTERRUPT_MARKER = '[Request interrupted by user'

/** The text of a user message typed by the user, or null for tool results and non-text content. */
function userText(message: unknown): string | null {
  const content =
    typeof message === 'object' && message !== null
      ? (message as { content?: unknown }).content
      : null
  if (typeof content === 'string') return content
  if (!Array.isArray(content)) return null
  const texts = content
    .filter(
      (b): b is { type: 'text'; text: string } => b?.type === 'text' && typeof b.text === 'string'
    )
    .map((b) => b.text)
  return texts.length > 0 ? texts.join('\n') : null
}

/** Harness-injected user messages (slash command echoes, system reminders) start with a tag. */
function isInjected(text: string): boolean {
  return text.trimStart().startsWith('<')
}

/**
 * Rebuilds the Session view from a stored transcript by running the messages through the same
 * translator and reducer as a live session. Errors and other transient notices are not stored,
 * so they do not come back. The session is left waiting for input, with unfinished tools stopped.
 */
export function replaySession(messages: StoredMessage[]): SessionState {
  const translator = new Translator()
  let state: SessionState = initialSession
  for (const message of messages) {
    const text = message.type === 'user' ? userText(message.message) : null
    if (text !== null) {
      if (text.startsWith(INTERRUPT_MARKER)) {
        const notice: TranscriptItem = { kind: 'notice', text: 'Interrupted' }
        state = { ...state, items: [...state.items, notice] }
      } else if (!isInjected(text)) {
        state = sessionReducer(state, { type: 'sent', text })
      }
      continue
    }
    for (const event of translator.translate(message)) {
      state = sessionReducer(state, { type: 'event', event })
    }
  }
  return sessionReducer(state, { type: 'event', event: { type: 'turn-finished' } })
}
