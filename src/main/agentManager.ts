import type { AgentEvent, SlashCommandInfo } from '@shared/agent'
import { INITIAL_READINESS, type Readiness, readinessBlockReason } from '@shared/readiness'
import type { DockerProblem } from '@shared/sandbox'

/** The part of the agent adapter the manager drives (implemented by `ClaudeAdapter`). */
export interface AgentPort {
  warm(cwd: string, resume?: string): Promise<void>
  start(prompt: string, cwd: string, resume?: string): Promise<void>
  send(message: string): Promise<void>
  interrupt(): Promise<void>
  stop(): Promise<void>
  onEvent(listener: (event: AgentEvent) => void): () => void
}

export interface AgentManagerDeps {
  agent: AgentPort
  /** What is wrong with Docker or the image, or null when the sandbox can start. */
  checkDocker(): Promise<DockerProblem | null>
  /**
   * Build the sandbox image, reporting build output lines. Resolves to null on success or to an
   * error message; absent (or resolving to `undefined`) when the app cannot build it here.
   */
  buildImage?(onProgress: (line: string) => void): Promise<string | null | undefined>
  hasKey(): Promise<boolean>
}

/** Unrequested exits in a row (with no finished turn between) before giving up restarting. */
const MAX_CRASHES = 3

/**
 * Owns the agent's lifecycle and readiness for the open project (design 0002). All transitions
 * run through one queue, so a start never overlaps a stop, and work for a project that has been
 * replaced since it was queued is dropped.
 */
export class AgentManager {
  private state: Readiness = INITIAL_READINESS
  private readonly listeners = new Set<(readiness: Readiness) => void>()
  private tail: Promise<void> = Promise.resolve()
  private project: string | null = null
  /** Bumped when the project changes, so queued and in-flight work for the old one is discarded. */
  private generation = 0
  /** True while the manager itself stops the agent, so that exit is not taken for a crash. */
  private stopping = false
  /** A turn is running. */
  private busy = false
  /** The warm agent has received a prompt, so a new session needs a fresh one. */
  private used = false
  private warmResume: string | undefined
  private sessionId: string | null = null
  private crashes = 0
  /** The latest slash commands the agent reported; empty while no agent is running. */
  private commands: SlashCommandInfo[] = []

  constructor(private readonly deps: AgentManagerDeps) {
    deps.agent.onEvent((event) => this.onAgentEvent(event))
  }

  /** The slash commands the running agent offers (the renderer may load after the event). */
  get slashCommands(): SlashCommandInfo[] {
    return this.commands
  }

  get readiness(): Readiness {
    return this.state
  }

  /** Whether the agent is working on a turn (losing it would lose work in progress). */
  get isBusy(): boolean {
    return this.busy
  }

