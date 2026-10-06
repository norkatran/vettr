import { defaultSettings, EDITOR_PRESETS, matchPreset, type Settings } from '@shared/settings'
import { useEffect, useState } from 'react'
import { ApiKeyForm } from './ApiKeyForm'
import type { ApiKey } from './useApiKey'

const CUSTOM = '__custom__'
const NONE = ''

/**
 * The saved key or token, managed here. Its value is never shown once saved: the user can only
 * replace it or remove it.
 */
function ApiKeySetting({ apiKey }: { apiKey: ApiKey }): React.JSX.Element | null {
  const [changing, setChanging] = useState(false)
  const [confirmingRemove, setConfirmingRemove] = useState(false)
  const [error, setError] = useState<string | null>(null)
  if (apiKey.hasKey === null) return null

  if (!apiKey.hasKey) {
    return (
      <div className="settings-field">
        <ApiKeyForm
          onSave={apiKey.save}
          explanation="No key or token is saved, so the agent cannot start."
        />
      </div>
    )
  }
  if (changing) {
    return (
      <div className="settings-field">
        <ApiKeyForm
          saveLabel="Replace"
          onSave={async (key) => {
            const failure = await apiKey.save(key)
            if (!failure) setChanging(false)
            return failure
          }}
          onCancel={() => setChanging(false)}
        />
      </div>
    )
  }
  return (
    <div className="settings-field">
      <span className="settings-label">Anthropic API key or Claude OAuth token</span>
      <p className="hint">
        A key or token is saved. For security it is not shown again; replace it or remove it.
      </p>
      <div className="key-row">
        <button
          type="button"
          title="Saving a new key restarts the agent, which ends any session in progress"
          onClick={() => setChanging(true)}
        >
          Change
        </button>
        {confirmingRemove ? (
          <>
            <button
              type="button"
              title="Stops the agent, ending any session in progress"
              onClick={() => {
                setConfirmingRemove(false)
                void apiKey.clear().then(setError)
              }}
            >
              Confirm remove
            </button>
            <button type="button" className="secondary" onClick={() => setConfirmingRemove(false)}>
              Cancel
            </button>
          </>
        ) : (
          <button
            type="button"
            className="secondary"
            title="Removing the key stops the agent"
            onClick={() => setConfirmingRemove(true)}
          >
            Remove
          </button>
        )}
      </div>
      {error && <p className="error-text">{error}</p>}
    </div>
  )
}

export function SettingsView({ apiKey }: { apiKey: ApiKey }): React.JSX.Element {
  const [settings, setSettings] = useState<Settings | null>(null)
  // Picking "Custom" with an empty command has no value to infer it from, so remember it
  const [custom, setCustom] = useState(false)

  useEffect(() => {
    void window.vettr.getSettings().then((s) => {
      setSettings(s)
      setCustom(s.editorCommand !== '' && matchPreset(s.editorCommand) === null)
    })
  }, [])

  if (!settings) return <main className="settings" />

  const save = (next: Settings): void => {
    setSettings(next)
    void window.vettr.setSettings(next).then(setSettings)
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
      <ApiKeySetting apiKey={apiKey} />
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
          Used by "Open in editor". <code>{'{file}'}</code>, <code>{'{line}'}</code> and{' '}
          <code>{'{project}'}</code> are replaced with the file path, line number and project
          folder.
        </p>
      </div>
      {command === defaultSettings.editorCommand && selected !== CUSTOM && (
        <p className="hint">No editor set yet.</p>
      )}
    </main>
  )
}
