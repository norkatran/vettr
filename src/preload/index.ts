import { type AgentideApi, IpcChannel } from '@shared/ipc'
import { contextBridge, ipcRenderer } from 'electron'

const api: AgentideApi = {
  openProject: () => ipcRenderer.invoke(IpcChannel.openProject)
}

contextBridge.exposeInMainWorld('agentide', api)
