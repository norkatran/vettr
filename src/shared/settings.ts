/** User settings persisted on the host. Add new fields here with a default. */
export interface Settings {
  /** Command template for "Open in editor"; `{file}`, `{line}` and `{project}` (the project folder) are substituted. Empty means unset. */
  editorCommand: string
}

export const defaultSettings: Settings = { editorCommand: '' }

export interface EditorPreset {
  label: string
  command: string
}

export const EDITOR_PRESETS: EditorPreset[] = [
  { label: 'VS Code', command: 'code {project} -g {file}:{line}' },
  { label: 'Cursor', command: 'cursor {project} -g {file}:{line}' },
  { label: 'Zed', command: 'zed {project} {file}:{line}' },
  { label: 'Sublime Text', command: 'subl {project} {file}:{line}' },
  { label: 'IntelliJ IDEA', command: 'idea {project} --line {line} {file}' }
]

/** The preset whose command equals `command`, or null when it is empty or custom. */
export function matchPreset(command: string): EditorPreset | null {
  return EDITOR_PRESETS.find((p) => p.command === command) ?? null
}

/** Validate untrusted JSON read from disk, falling back to defaults per field. */
export function parseSettings(raw: unknown): Settings {
  if (typeof raw !== 'object' || raw === null) return defaultSettings
  const { editorCommand } = raw as Record<string, unknown>
  return { editorCommand: typeof editorCommand === 'string' ? editorCommand.trim() : '' }
}
