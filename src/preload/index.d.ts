import type { AgentideApi } from '@shared/ipc'

declare global {
  interface Window {
    agentide: AgentideApi
  }
}
