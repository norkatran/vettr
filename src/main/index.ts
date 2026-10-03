import { join } from 'node:path'
import { IpcChannel } from '@shared/ipc'
import {
  app,
  BrowserWindow,
  dialog,
  ipcMain,
  Menu,
  type MenuItemConstructorOptions,
  type OpenDialogOptions
} from 'electron'
import { findRepoRoot, getChanges, getRepoStatus } from './git'
import { forgetProject, getProjectState, loadProjectState, setCurrentProject } from './projectStore'
import { watchTree } from './watcher'

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
  const { current } = getProjectState()
  if (current && !(await findRepoRoot(current))) forgetProject(current)
  ipcMain.handle(IpcChannel.getCurrentProject, () => getProjectState().current)
  ipcMain.handle(IpcChannel.getRepoStatus, (_event, project: string) => getRepoStatus(project))
  ipcMain.handle(IpcChannel.getChanges, (_event, project: string) => getChanges(project))
  buildMenu()
  if (getProjectState().current) void watchProject(getProjectState().current as string)
  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})
