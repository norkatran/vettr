import { execFile, spawn } from 'node:child_process'
import { join, relative, resolve } from 'node:path'
import { promisify } from 'node:util'
import { getSessionMessages, listSessions as sdkListSessions } from '@anthropic-ai/claude-agent-sdk'
import { buildEditorCommand } from '@shared/editor'
import type { GitAction } from '@shared/gitActions'
import { IpcChannel } from '@shared/ipc'
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
import { createApiKeyStore } from './apiKey'
import { saveApiKey } from './apiKeyCheck'
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
import { checkDocker, startSandbox, stopSandbox } from './sandbox'
import { listProjectSessions, loadSession } from './sessions'
import { getSettings, loadSettings, updateSettings } from './settingsStore'
import { transcriptsDir } from './transcripts'
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

function createWindow(): void {
  const win = new BrowserWindow({
    width: 1200,
    height: 800,
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
  // A session belongs to one project's container
  void agent.stop()
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
      await agent.start(prompt, project, resume)
    })
  )
  ipcMain.handle(IpcChannel.agentSend, (_event, message: string) =>
    attempt(() => agent.send(message))
  )
  ipcMain.handle(IpcChannel.agentInterrupt, () => attempt(() => agent.interrupt()).then(() => {}))
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
  ipcMain.handle(IpcChannel.agentStop, () => agent.stop())
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
  ipcMain.handle(IpcChannel.setApiKey, (_event, key: string) =>
    attempt(() => saveApiKey(apiKeys, key, (url, init) => fetch(url, init)))
  )
  buildMenu()
  if (getProjectState().current) void watchProject(getProjectState().current as string)
  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('will-quit', () => {
  void agent.stop()
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})
