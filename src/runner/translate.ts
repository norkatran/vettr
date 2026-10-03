import type { AgentEvent } from '../shared/agent'

/** The parts of the Agent SDK's messages the runner reads; kept structural so tests need no SDK. */
export interface SdkMessage {
  type: string
  subtype?: string
  is_error?: boolean
  result?: string
  errors?: string[]
  message?: unknown
}

interface Block {
  type?: string
  text?: string
  id?: string
  name?: string
  input?: unknown
  tool_use_id?: string
  content?: unknown
  is_error?: boolean
}

/** Tools whose successful completion means a file changed; the value is the input field with its path. */
const EDIT_TOOLS: Record<string, string> = {
  Edit: 'file_path',
  MultiEdit: 'file_path',
  Write: 'file_path',
  NotebookEdit: 'notebook_path'
}

/** Tool result content is a string or an array of blocks; keep the text. */
function resultText(content: unknown): string {
  if (typeof content === 'string') return content
  if (!Array.isArray(content)) return ''
  return content
    .map((block: Block) => (block.type === 'text' ? block.text : ''))
    .filter(Boolean)
    .join('\n')
}

function blocksOf(message: SdkMessage): Block[] {
  const inner = message.message
  const content = typeof inner === 'object' && inner !== null ? (inner as Block).content : null
  return Array.isArray(content) ? content : []
}

/**
 * Turns the SDK's message stream into app events. Stateful because a file-edited event is only
 * emitted once the matching tool call finishes without an error.
 */
export class Translator {
  private readonly editPaths = new Map<string, string>()

  translate(message: SdkMessage): AgentEvent[] {
    if (message.type === 'assistant') return this.assistant(message)
    if (message.type === 'user') return this.toolResults(message)
    if (message.type === 'result') return this.result(message)
    return []
  }

  private assistant(message: SdkMessage): AgentEvent[] {
    const events: AgentEvent[] = []
    for (const block of blocksOf(message)) {
      if (block.type === 'text' && block.text) {
        events.push({ type: 'text', text: block.text })
      } else if (block.type === 'tool_use') {
        const id = block.id as string
        const name = block.name as string
        events.push({ type: 'tool-started', id, name, input: block.input })
        const field = EDIT_TOOLS[name]
        const path = field ? (block.input as Record<string, unknown> | undefined)?.[field] : null
        if (typeof path === 'string') this.editPaths.set(id, path)
      }
    }
    return events
  }

  private toolResults(message: SdkMessage): AgentEvent[] {
    const events: AgentEvent[] = []
    for (const block of blocksOf(message)) {
      if (block.type !== 'tool_result') continue
      const id = block.tool_use_id as string
      const isError = block.is_error === true
      events.push({ type: 'tool-finished', id, output: resultText(block.content), isError })
      const path = this.editPaths.get(id)
      this.editPaths.delete(id)
      if (path && !isError) events.push({ type: 'file-edited', path })
    }
    return events
  }

  private result(message: SdkMessage): AgentEvent[] {
    const events: AgentEvent[] = []
    if (message.subtype !== 'success' || message.is_error) {
      const detail = message.errors?.join('; ') || message.result || message.subtype
      events.push({ type: 'error', message: `The agent stopped: ${detail}` })
    }
    events.push({ type: 'turn-finished' })
    return events
  }
}
