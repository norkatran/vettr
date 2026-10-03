import { type AgentideApi, IpcChannel } from '@shared/ipc'
import { contextBridge, type IpcRendererEvent, ipcRenderer } from 'electron'

const api: AgentideApi = {
  onProjectOpened: (callback) => {
    const listener = (_event: IpcRendererEvent, path: string): void => callback(path)
    ipcRenderer.on(IpcChannel.projectOpened, listener)
    return () => ipcRenderer.removeListener(IpcChannel.projectOpened, listener)
  },
  onRepoChanged: (callback) => {
    const listener = (): void => callback()
    ipcRenderer.on(IpcChannel.repoChanged, listener)
    return () => ipcRenderer.removeListener(IpcChannel.repoChanged, listener)
  },
  getCurrentProject: () => ipcRenderer.invoke(IpcChannel.getCurrentProject),
  getRepoStatus: (project) => ipcRenderer.invoke(IpcChannel.getRepoStatus, project),
  getChanges: (project) => ipcRenderer.invoke(IpcChannel.getChanges, project)
}

contextBridge.exposeInMainWorld('agentide', api)
