import { type AgentideApi, IpcChannel } from '@shared/ipc'
import { contextBridge, type IpcRendererEvent, ipcRenderer } from 'electron'

const api: AgentideApi = {
  onProjectOpened: (callback) => {
    const listener = (_event: IpcRendererEvent, path: string): void => callback(path)
    ipcRenderer.on(IpcChannel.projectOpened, listener)
    return () => ipcRenderer.removeListener(IpcChannel.projectOpened, listener)
  },
  getCurrentProject: () => ipcRenderer.invoke(IpcChannel.getCurrentProject),
  getRepoStatus: (project) => ipcRenderer.invoke(IpcChannel.getRepoStatus, project)
}

contextBridge.exposeInMainWorld('agentide', api)
