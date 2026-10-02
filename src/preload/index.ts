import { contextBridge, ipcRenderer } from 'electron'
import { type AgentideApi, IpcChannel } from '@shared/ipc'

const api: AgentideApi = {
  openProject: () => ipcRenderer.invoke(IpcChannel.openProject)
}

contextBridge.exposeInMainWorld('agentide', api)
