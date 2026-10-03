import { defaultSettings, EDITOR_PRESETS, matchPreset, type Settings } from '@shared/settings'
import { useEffect, useState } from 'react'

const CUSTOM = '__custom__'
const NONE = ''

export function SettingsView(): React.JSX.Element {
  const [settings, setSettings] = useState<Settings | null>(null)
  // Picking "Custom" with an empty command has no value to infer it from, so remember it
  const [custom, setCustom] = useState(false)

  useEffect(() => {
    void window.agentide.getSettings().then((s) => {
      setSettings(s)
      setCustom(s.editorCommand !== '' && matchPreset(s.editorCommand) === null)
    })
  }, [])

  if (!settings) return <main className="settings" />

  const save = (next: Settings): void => {
    setSettings(next)
    void window.agentide.setSettings(next).then(setSettings)
  }
  const command = settings.editorCommand
  const selected = custom ? CUSTOM : (matchPreset(command)?.command ?? NONE)
  const choose = (value: string): void => {
    setCustom(value === CUSTOM)
    save({ ...settings, editorCommand: value === CUSTOM ? command : value })
  }

  return (
    <main className="settings">
      <h1>Settings</h1>
      <div className="settings-field">
        <label htmlFor="editor-preset">External editor</label>
        <select id="editor-preset" value={selected} onChange={(e) => choose(e.target.value)}>
          <option value={NONE}>Not set</option>
          {EDITOR_PRESETS.map((p) => (
            <option key={p.command} value={p.command}>
              {p.label}
            </option>
          ))}
          <option value={CUSTOM}>Custom command</option>
        </select>
        {selected === CUSTOM && (
          <input
            aria-label="Editor command"
            placeholder="my-editor --goto {file}:{line}"
            value={command}
            onChange={(e) => save({ ...settings, editorCommand: e.target.value })}
          />
        )}
        <p className="hint">
          Used by "Open in editor". <code>{'{file}'}</code> and <code>{'{line}'}</code> are replaced
          with the file path and line number.
        </p>
      </div>
      {command === defaultSettings.editorCommand && selected !== CUSTOM && (
        <p className="hint">No editor set yet.</p>
      )}
    </main>
  )
}
