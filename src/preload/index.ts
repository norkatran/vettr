import type { AgentEvent } from '@shared/agent'
import { IpcChannel, type VettrApi } from '@shared/ipc'
import type { ProfilesState } from '@shared/profiles'
import type { Readiness } from '@shared/readiness'
import { contextBridge, type IpcRendererEvent, ipcRenderer } from 'electron'

const api: VettrApi = {
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
  snapshotTree: (project) => ipcRenderer.invoke(IpcChannel.snapshotTree, project),
  getChangesSince: (project, tree) => ipcRenderer.invoke(IpcChannel.getChangesSince, project, tree),
  stageFiles: (project, paths) => ipcRenderer.invoke(IpcChannel.stageFiles, project, paths),
  unstageFiles: (project, paths) => ipcRenderer.invoke(IpcChannel.unstageFiles, project, paths),
  commitStaged: (project, message) => ipcRenderer.invoke(IpcChannel.commitStaged, project, message),
  push: (project) => ipcRenderer.invoke(IpcChannel.push, project),
  listRemotes: (project) => ipcRenderer.invoke(IpcChannel.listRemotes, project),
  publish: (project, remote) => ipcRenderer.invoke(IpcChannel.publish, project, remote),
  listBranches: (project) => ipcRenderer.invoke(IpcChannel.listBranches, project),
  runGitAction: (project, action) => ipcRenderer.invoke(IpcChannel.runGitAction, project, action),
  agentStart: (prompt, resume) => ipcRenderer.invoke(IpcChannel.agentStart, prompt, resume),
  agentSend: (message) => ipcRenderer.invoke(IpcChannel.agentSend, message),
  agentInterrupt: () => ipcRenderer.invoke(IpcChannel.agentInterrupt),
  agentStop: () => ipcRenderer.invoke(IpcChannel.agentStop),
  listSessions: () => ipcRenderer.invoke(IpcChannel.listSessions),
  loadSession: (id: string) => ipcRenderer.invoke(IpcChannel.loadSession, id),
  getResolvedComments: (project) => ipcRenderer.invoke(IpcChannel.getResolvedComments, project),
  setCommentResolved: (project, id, resolved) =>
    ipcRenderer.invoke(IpcChannel.setCommentResolved, project, id, resolved),
  onAgentEvent: (callback) => {
    const listener = (_event: IpcRendererEvent, event: AgentEvent): void => callback(event)
    ipcRenderer.on(IpcChannel.agentEvent, listener)
    return () => ipcRenderer.removeListener(IpcChannel.agentEvent, listener)
  },
  getSlashCommands: () => ipcRenderer.invoke(IpcChannel.getSlashCommands),
  getReadiness: () => ipcRenderer.invoke(IpcChannel.getReadiness),
  onReadiness: (callback) => {
    const listener = (_event: IpcRendererEvent, readiness: Readiness): void => callback(readiness)
    ipcRenderer.on(IpcChannel.readiness, listener)
    return () => ipcRenderer.removeListener(IpcChannel.readiness, listener)
  },
  getProfiles: () => ipcRenderer.invoke(IpcChannel.getProfiles),
  addProfile: (name, credential) => ipcRenderer.invoke(IpcChannel.addProfile, name, credential),
  updateProfile: (id, changes) => ipcRenderer.invoke(IpcChannel.updateProfile, id, changes),
  removeProfile: (id) => ipcRenderer.invoke(IpcChannel.removeProfile, id),
  setActiveProfile: (id) => ipcRenderer.invoke(IpcChannel.setActiveProfile, id),
  onProfilesChanged: (callback) => {
    const listener = (_event: IpcRendererEvent, state: ProfilesState): void => callback(state)
    ipcRenderer.on(IpcChannel.profilesChanged, listener)
    return () => ipcRenderer.removeListener(IpcChannel.profilesChanged, listener)
  },
  openInEditor: (project, path, line) =>
    ipcRenderer.invoke(IpcChannel.openInEditor, project, path, line),
  getSettings: () => ipcRenderer.invoke(IpcChannel.getSettings),
  setSettings: (settings) => ipcRenderer.invoke(IpcChannel.setSettings, settings)
}

contextBridge.exposeInMainWorld('vettr', api)
