import { fuzzyFilter } from '@shared/fuzzy'
import { useCallback, useEffect, useRef, useState } from 'react'

export interface PaletteItem {
  id: string
  label: string
  /** Muted text on the right, such as a "remote" marker. */
  detail?: string
}

/** The prompts a command can show. Both resolve to null when the user dismisses the palette. */
export interface PaletteApi {
  pick(placeholder: string, items: PaletteItem[]): Promise<string | null>
  input(placeholder: string): Promise<string | null>
}

type Prompt = { key: number; placeholder: string; resolve: (value: string | null) => void } & (
  | { kind: 'pick'; items: PaletteItem[] }
  | { kind: 'input' }
)

let nextKey = 1

/** State for the one palette overlay: commands call `pick` and `input`; render `element`. */
export function usePalette(): { api: PaletteApi; element: React.JSX.Element | null } {
  const [prompt, setPrompt] = useState<Prompt | null>(null)
  const current = useRef<Prompt | null>(null)

  const show = useCallback((next: Prompt): void => {
    // A prompt replaced by a newer one counts as dismissed, so no command waits forever
    current.current?.resolve(null)
    current.current = next
    setPrompt(next)
  }, [])

  const api: PaletteApi = {
    pick: (placeholder, items) =>
      new Promise((resolve) => show({ key: nextKey++, kind: 'pick', placeholder, items, resolve })),
    input: (placeholder) =>
      new Promise((resolve) => show({ key: nextKey++, kind: 'input', placeholder, resolve }))
  }
  const finish = (value: string | null): void => {
    const done = current.current
    current.current = null
    setPrompt(null)
    done?.resolve(value)
  }

  return {
    api,
    element: prompt && <PalettePrompt key={prompt.key} prompt={prompt} onFinish={finish} />
  }
}

function PalettePrompt({
  prompt,
  onFinish
}: {
  prompt: Prompt
  onFinish: (value: string | null) => void
}): React.JSX.Element {
  const [query, setQuery] = useState('')
  const [index, setIndex] = useState(0)
  const items = prompt.kind === 'pick' ? fuzzyFilter(prompt.items, query, (i) => i.label) : []
  const selected = Math.min(index, Math.max(items.length - 1, 0))
  const listRef = useRef<HTMLUListElement>(null)

  useEffect(() => {
    listRef.current?.children[selected]?.scrollIntoView({ block: 'nearest' })
  }, [selected])

  const onKeyDown = (e: React.KeyboardEvent): void => {
    if (e.key === 'Escape') {
      e.preventDefault()
      onFinish(null)
    } else if (e.key === 'Enter') {
      e.preventDefault()
      if (prompt.kind === 'input') onFinish(query)
      else if (items[selected]) onFinish(items[selected].id)
    } else if (prompt.kind === 'pick' && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
      e.preventDefault()
      const step = e.key === 'ArrowDown' ? 1 : -1
      setIndex(items.length === 0 ? 0 : (selected + step + items.length) % items.length)
    }
  }

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the backdrop only dismisses on click
    <div className="palette-backdrop" onMouseDown={() => onFinish(null)}>
      <div
        className="palette"
        role="dialog"
        aria-label={prompt.placeholder}
        onMouseDown={(e) => e.stopPropagation()}
      >
        <input
          // biome-ignore lint/a11y/noAutofocus: the palette exists to be typed into
          autoFocus
          className="palette-input"
          placeholder={prompt.placeholder}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value)
            setIndex(0)
          }}
          onKeyDown={onKeyDown}
        />
        {prompt.kind === 'pick' && (
          <ul className="palette-list" ref={listRef}>
            {items.length === 0 && <li className="palette-empty">No matching items</li>}
            {items.map((item, i) => (
              <li key={item.id}>
                <button
                  type="button"
                  tabIndex={-1}
                  className={i === selected ? 'selected' : ''}
                  onMouseMove={() => setIndex(i)}
                  onClick={() => onFinish(item.id)}
                >
                  <span>{item.label}</span>
                  {item.detail && <span className="muted">{item.detail}</span>}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  )
}
