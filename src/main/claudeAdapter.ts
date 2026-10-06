import type { ChildProcessWithoutNullStreams } from 'node:child_process'
import {
  type AgentAdapter,
  type AgentEvent,
  encodeLine,
  LineBuffer,
  parseEventLine,
  type RunnerCommand
} from '@shared/agent'

export interface ClaudeAdapterDeps {
  /** A user-facing problem with Docker or the image, or null when the sandbox can start. */
  checkDocker(): Promise<string | null>
  /** Start a sandbox container for the project; its stdio speaks the runner protocol. */
  startSandbox(project: string): Promise<ChildProcessWithoutNullStreams>
  getApiKey(): Promise<string | null>
  /** Ask the container to stop; it should end with a `close` event on the process. */
  stopSandbox(container: ChildProcessWithoutNullStreams): void
}

const STDERR_TAIL_BYTES = 2000

/** Drives Claude Code through the in-container runner. One instance holds at most one session. */
export class ClaudeAdapter implements AgentAdapter {
  private readonly listeners = new Set<(event: AgentEvent) => void>()
  private container: ChildProcessWithoutNullStreams | null = null
  private starting = false
  /** Set once the first prompt has gone to the current container; before that it is warm. */
  private active = false
  private warmCwd: string | null = null
  private warmResume: string | undefined
  private warming: Promise<void> | null = null
  private closed: Promise<void> = Promise.resolve()
  /** Set by stop() so the resulting exit is not reported as a failure. */
  private stopRequested = false

  constructor(private readonly deps: ClaudeAdapterDeps) {}

  onEvent(listener: (event: AgentEvent) => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  /**
   * Prewarm: start the container and its idle query without a prompt, so the first prompt does
   * not wait for the container. A no-op when a matching warm agent already exists.
   */
  async warm(cwd: string, resume?: string): Promise<void> {
    if (this.container && !this.active && this.warmCwd === cwd && this.warmResume === resume) return
    // Join a prewarm already in progress rather than racing it
    if (this.warming) return this.warming
    const launching = this.launch(cwd, resume)
    this.warming = launching
    try {
      await launching
    } finally {
      this.warming = null
    }
  }

  async start(prompt: string, cwd: string, resume?: string): Promise<void> {
    // A prewarm in progress is waited for; its failure is reported by launching again below
    if (this.warming) await this.warming.catch(() => undefined)
    if (this.container && !this.active) {
      if (this.warmCwd === cwd && this.warmResume === resume) {
        this.active = true
        this.write({ type: 'prompt', text: prompt })
        return
      }
      // A warm agent for another project, or one that must resume a stored session
      await this.stop()
    }
    await this.launch(cwd, resume)
    this.active = true
    this.write({ type: 'prompt', text: prompt })
  }

  /** Start the container and send `init`, leaving the agent warm and idle. */
  private async launch(cwd: string, resume?: string): Promise<void> {
    // Claim the slot before the first await so two concurrent starts cannot both proceed
    if (this.container || this.starting) throw new Error('A session is already running')
    this.starting = true
    try {
      const problem = await this.deps.checkDocker()
      if (problem) throw new Error(problem)
      const credential = await this.deps.getApiKey()
      if (!credential) {
        throw new Error('No API key or token is saved. Add one to start a session.')
      }
      const container = await this.deps.startSandbox(cwd)
      this.container = container
      this.active = false
      this.warmCwd = cwd
      this.warmResume = resume
      this.stopRequested = false
      this.watch(container)
      this.write({ type: 'init', credential, cwd, ...(resume ? { resume } : {}) })
    } finally {
      this.starting = false
    }
  }

  async send(message: string): Promise<void> {
    this.write({ type: 'prompt', text: message })
  }

  async interrupt(): Promise<void> {
    this.write({ type: 'interrupt' })
  }

  /** The MVP sandbox grants full permissions, so nothing ever asks for approval. */
  async respondToApproval(): Promise<void> {}

  async stop(): Promise<void> {
    if (!this.container) return
    this.stopRequested = true
    this.deps.stopSandbox(this.container)
    await this.closed
  }

  private emit(event: AgentEvent): void {
    for (const listener of this.listeners) listener(event)
  }

  private write(command: RunnerCommand): void {
    if (!this.container) throw new Error('No session is running')
    this.container.stdin.write(encodeLine(command))
  }

  /** Forward the container's events and report how it ended. */
  private watch(container: ChildProcessWithoutNullStreams): void {
    const lines = new LineBuffer()
    let sawError = false
    let stderrTail = ''

    container.stdout.setEncoding('utf8')
    container.stdout.on('data', (chunk: string) => {
      for (const line of lines.push(chunk)) {
        const event = parseEventLine(line)
        if (!event) continue
        if (event.type === 'error') sawError = true
        this.emit(event)
      }
    })
    container.stderr.on('data', (chunk: Buffer | string) => {
      stderrTail = (stderrTail + chunk.toString()).slice(-STDERR_TAIL_BYTES)
    })
    // A write to a dead container raises EPIPE here; the close event reports the real cause
    container.stdin.on('error', () => undefined)

    this.closed = new Promise((resolve) => {
      let finished = false
      const finish = (code: number | null, failure?: string): void => {
        if (finished) return
        finished = true
        this.container = null
        this.active = false
        this.warmCwd = null
        this.warmResume = undefined
        const message =
          failure ??
          (!this.stopRequested && !sawError && code !== 0
            ? `The sandbox stopped unexpectedly${code === null ? '' : ` (exit code ${code})`}.`
            : null)
        if (message) {
          const detail = stderrTail.trim()
          this.emit({ type: 'error', message: detail ? `${message}\n${detail}` : message })
        }
        this.emit({ type: 'exited', code })
        resolve()
      }
      container.on('error', (error) => finish(null, `Could not run Docker: ${error.message}`))
      container.on('close', (code) => finish(code))
    })
  }
}

/** Run an action for IPC: resolves to null on success or to the error message on failure. */
export async function attempt(action: () => Promise<void>): Promise<string | null> {
  try {
    await action()
    return null
  } catch (error) {
    return error instanceof Error ? error.message : String(error)
  }
}
