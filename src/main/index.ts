import { execFile, spawn } from 'node:child_process'
import { access, readFile } from 'node:fs/promises'
import { join, relative, resolve } from 'node:path'
import { promisify } from 'node:util'
import { getSessionMessages, listSessions as sdkListSessions } from '@anthropic-ai/claude-agent-sdk'
import { buildEditorCommand } from '@shared/editor'
import type { GitAction } from '@shared/gitActions'
import { IpcChannel } from '@shared/ipc'
import { SANDBOX_IMAGE } from '@shared/sandbox'
import {
  app,
  BrowserWindow,
  dialog,
  ipcMain,
  Menu,
  type MenuItemConstructorOptions,
  type OpenDialogOptions,
  safeStorage
} from 'electron'
import icon from '../../branding/icons/vettr-app-icon-512.png?asset'
import { AgentManager } from './agentManager'
import { createApiKeyStore } from './apiKey'
import { saveApiKey } from './apiKeyCheck'
import { confirmIfBusy } from './busyGuard'
import { attempt, ClaudeAdapter } from './claudeAdapter'
import {
  commitStaged,
  findRepoRoot,
  getChanges,
  getChangesSince,
  getRepoStatus,
  listBranches,
  listRemotes,
  publishBranch,
  pushCurrent,
  runGitAction,
  snapshotTree,
  stageFiles,
  unstageFiles
} from './git'
import { forgetProject, getProjectState, loadProjectState, setCurrentProject } from './projectStore'
import { readResolved, setResolved } from './resolvedStore'
import {
  checkDocker,
  checkDockerDetailed,
  isProcessAlive,
  startSandbox,
  stopSandbox,
  sweepOrphans
} from './sandbox'
import { buildImageFromApp } from './sandboxImage'
import { listProjectSessions, loadSession } from './sessions'
import { getSettings, loadSettings, updateSettings } from './settingsStore'
import { projectDataDir, transcriptsDir } from './transcripts'
import { watchTree } from './watcher'

const apiKeys = createApiKeyStore(join(app.getPath('userData'), 'apikey'), {
  isAvailable: () => safeStorage.isEncryptionAvailable(),
  encrypt: (plain) => safeStorage.encryptString(plain),
  decrypt: (encrypted) => safeStorage.decryptString(encrypted)
})
const exec = promisify(execFile)
const agent = new ClaudeAdapter({
  checkDocker: () => checkDocker((file, args) => exec(file, args)),
  startSandbox: (project) =>
    startSandbox({
      project,
      ownerPid: process.pid,
      transcriptsDir: transcriptsDir(app.getPath('userData'), project),
      spawn,
      // Linux first, so these exist; the container runs as the host user
      uid: process.getuid?.() as number,
      gid: process.getgid?.() as number
    }),
  getApiKey: () => apiKeys.get(),
  stopSandbox
})
agent.onEvent((event) => {
  for (const win of BrowserWindow.getAllWindows())
    win.webContents.send(IpcChannel.agentEvent, event)
})
const agentManager = new AgentManager({
  agent,
  checkDocker: () => checkDockerDetailed((file, args) => exec(file, args)),
  buildImage: (onProgress) =>
    buildImageFromApp({
      spawn,
      appPath: app.getAppPath(),
      image: SANDBOX_IMAGE,
      exists: (path) =>
        access(path).then(
          () => true,
          () => false
        ),
      readFile: (path) => readFile(path, 'utf8'),
      onProgress
    }),
  hasKey: async () => (await apiKeys.get()) !== null
})
agentManager.onReadiness((readiness) => {
  for (const win of BrowserWindow.getAllWindows())
    win.webContents.send(IpcChannel.readiness, readiness)
})

