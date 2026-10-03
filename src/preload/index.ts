import type { AgentEvent } from '@shared/agent'
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
  getChanges: (project) => ipcRenderer.invoke(IpcChannel.getChanges, project),
  stageFiles: (project, paths) => ipcRenderer.invoke(IpcChannel.stageFiles, project, paths),
  unstageFiles: (project, paths) => ipcRenderer.invoke(IpcChannel.unstageFiles, project, paths),
  commitStaged: (project, message) => ipcRenderer.invoke(IpcChannel.commitStaged, project, message),
  push: (project) => ipcRenderer.invoke(IpcChannel.push, project),
  listRemotes: (project) => ipcRenderer.invoke(IpcChannel.listRemotes, project),
  publish: (project, remote) => ipcRenderer.invoke(IpcChannel.publish, project, remote),
  agentStart: (prompt) => ipcRenderer.invoke(IpcChannel.agentStart, prompt),
  agentSend: (message) => ipcRenderer.invoke(IpcChannel.agentSend, message),
  agentInterrupt: () => ipcRenderer.invoke(IpcChannel.agentInterrupt),
  agentStop: () => ipcRenderer.invoke(IpcChannel.agentStop),
  onAgentEvent: (callback) => {
    const listener = (_event: IpcRendererEvent, event: AgentEvent): void => callback(event)
    ipcRenderer.on(IpcChannel.agentEvent, listener)
    return () => ipcRenderer.removeListener(IpcChannel.agentEvent, listener)
  },
  hasApiKey: () => ipcRenderer.invoke(IpcChannel.hasApiKey),
  setApiKey: (key) => ipcRenderer.invoke(IpcChannel.setApiKey, key)
}

contextBridge.exposeInMainWorld('agentide', api)
