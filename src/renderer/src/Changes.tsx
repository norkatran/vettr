import { rangeOf, type Side, snapshotLines } from '@shared/comments'
import { changedFileCount, type DiffLine, type FileChange, splitRows } from '@shared/diff'
import { Fragment, useState } from 'react'
import {
  CommentContext,
  type CommentUi,
  type Draft,
  isSelected,
  LineComments,
  OutdatedComments,
  useCommentUi
} from './Comments'
import { useNotify } from './Notifications'
import type { ChangesState } from './useChanges'
import type { Review } from './useReviewComments'

export type DiffMode = 'unified' | 'split'

export const fileAnchor = (index: number): string => `change-file-${index}`

interface ChangesProps {
  project: string | null
  changes: ChangesState
  review: Review
  /** Send the unsent comments to the agent; null when that is not possible, with the reason. */
  send: { pending: number; run: () => void; blocked: string | null }
}

export function Changes({ project, changes, review, send }: ChangesProps): React.JSX.Element {
  const [mode, setMode] = useState<DiffMode>('unified')
  const notify = useNotify()
  const [draft, setDraft] = useState<Draft | null>(null)
  const { files, loading } = changes
  const count = changes.changes ? changedFileCount(changes.changes) : 0

  if (!project) return <Empty>Open a project to see its changes.</Empty>
  if (loading && !files) return <Empty>Loading changes...</Empty>
  if (!files || !changes.changes) return <Empty>Could not read the changes for this project.</Empty>
  if (files.length === 0) return <Empty>No changes.</Empty>

  const { staged, unstaged } = changes.changes
  const move = async (to: 'stage' | 'unstage', moved: FileChange[]): Promise<void> => {
    const paths = moved.flatMap(filePaths)
    const failure = await (to === 'stage'
      ? window.agentide.stageFiles(project, paths)
      : window.agentide.unstageFiles(project, paths))
    if (failure) notify(to === 'stage' ? 'Stage failed' : 'Unstage failed', failure)
  }

  const ui: CommentUi = {
    comments: review.comments,
    draft,
    pick: (file, isStaged, side, no, shift) =>
      setDraft((prev) => {
        const extend =
          shift && prev && prev.file === file && prev.staged === isStaged && prev.side === side
        const anchor = extend ? prev.anchor : no
        const [start, end] = rangeOf(anchor, no)
        return { file, staged: isStaged, side, anchor, start, end }
      }),
    save: (text) => {
      const target = draft && (draft.staged ? staged : unstaged).find((f) => f.path === draft.file)
      if (draft && target) {
        review.add({
          file: draft.file,
          staged: draft.staged,
          side: draft.side,
          start: draft.start,
          end: draft.end,
          snapshot: snapshotLines(target, draft.side, draft.start, draft.end),
          text
        })
      }
      setDraft(null)
    },
    cancel: () => setDraft(null),
    edit: review.edit,
    remove: review.remove
  }

  return (
    <CommentContext.Provider value={ui}>
      <main className="changes">
        <div className="changes-toolbar">
          <span>
            {count} changed {count === 1 ? 'file' : 'files'}
          </span>
          <div className="segmented">
            {(['unified', 'split'] as const).map((m) => (
              <button
                key={m}
                type="button"
                className={m === mode ? 'selected' : ''}
                aria-pressed={m === mode}
                onClick={() => setMode(m)}
              >
                {m === 'unified' ? 'Unified' : 'Split'}
              </button>
            ))}
          </div>
          <button
            type="button"
            className="send-review"
            disabled={send.pending === 0 || send.blocked !== null}
            title={send.blocked ?? 'Send the unsent comments to the agent'}
            onClick={send.run}
          >
            Send {send.pending} {send.pending === 1 ? 'comment' : 'comments'} to agent
          </button>
        </div>
        <OutdatedComments />
        <Group
          title="Staged changes"
          action="Unstage"
          files={staged}
          firstIndex={0}
          mode={mode}
          onMove={(moved) => move('unstage', moved)}
        />
        <Group
          title="Unstaged changes"
          action="Stage"
          files={unstaged}
          firstIndex={staged.length}
          mode={mode}
          onMove={(moved) => move('stage', moved)}
        />
      </main>
    </CommentContext.Provider>
  )
}

/** Every path a file change touches in the index: a rename covers its old and new path. */
export const filePaths = (file: FileChange): string[] =>
  file.oldPath ? [file.oldPath, file.path] : [file.path]

interface GroupProps {
  title: string
  /** The verb on the buttons: moves the file to the other group. */
  action: 'Stage' | 'Unstage'
  files: FileChange[]
  /** Index of the first file in the combined staged-then-unstaged list, for scroll anchors. */
  firstIndex: number
  mode: DiffMode
  onMove: (files: FileChange[]) => Promise<void>
}

function Group({ title, action, files, firstIndex, mode, onMove }: GroupProps): React.JSX.Element {
  return (
    <details className="change-group" open>
      <summary>
        <span>
          {title} ({files.length})
        </span>
        {files.length > 0 && (
          <button
            type="button"
            className="file-action"
            onClick={(e) => {
              e.preventDefault()
              void onMove(files)
            }}
          >
            {action} all
          </button>
        )}
      </summary>
      {files.length === 0 && <p className="diff-note">Nothing here.</p>}
      {files.map((file, i) => (
        <FileDiff
          key={`${file.oldPath ?? ''}>${file.path}`}
          id={fileAnchor(firstIndex + i)}
          file={file}
          mode={mode}
          staged={action === 'Unstage'}
          action={action}
          onMove={() => onMove([file])}
        />
      ))}
    </details>
  )
}

