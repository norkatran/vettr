import { useState } from 'react'

/**
 * Entry for an Anthropic API key or Claude OAuth token. The value is shown as plain text while it
 * is typed, so the user can check it before committing, and is cleared from the input as soon as
 * it is saved: a saved key is never displayed again.
 */
export function ApiKeyForm({
  onSave,
  onCancel,
  explanation,
  saveLabel = 'Save'
}: {
  onSave: (key: string) => Promise<string | null>
  onCancel?: () => void
  explanation?: string
  saveLabel?: string
}): React.JSX.Element {
  const [key, setKey] = useState('')
  const [error, setError] = useState<string | null>(null)
  const [saving, setSaving] = useState(false)
  return (
    <form
      className="key-form"
      onSubmit={(e) => {
        e.preventDefault()
        setSaving(true)
        void onSave(key.trim()).then((message) => {
          setError(message)
          setSaving(false)
          if (!message) setKey('')
        })
      }}
    >
      {explanation && <p className="hint">{explanation}</p>}
      <label htmlFor="api-key">Anthropic API key or Claude OAuth token</label>
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
          disabled={saving || key.trim() === ''}
          title="Saving a key restarts the agent, which ends any session in progress"
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
