import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  createSdkMcpServer,
  query,
  type SDKUserMessage,
  tool
} from '@anthropic-ai/claude-agent-sdk'
import { z } from 'zod'
import { type AgentEvent, encodeLine, LineBuffer, parseCommandLine } from '../shared/agent'
import { credentialEnv } from '../shared/credential'
import { REPLY_DESCRIPTION, REPLY_SERVER, REPLY_TOOL, ReplyTracker } from './replyTool'
import { Translator, toCommandInfo } from './translate'

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
let replies = new ReplyTracker(true)

async function pump(cwd: string, credential: string, resume?: string) {
  const translator = new Translator()
  replies = new ReplyTracker(!resume)
  // The reply tool does nothing itself: the app reads the call from the event stream
  const replyServer = createSdkMcpServer({
    name: REPLY_SERVER,
    tools: [
      tool(
        REPLY_TOOL,
        REPLY_DESCRIPTION,
        {
          comment_id: z.string().describe('The id of the comment you are replying to'),
          message: z.string().describe('Your reply'),
          kind: z.enum(['question', 'resolved']).optional()
        },
        async ({ comment_id, message, kind }) => {
          const result = replies.reply(comment_id, message, kind)
          return { content: [{ type: 'text', text: result.text }], isError: result.isError }
        }
      )
    ]
  })
  session = query({
    prompt: prompts,
    options: {
      cwd,
      ...(resume ? { resume } : {}),
      // The container is the safety boundary, so the agent gets full permissions
      mcpServers: { [REPLY_SERVER]: replyServer },
      permissionMode: 'bypassPermissions',
      allowDangerouslySkipPermissions: true,
      // Project settings and CLAUDE.md only, never user settings from the container's home
      settingSources: ['project'],
      env: {
        ...process.env,
        ...credentialEnv(credential),
        // Our own temp dir: Claude Code refuses /tmp/claude-<uid> when it is root-owned, which
        // happens if the project's host path (mounted at the same path) runs through /tmp
        CLAUDE_CODE_TMPDIR: mkdtempSync(join(tmpdir(), 'vettr-'))
      },
      stderr: (data) => process.stderr.write(data)
    }
  })
  // The idle query answers before any prompt; later changes arrive as `commands_changed`
  session.supportedCommands().then(
    (commands) => emit({ type: 'commands', commands: toCommandInfo(commands) }),
    (error) => process.stderr.write(`could not list slash commands: ${error}\n`)
  )
  // A failed turn is reported by the translator and then makes the SDK throw as well; report once
  let reported = false
  try {
    for await (const message of session) {
      for (const event of translator.translate(message)) {
        if (event.type === 'error') reported = true
        emit(event)
      }
    }
  } catch (error) {
    if (!reported) {
      emit({ type: 'error', message: error instanceof Error ? error.message : String(error) })
    }
    // The session is over, so stop reading stdin: the process then exits and the host sees it
    process.exitCode = 1
    process.stdin.destroy()
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
      if (!session) void pump(command.cwd, command.credential, command.resume)
    } else if (!session) {
      emit({ type: 'error', message: 'Received a command before init' })
    } else if (command.type === 'prompt') {
      replies.noteMessage(command.text)
      prompts.push(command.text)
    } else {
      void session.interrupt()
    }
  }
})
process.stdin.on('end', () => prompts.close())
