import { existsSync } from 'node:fs'
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
import { forgetProject, getProjectState, loadProjectState, setCurrentProject } from './projectStore'

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

function activateProject(win: BrowserWindow | undefined, path: string): void {
  setCurrentProject(path)
  buildMenu()
  const target = win ?? BrowserWindow.getAllWindows()[0]
  target?.webContents.send(IpcChannel.projectOpened, path)
}

function openRecentProject(win: BrowserWindow | undefined, path: string): void {
  if (existsSync(path)) {
    activateProject(win, path)
  } else {
    forgetProject(path)
    buildMenu()
  }
}

async function openProject(win: BrowserWindow | undefined): Promise<void> {
  const options: OpenDialogOptions = { properties: ['openDirectory'] }
  const result = win
    ? await dialog.showOpenDialog(win, options)
    : await dialog.showOpenDialog(options)
  const path = result.filePaths[0]
  if (result.canceled || !path) return
  activateProject(win, path)
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
            click: (_item, win) => openRecentProject(win as BrowserWindow | undefined, path)
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

void app.whenReady().then(() => {
  loadProjectState()
  ipcMain.handle(IpcChannel.getCurrentProject, () => getProjectState().current)
  buildMenu()
  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') app.quit()
})