function Empty({ children }: { children: React.ReactNode }): React.JSX.Element {
  return (
    <main className="placeholder">
      <p className="hint">{children}</p>
    </main>
  )
}

export function FileTitle({ file }: { file: FileChange }): React.JSX.Element {
  return (
    <>
      {file.oldPath && <span className="old-path">{file.oldPath} → </span>}
      {file.path}
    </>
  )
}

function FileDiff({
  id,
  file,
  mode,
  staged,
  action,
  onMove
}: {
  id: string
  file: FileChange
  mode: DiffMode
  staged: boolean
  action: 'Stage' | 'Unstage'
  onMove: () => void
}): React.JSX.Element {
  return (
    <section className="file-diff" id={id}>
      <header>
        <span className={`badge ${file.status}`}>{file.status}</span>
        <span className="file-title">
          <FileTitle file={file} />
        </span>
        <span className="counts">
          <span className="add">+{file.additions}</span>{' '}
          <span className="del">-{file.deletions}</span>
        </span>
        <button type="button" className="file-action" onClick={onMove}>
          {action}
        </button>
      </header>
      {file.binary ? (
        <p className="diff-note">Binary file not shown.</p>
      ) : file.tooLarge ? (
        <p className="diff-note">
          Diff too large to display ({file.additions + file.deletions} changed lines).
        </p>
      ) : file.hunks.length === 0 ? (
        <p className="diff-note">No content changes.</p>
      ) : (
        <table className={`diff ${mode}`}>
          <tbody>
            {file.hunks.map((hunk) => (
              <HunkRows
                key={hunk.header + hunk.lines[0]?.text}
                header={hunk.header}
                lines={hunk.lines}
                mode={mode}
                file={file.path}
                staged={staged}
              />
            ))}
          </tbody>
        </table>
      )}
    </section>
  )
}

function HunkRows({
  header,
  lines,
  mode,
  file,
  staged
}: {
  header: string
  lines: DiffLine[]
  mode: DiffMode
  file: string
  staged: boolean
}): React.JSX.Element {
  const split = mode === 'split'
  return (
    <>
      <tr className="hunk-header">
        <td colSpan={split ? 4 : 3}>{header}</td>
      </tr>
      {split
        ? splitRows(lines).map((row) => (
            <Fragment key={`${row.left?.oldNo}:${row.right?.newNo}`}>
              <tr>
                <SplitCells line={row.left} side="old" file={file} staged={staged} />
                <SplitCells line={row.right} side="new" file={file} staged={staged} />
              </tr>
              <LineComments
                file={file}
                staged={staged}
                side="old"
                no={row.left?.oldNo ?? null}
                colSpan={4}
              />
              <LineComments
                file={file}
                staged={staged}
                side="new"
                no={row.right?.newNo ?? null}
                colSpan={4}
              />
            </Fragment>
          ))
        : lines.map((line) => (
            <Fragment key={`${line.oldNo}:${line.newNo}`}>
              <tr className={line.kind}>
                <NumberCell line={line} side="old" file={file} staged={staged} />
                <NumberCell line={line} side="new" file={file} staged={staged} />
                <td className="code">
                  <span className="sign">{SIGN[line.kind]}</span>
                  {line.text}
                </td>
              </tr>
              <LineComments file={file} staged={staged} side="old" no={line.oldNo} colSpan={3} />
              <LineComments file={file} staged={staged} side="new" no={line.newNo} colSpan={3} />
            </Fragment>
          ))}
    </>
  )
}

const SIGN = { context: ' ', add: '+', del: '-' } as const

interface CellProps {
  line: DiffLine
  side: Side
  file: string
  staged: boolean
}

/** A line number cell; clicking it comments on the line, shift-click extends to a range. */
function NumberCell({
  line,
  side,
  file,
  staged,
  className = ''
}: CellProps & { className?: string }) {
  const ui = useCommentUi()
  const no = side === 'old' ? line.oldNo : line.newNo
  if (no === null) return <td className="num" />
  const selected = isSelected(ui.draft, file, staged, side, no)
  return (
    <td className={`num ${className} ${selected ? 'selected' : ''}`}>
      <button
        type="button"
        className="line-number"
        title="Click to comment (shift-click for a range)"
        aria-label={`Comment on ${side} line ${no}`}
        onClick={(e) => ui.pick(file, staged, side, no, e.shiftKey)}
      >
        {no}
      </button>
    </td>
  )
}

function SplitCells({
  line,
  side,
  file,
  staged
}: {
  line: DiffLine | null
  side: Side
  file: string
  staged: boolean
}) {
  if (!line) {
    return (
      <>
        <td className="num empty" />
        <td className="code empty" />
      </>
    )
  }
  return (
    <>
      <NumberCell line={line} side={side} file={file} staged={staged} className={line.kind} />
      <td className={`code ${line.kind}`}>{line.text}</td>
    </>
  )
}
