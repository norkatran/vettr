import type { ProfileInfo } from '@shared/profiles'
import { defaultSettings, EDITOR_PRESETS, matchPreset, type Settings } from '@shared/settings'
import { useEffect, useState } from 'react'
import { ApiKeyForm } from './ApiKeyForm'
import type { Profiles } from './useProfiles'

const CUSTOM = '__custom__'
const NONE = ''

/** One saved profile: use it, edit it (rename or replace its key) or remove it. */
function ProfileRow({
  profile,
  active,
  profiles
}: {
  profile: ProfileInfo
  active: boolean
  profiles: Profiles
}): React.JSX.Element {
  const [editing, setEditing] = useState(false)
  const [confirmingRemove, setConfirmingRemove] = useState(false)
  const [error, setError] = useState<string | null>(null)
  if (editing) {
    return (
      <li className="profile-row">
        <ApiKeyForm
          saveLabel="Save"
          initialName={profile.name}
          keyOptional
          onSave={async (name, key) => {
            const failure = await profiles.update(profile.id, {
              name,
              ...(key ? { credential: key } : {})
            })
            if (!failure) setEditing(false)
            return failure
          }}
          onCancel={() => setEditing(false)}
        />
      </li>
    )
  }
  return (
    <li className="profile-row">
      <div className="profile-line">
        <span className="profile-name">{profile.name}</span>
        {active && <span className="profile-badge">In use</span>}
        <span className="profile-spacer" />
        {!active && (
          <button
            type="button"
            title="Use this profile in this window; restarts the agent"
            onClick={() => void profiles.use(profile.id).then(setError)}
          >
            Use
          </button>
        )}
        <button type="button" className="secondary" onClick={() => setEditing(true)}>
          Edit
        </button>
        {confirmingRemove ? (
          <>
            <button
              type="button"
              onClick={() => {
                setConfirmingRemove(false)
                void profiles.remove(profile.id).then(setError)
              }}
            >
              Confirm remove
            </button>
            <button type="button" className="secondary" onClick={() => setConfirmingRemove(false)}>
              Cancel
            </button>
          </>
        ) : (
          <button type="button" className="secondary" onClick={() => setConfirmingRemove(true)}>
            Remove
          </button>
        )}
      </div>
      {error && <p className="error-text">{error}</p>}
    </li>
  )
}

/**
 * The saved profiles, managed here. Credentials are never shown once saved: a profile can only be
 * renamed, given a new key, or removed. The profile in use applies to this app instance only.
 */
function ProfilesSetting({ profiles }: { profiles: Profiles }): React.JSX.Element | null {
  const [adding, setAdding] = useState(false)
  if (!profiles.state) return null
  const list = profiles.state.profiles
  return (
    <div className="settings-field">
      <span className="settings-label">Claude profiles</span>
      <p className="hint">
        {list.length === 0
          ? 'No profile is saved, so the agent cannot start.'
          : 'Each profile is a named API key or OAuth token. The one in use applies to this app instance only; other open instances keep theirs.'}
      </p>
      {list.length > 0 && (
        <ul className="profile-list">
          {list.map((p) => (
            <ProfileRow
              key={p.id}
              profile={p}
              active={p.id === profiles.state?.activeId}
              profiles={profiles}
            />
          ))}
        </ul>
      )}
      {list.length === 0 || adding ? (
        <ApiKeyForm
          saveLabel="Add profile"
          onSave={async (name, key) => {
            const failure = await profiles.add(name, key)
            if (!failure) setAdding(false)
            return failure
          }}
          onCancel={list.length > 0 ? () => setAdding(false) : undefined}
        />
      ) : (
        <div className="key-row">
          <button type="button" onClick={() => setAdding(true)}>
            Add profile
          </button>
        </div>
      )}
    </div>
  )
}

export function SettingsView({ profiles }: { profiles: Profiles }): React.JSX.Element {
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
      <ProfilesSetting profiles={profiles} />
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
