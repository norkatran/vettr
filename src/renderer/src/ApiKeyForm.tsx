import { MAX_PROFILE_NAME } from '@shared/profiles'
import { useState } from 'react'

/**
 * Entry for a profile: a name and an Anthropic API key or Claude OAuth token. The value is shown
 * as plain text while it is typed, so the user can check it before committing, and is cleared from
 * the input as soon as it is saved: a saved key is never displayed again. When `initialName` is
 * given (editing) the key may be left blank to keep the saved one.
 */
export function ApiKeyForm({
  onSave,
  onCancel,
  explanation,
  saveLabel = 'Save',
  initialName = '',
  keyOptional = false
}: {
  onSave: (name: string, key: string) => Promise<string | null>
  onCancel?: () => void
  explanation?: string
  saveLabel?: string
  initialName?: string
  keyOptional?: boolean
}): React.JSX.Element {
  const [name, setName] = useState(initialName)
  const [key, setKey] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  return (
    <form
      className="key-form"
      onSubmit={(e) => {
        e.preventDefault()
        setSaving(true)
        void onSave(name.trim(), key.trim()).then((message) => {
          setError(message)
          setSaving(false)
          if (!message) {
            setKey('')
            if (!initialName) setName('')
          }
        })
      }}
    >
      {explanation && <p className="hint">{explanation}</p>}
      <label htmlFor="profile-name">Profile name</label>
      <div className="key-row">
        <input
          id="profile-name"
          type="text"
          autoComplete="off"
          maxLength={MAX_PROFILE_NAME}
          placeholder="Work, Personal, ..."
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
      </div>
      <label htmlFor="api-key">
        Anthropic API key or Claude OAuth token
        {keyOptional ? ' (leave blank to keep the saved one)' : ''}
      </label>
      <div className="key-row">
        <input
          id="api-key"
          type="text"
          autoComplete="off"
          autoCorrect="off"
          autoCapitalize="off"
          spellCheck={false}
          placeholder="sk-ant-api03-... or sk-ant-oat01-..."
          value={key}
          onChange={(e) => setKey(e.target.value)}
        />
        <button
          type="submit"
          disabled={saving || name.trim() === '' || (!keyOptional && key.trim() === '')}
          title="Saving a key for the profile in use restarts the agent, which ends any session in progress"
        >
          {saveLabel}
        </button>
        {onCancel && (
          <button type="button" className="secondary" onClick={onCancel}>
            Cancel
          </button>
        )}
      </div>
      <p className="hint">
        Stored encrypted on this computer and sent to the sandbox when the agent starts. Run{' '}
        <code>claude setup-token</code> to get an OAuth token.
      </p>
      {error && <p className="error-text">{error}</p>}
    </form>
  )
}
