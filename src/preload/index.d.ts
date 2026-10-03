import type { VettrApi } from '@shared/ipc'

declare global {
  interface Window {
    vettr: VettrApi
  }
}
