import { endsAt, inRange, type ReviewComment, type Side } from '@shared/comments'
import { createContext, useContext, useState } from 'react'

/** The lines being selected for a new comment. */
export interface Draft {
  file: string
  staged: boolean
  side: Side
  /** The line first clicked; shift-click extends the range from here. */
  anchor: number
  start: number
  end: number
}

export interface CommentUi {
  comments: ReviewComment[]
  draft: Draft | null
  /** A line number was clicked (shift extends the current selection). */
  pick(file: string, staged: boolean, side: Side, no: number, shift: boolean): void
  save(text: string): void
  cancel(): void
  edit(id: number, text: string): void
  remove(id: number): void
}

export const CommentContext = createContext<CommentUi | null>(null)

export const useCommentUi = (): CommentUi => {
  const ui = useContext(CommentContext)
  if (!ui) throw new Error('CommentContext is missing')
  return ui
}

/** Whether `no` on this side of this file diff is inside the current draft selection. */
export const isSelected = (
  draft: Draft | null,
  file: string,
  staged: boolean,
  side: Side,
  no: number | null
): boolean =>
  !!draft &&
  no !== null &&
  draft.file === file &&
  draft.staged === staged &&
  draft.side === side &&
  inRange(draft, no)

function Editor({
  initial,
  label,
  onSave,
  onCancel
}: {
  initial: string
  label: string
  onSave: (text: string) => void
  onCancel: () => void
}): React.JSX.Element {
  const [text, setText] = useState(initial)
  const submit = (): void => {
    if (text.trim()) onSave(text.trim())
  }
  return (
    <div className="comment-editor">
      <textarea
        // biome-ignore lint/a11y/noAutofocus: the editor opens because the user asked for it
        autoFocus
        aria-label={label}
        placeholder="Leave a comment (Ctrl+Enter to save, Esc to cancel)"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
            e.preventDefault()
            submit()
          } else if (e.key === 'Escape') onCancel()
        }}
      />
      <div className="comment-actions">
        <button type="button" onClick={onCancel}>
          Cancel
        </button>
        <button type="button" disabled={!text.trim()} onClick={submit}>
          Save
        </button>
      </div>
    </div>
  )
}

function CommentCard({ comment }: { comment: ReviewComment }): React.JSX.Element {
  const ui = useCommentUi()
  const [editing, setEditing] = useState(false)
  const range =
    comment.start === comment.end
      ? `line ${comment.start}`
      : `lines ${comment.start}-${comment.end}`
  if (editing) {
    return (
      <Editor
        initial={comment.text}
        label="Edit comment"
        onSave={(text) => {
          ui.edit(comment.id, text)
          setEditing(false)
        }}
        onCancel={() => setEditing(false)}
      />
    )
  }
  return (
    <div className={comment.sent ? 'comment sent' : 'comment'}>
      <div className="comment-meta">
        <span>
          {comment.side === 'old' ? 'Old' : 'New'} {range}
          {comment.sent && ` · sent in round ${comment.round}`}
        </span>
        {!comment.sent && (
          <span className="comment-actions">
            <button type="button" onClick={() => setEditing(true)}>
              Edit
            </button>
            <button type="button" onClick={() => ui.remove(comment.id)}>
              Delete
            </button>
          </span>
        )}
      </div>
      <p>{comment.text}</p>
    </div>
  )
}

/** The editor for the current draft and the comments that end at one line, drawn under it. */
export function LineComments({
  file,
  staged,
  side,
  no,
  colSpan
}: {
  file: string
  staged: boolean
  side: Side
  no: number | null
  colSpan: number
}): React.JSX.Element | null {
  const ui = useCommentUi()
  if (no === null) return null
  const here = ui.comments.filter((c) => endsAt(c, file, staged, side, no))
  const draft =
    ui.draft &&
    ui.draft.file === file &&
    ui.draft.staged === staged &&
    ui.draft.side === side &&
    ui.draft.end === no
      ? ui.draft
      : null
  if (here.length === 0 && !draft) return null
  return (
    <tr className="comment-row">
      <td colSpan={colSpan}>
        {here.map((c) => (
          <CommentCard key={c.id} comment={c} />
        ))}
        {draft && <Editor initial="" label="New comment" onSave={ui.save} onCancel={ui.cancel} />}
      </td>
    </tr>
  )
}