  onReadiness(listener: (readiness: Readiness) => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  /** Open `project` (or none). The same project is a no-op; another one tears the agent down. */
  setProject(project: string | null): Promise<void> {
    if (project === this.project) return this.tail
    this.project = project
    this.generation++
    this.sessionId = null
    this.crashes = 0
    this.busy = false
    return this.enqueue(this.generation, undefined)
  }

  /** The credential changed or was removed: always restart, keeping the stored session. */
  keyChanged(): Promise<void> {
    return this.enqueue(this.generation, this.sessionId ?? undefined)
  }

  /** End the current session and warm a fresh agent (nothing to do if it was never used). */
  async newSession(): Promise<void> {
    this.sessionId = null
    if (!this.used && this.state.status === 'ready') return
    await this.enqueue(this.generation, undefined)
  }

  /** Start a session with a prompt, resuming stored session `resume` if given. */
  async start(prompt: string, resume?: string): Promise<void> {
    this.requireReady()
    if (this.used || resume !== this.warmResume) {
      await this.enqueue(this.generation, resume)
      this.requireReady()
    }
    this.used = true
    this.busy = true
    if (resume) this.sessionId = resume
    try {
      await this.deps.agent.start(prompt, this.project as string, resume)
    } catch (error) {
      this.busy = false
      throw error
    }
  }

  async send(message: string): Promise<void> {
    this.requireReady()
    this.used = true
    this.busy = true
    try {
      await this.deps.agent.send(message)
    } catch (error) {
      this.busy = false
      throw error
    }
  }

  async interrupt(): Promise<void> {
    this.requireReady()
    await this.deps.agent.interrupt()
  }

  /** Stop the agent for good (app quit). */
  async shutdown(): Promise<void> {
    this.generation++
    await this.enqueue(this.generation, undefined, false)
  }

  private requireReady(): void {
    const block = readinessBlockReason(this.state)
    if (block) throw new Error(block)
  }

  private set(next: Readiness): void {
    this.state = next
    for (const listener of this.listeners) listener(next)
  }

  private enqueue(generation: number, resume: string | undefined, warm = true): Promise<void> {
    const job = this.tail.then(() => this.cycle(generation, resume, warm))
    this.tail = job.catch(() => undefined)
    return job
  }

  /** Stop whatever is running, then check the prerequisites and warm an agent. */
  private async cycle(
    generation: number,
    resume: string | undefined,
    warm: boolean
  ): Promise<void> {
    if (this.state.status === 'ready') this.set({ status: 'stopping' })
    this.stopping = true
    try {
      await this.deps.agent.stop()
    } finally {
      this.stopping = false
    }
    this.busy = false
    this.used = false
    this.warmResume = undefined
    const stale = (): boolean => generation !== this.generation
    if (stale()) return
    const project = this.project
    if (!project || !warm) {
      this.set(INITIAL_READINESS)
      return
    }
    let docker = await this.deps.checkDocker()
    if (stale()) return
    if (docker?.kind === 'image' && this.deps.buildImage) {
      this.set({ status: 'starting', reason: 'building-image' })
      const failure = await this.deps.buildImage((line) => {
        if (!stale()) {
          this.set({
            status: 'starting',
            reason: 'building-image',
            message: `Building the sandbox image… ${line}`
          })
        }
      })
      if (stale()) return
      // undefined: this app cannot build the image, so the original problem stands
      if (failure !== undefined) {
        docker = failure ? { kind: 'image', message: failure } : await this.deps.checkDocker()
        if (stale()) return
      }
    }
    if (docker) {
      this.set({
        status: 'error',
        reason: docker.kind === 'docker' ? 'docker-unavailable' : 'error',
        message: docker.message
      })
      return
    }
    const hasKey = await this.deps.hasKey()
    if (stale()) return
    if (!hasKey) {
      this.set({ status: 'idle', reason: 'no-key' })
      return
    }
    this.set({
      status: 'starting',
      reason: this.state.reason === 'crashed-restarting' ? 'crashed-restarting' : 'starting'
    })
    try {
      await this.deps.agent.warm(project, resume)
    } catch (error) {
      if (stale()) return
      const message = error instanceof Error ? error.message : String(error)
      this.set({ status: 'error', reason: 'error', message })
      return
    }
    // A stale warm agent is stopped by the next queued cycle, which always stops first
    if (stale()) return
    this.warmResume = resume
    this.set({ status: 'ready' })
  }

  private onAgentEvent(event: AgentEvent): void {
    if (event.type === 'commands') {
      this.commands = event.commands
    } else if (event.type === 'exited') {
      this.commands = []
    }
    if (event.type === 'session-started') {
      this.sessionId = event.sessionId
    } else if (event.type === 'turn-finished') {
      this.busy = false
      this.crashes = 0
    } else if (event.type === 'exited' && !this.stopping && this.state.status === 'ready') {
      this.busy = false
      this.restartAfterCrash()
    }
  }

  private restartAfterCrash(): void {
    if (++this.crashes > MAX_CRASHES) {
      this.set({
        status: 'error',
        reason: 'error',
        message: 'The agent keeps stopping unexpectedly.'
      })
      return
    }
    this.set({ status: 'starting', reason: 'crashed-restarting' })
    void this.enqueue(this.generation, this.sessionId ?? undefined)
  }
}