function createWindow(): void {
  const win = new BrowserWindow({
    width: 1200,
    height: 800,
    icon,
    webPreferences: {
      preload: join(__dirname, '../preload/index.js'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: true
    }
  })

  if (process.env['ELECTRON_RENDERER_URL']) {
    void win.loadURL(process.env['ELECTRON_RENDERER_URL'])
  } else {
    void win.loadFile(join(__dirname, '../renderer/index.html'))
  }
}

let stopWatching: (() => Promise<void>) | null = null

/** Watch `path` and tell every window when its working tree or git state changes. */
async function watchProject(path: string): Promise<void> {
  const previous = stopWatching
  stopWatching = null
  await previous?.()
  stopWatching = await watchTree(path, () => {
    for (const win of BrowserWindow.getAllWindows()) win.webContents.send(IpcChannel.repoChanged)
  })
}

function activateProject(win: BrowserWindow | undefined, path: string): void {
  // The manager tears the old project's agent down and prewarms one for this project
  void agentManager.setProject(path).catch(() => undefined)
  setCurrentProject(path)
  void watchProject(path)
  buildMenu()
  const target = win ?? BrowserWindow.getAllWindows()[0]
  target?.webContents.send(IpcChannel.projectOpened, path)
}

/**
 * Open `path` as the project, resolving it to its git repo root. If it is not
 * in a git repository, tell the user and drop it from the recent list.
 */
async function openProjectAt(win: BrowserWindow | undefined, path: string): Promise<void> {
  const root = await findRepoRoot(path)
  if (root) {
    if (root !== getProjectState().current && agentManager.isBusy && !(await confirmSwitch(win))) {
      return
    }
    activateProject(win, root)
    return
  }
  forgetProject(path)
  buildMenu()
  const message = `${path} is not a git repository.`
  const detail = 'Choose a folder that is inside a git repository.'
  const options = { type: 'error' as const, message, detail }
  await (win ? dialog.showMessageBox(win, options) : dialog.showMessageBox(options))
}

/** Ask before stopping an agent that is working, since its work in progress is lost. */
async function confirmStop(
  win: BrowserWindow | undefined,
  detail: string,
  confirmLabel: string
): Promise<boolean> {
  const options = {
    type: 'warning' as const,
    message: 'The agent is still working.',
    detail,
    buttons: [confirmLabel, 'Cancel'],
    defaultId: 1,
    cancelId: 1
  }
  const result = await (win ? dialog.showMessageBox(win, options) : dialog.showMessageBox(options))
  return result.response === 0
}

const confirmSwitch = (win: BrowserWindow | undefined): Promise<boolean> =>
  confirmStop(
    win,
    'Opening another project stops it and the work in progress is lost.',
    'Open project'
  )

async function openProject(win: BrowserWindow | undefined): Promise<void> {
  const options: OpenDialogOptions = { properties: ['openDirectory'] }
  const result = win
    ? await dialog.showOpenDialog(win, options)
    : await dialog.showOpenDialog(options)
  const path = result.filePaths[0]
  if (result.canceled || !path) return
  await openProjectAt(win, path)
}

function buildMenu(): void {
  const isMac = process.platform === 'darwin'
  const { recent } = getProjectState()
  const template: MenuItemConstructorOptions[] = [
    ...(isMac ? [{ role: 'appMenu' } as MenuItemConstructorOptions] : []),
    {
      label: 'File',
      submenu: [
        {
          label: 'Open Project...',
          accelerator: 'CmdOrCtrl+O',
          click: (_item, win) => void openProject(win as BrowserWindow | undefined)
        },
        {
          label: 'Recent Projects',
          enabled: recent.length > 0,
          submenu: recent.map((path) => ({
            label: path,
            click: (_item, win) => void openProjectAt(win as BrowserWindow | undefined, path)
          }))
        },
        { type: 'separator' },
        isMac ? { role: 'close' } : { role: 'quit' }
      ]
    },
    { role: 'editMenu' },
    { role: 'viewMenu' },
    { role: 'windowMenu' }
  ]
  Menu.setApplicationMenu(Menu.buildFromTemplate(template))
}

void app.whenReady().then(async () => {
  loadProjectState()
  loadSettings()
  const { current } = getProjectState()
  if (current && !(await findRepoRoot(current))) forgetProject(current)
  ipcMain.handle(IpcChannel.getCurrentProject, () => getProjectState().current)
  ipcMain.handle(IpcChannel.getRepoStatus, (_event, project: string) => getRepoStatus(project))
  ipcMain.handle(IpcChannel.getChanges, (_event, project: string) => getChanges(project))
  ipcMain.handle(IpcChannel.snapshotTree, (_event, project: string) => snapshotTree(project))
  ipcMain.handle(IpcChannel.getChangesSince, (_event, project: string, tree: string) =>
    getChangesSince(project, tree)
  )
  ipcMain.handle(IpcChannel.stageFiles, (_event, project: string, paths: string[]) =>
    stageFiles(project, paths)
  )
  ipcMain.handle(IpcChannel.unstageFiles, (_event, project: string, paths: string[]) =>
    unstageFiles(project, paths)
  )
  ipcMain.handle(IpcChannel.commitStaged, (_event, project: string, message: string) =>
    commitStaged(project, message)
  )
  ipcMain.handle(IpcChannel.push, (_event, project: string) => pushCurrent(project))
  ipcMain.handle(IpcChannel.listRemotes, (_event, project: string) => listRemotes(project))
  ipcMain.handle(IpcChannel.publish, (_event, project: string, remote: string) =>
    publishBranch(project, remote)
  )
  ipcMain.handle(IpcChannel.listBranches, (_event, project: string) => listBranches(project))
  ipcMain.handle(IpcChannel.runGitAction, (_event, project: string, action: GitAction) =>
    runGitAction(project, action)
  )
  ipcMain.handle(IpcChannel.agentStart, (_event, prompt: string, resume?: string) =>
    attempt(async () => {
      const project = getProjectState().current
      if (!project) throw new Error('Open a project first')
      await agentManager.start(prompt, resume)
    })
  )
  ipcMain.handle(IpcChannel.agentSend, (_event, message: string) =>
    attempt(() => agentManager.send(message))
  )
  ipcMain.handle(IpcChannel.agentInterrupt, () =>
    attempt(() => agentManager.interrupt()).then(() => {})
  )
  ipcMain.handle(IpcChannel.listSessions, async () => {
    const project = getProjectState().current
    if (!project) return []
    return listProjectSessions(
      { listSessions: sdkListSessions, env: process.env },
      transcriptsDir(app.getPath('userData'), project),
      project
    )
  })
  ipcMain.handle(IpcChannel.loadSession, async (_event, id: string) => {
    const project = getProjectState().current
    if (!project) return null
    return loadSession(
      { getSessionMessages, env: process.env },
      transcriptsDir(app.getPath('userData'), project),
      project,
      id
    )
  })
  ipcMain.handle(IpcChannel.getResolvedComments, (_event, project: string) =>
    readResolved(projectDataDir(app.getPath('userData'), project))
  )
  ipcMain.handle(
    IpcChannel.setCommentResolved,
    (_event, project: string, id: string, resolved: boolean) =>
      setResolved(projectDataDir(app.getPath('userData'), project), id, resolved)
  )
  ipcMain.handle(IpcChannel.agentStop, () => agentManager.newSession())
  ipcMain.handle(IpcChannel.getSlashCommands, () => agentManager.slashCommands)
  ipcMain.handle(IpcChannel.getReadiness, () => agentManager.readiness)
  ipcMain.handle(
    IpcChannel.openInEditor,
    async (_event, project: string, path: string, line: number) => {
      const noEditor = 'No editor is set. Choose one in Settings.'
      const template = getSettings().editorCommand
      if (!template) return noEditor
      const file = resolve(project, path)
      if (relative(project, file).startsWith('..')) return 'That file is outside the project.'
      const built = buildEditorCommand(
        template,
        file,
        Number.isInteger(line) && line > 0 ? line : 1,
        project
      )
      if (!built) return noEditor
      return new Promise<string | null>((done) => {
        const child = spawn(built.command, built.args, {
          cwd: project,
          detached: true,
          stdio: 'ignore'
        })
        child.once('error', (err) => done(`Could not run "${built.command}": ${err.message}`))
        child.once('spawn', () => {
          child.unref()
          done(null)
        })
      })
    }
  )
  ipcMain.handle(IpcChannel.getSettings, () => getSettings())
  ipcMain.handle(IpcChannel.setSettings, (_event, next: unknown) => updateSettings(next))
  ipcMain.handle(IpcChannel.hasApiKey, async () => (await apiKeys.get()) !== null)
  ipcMain.handle(IpcChannel.setApiKey, (event, key: string) =>
    attempt(async () => {
      const win = BrowserWindow.fromWebContents(event.sender) ?? undefined
      await saveApiKey(
        apiKeys,
        key,
        (url, init) => fetch(url, init),
        () =>
          confirmIfBusy(agentManager.isBusy, () =>
            confirmStop(
              win,
              'Saving a new key restarts the agent, and the work in progress is lost.',
              'Save key'
            )
          )
      )
      // Always restart the agent with the new credential
      void agentManager.keyChanged().catch(() => undefined)
    })
  )
  ipcMain.handle(IpcChannel.clearApiKey, (event) =>
    attempt(async () => {
      const win = BrowserWindow.fromWebContents(event.sender) ?? undefined
      await confirmIfBusy(agentManager.isBusy, () =>
        confirmStop(
          win,
          'Removing the key stops the agent, and the work in progress is lost.',
          'Remove key'
        )
      )
      await apiKeys.clear()
      // With no key the agent stops and inputs that direct it are disabled
      void agentManager.keyChanged().catch(() => undefined)
    })
  )
  buildMenu()
  // Containers left by an earlier run that died are removed in the background; ours carry this
  // process's pid, so this is safe to run alongside the first prewarm
  void sweepOrphans(async (file, args) => (await exec(file, args)).stdout, isProcessAlive).catch(
    () => 0
  )
  const launchProject = getProjectState().current
  if (launchProject) {
    void watchProject(launchProject)
    void agentManager.setProject(launchProject).catch(() => undefined)
  }
  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('will-quit', () => {
  void agentManager.shutdown().catch(() => undefined)
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})
