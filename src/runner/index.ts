import { query, type SDKUserMessage } from '@anthropic-ai/claude-agent-sdk'
import { type AgentEvent, encodeLine, LineBuffer, parseCommandLine } from '../shared/agent'
import { Translator } from './translate'

// Runs inside the sandbox container. Reads commands from stdin and writes events to stdout,
// one JSON object per line (see src/shared/agent.ts). Anything else goes to stderr.

const emit = (event: AgentEvent) => process.stdout.write(encodeLine(event))

/** Prompts waiting to be fed to the SDK; the SDK pulls from it for multi-turn sessions. */
class PromptQueue implements AsyncIterable<SDKUserMessage> {
  private readonly items: SDKUserMessage[] = []
  private waiting: ((result: IteratorResult<SDKUserMessage>) => void) | null = null
  private closed = false

  push(text: string) {
    const item: SDKUserMessage = {
      type: 'user',
      message: { role: 'user', content: text },
      parent_tool_use_id: null
    }
    if (this.waiting) {
      this.waiting({ value: item, done: false })
      this.waiting = null
    } else {
      this.items.push(item)
    }
  }

  close() {
    this.closed = true
    this.waiting?.({ value: undefined, done: true })
  }

  [Symbol.asyncIterator](): AsyncIterator<SDKUserMessage> {
    return {
      next: () => {
        const item = this.items.shift()
        if (item) return Promise.resolve({ value: item, done: false })
        if (this.closed) return Promise.resolve({ value: undefined, done: true })
        return new Promise((resolve) => {
          this.waiting = resolve
        })
      }
    }
  }
}

const prompts = new PromptQueue()
let session: ReturnType<typeof query> | null = null

async function pump(cwd: string, apiKey: string) {
  const translator = new Translator()
  session = query({
    prompt: prompts,
    options: {
      cwd,
      // The container is the safety boundary, so the agent gets full permissions
      permissionMode: 'bypassPermissions',
      allowDangerouslySkipPermissions: true,
      // Project settings and CLAUDE.md only, never user settings from the container's home
      settingSources: ['project'],
      env: { ...process.env, ANTHROPIC_API_KEY: apiKey },
      stderr: (data) => process.stderr.write(data)
    }
  })
  try {
    for await (const message of session) {
      for (const event of translator.translate(message)) emit(event)
    }
  } catch (error) {
    emit({ type: 'error', message: error instanceof Error ? error.message : String(error) })
    process.exitCode = 1
  }
}

const lines = new LineBuffer()
process.stdin.setEncoding('utf8')
process.stdin.on('data', (chunk: string) => {
  for (const line of lines.push(chunk)) {
    const command = parseCommandLine(line)
    if (!command) {
      process.stderr.write(`ignoring malformed command: ${line}\n`)
    } else if (command.type === 'init') {
      if (!session) void pump(command.cwd, command.apiKey)
    } else if (!session) {
      emit({ type: 'error', message: 'Received a command before init' })
    } else if (command.type === 'prompt') {
      prompts.push(command.text)
    } else {
      void session.interrupt()
    }
  }
})
process.stdin.on('end', () => prompts.close())
