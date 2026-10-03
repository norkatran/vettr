import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { defaultSettings, parseSettings, type Settings } from '@shared/settings'
import { app } from 'electron'

let settings: Settings = defaultSettings

function filePath(): string {
  return join(app.getPath('userData'), 'settings.json')
}

export function loadSettings(): void {
  try {
    settings = parseSettings(JSON.parse(readFileSync(filePath(), 'utf8')))
  } catch {
    settings = defaultSettings
  }
}

export function getSettings(): Settings {
  return settings
}

/** Validate, store and persist; resolves to the settings now in effect. */
export function updateSettings(next: unknown): Settings {
  settings = parseSettings(next)
  try {
    mkdirSync(dirname(filePath()), { recursive: true })
    writeFileSync(filePath(), JSON.stringify(settings, null, 2))
  } catch (err) {
    console.error('Failed to save settings', err)
  }
  return settings
}
