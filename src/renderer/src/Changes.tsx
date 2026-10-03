import { type DiffLine, type FileChange, splitRows } from '@shared/diff'
import { useState } from 'react'
import type { ChangesState } from './useChanges'

export type DiffMode = 'unified' | 'split'

export const fileAnchor = (index: number): string => `change-file-${index}`

interface ChangesProps {
  project: string | null
  changes: ChangesState
}

export function Changes({ project, changes }: ChangesProps): React.JSX.Element {
  const [mode, setMode] = useState<DiffMode>('unified')
  const { files, loading } = changes

  if (!project) return <Empty>Open a project to see its changes.</Empty>
  if (loading && !files) return <Empty>Loading changes...</Empty>
  if (!files) return <Empty>Could not read the changes for this project.</Empty>
  if (files.length === 0) return <Empty>No changes against HEAD.</Empty>

  return (
    <main className="changes">
      <div className="changes-toolbar">
        <span>
          {files.length} changed {files.length === 1 ? 'file' : 'files'}
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
      </div>
      {files.map((file, i) => (
        <FileDiff
          key={`${file.oldPath ?? ''}>${file.path}`}
          id={fileAnchor(i)}
          file={file}
          mode={mode}
        />
      ))}
    </main>
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
  mode
}: {
  id: string
  file: FileChange
  mode: DiffMode
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
  mode
}: {
  header: string
  lines: DiffLine[]
  mode: DiffMode
}): React.JSX.Element {
  return (
    <>
      <tr className="hunk-header">
        <td colSpan={mode === 'split' ? 4 : 3}>{header}</td>
      </tr>
      {mode === 'unified'
        ? lines.map((line) => (
            <tr key={`${line.oldNo}:${line.newNo}`} className={line.kind}>
              <td className="num">{line.oldNo}</td>
              <td className="num">{line.newNo}</td>
              <td className="code">
                <span className="sign">{SIGN[line.kind]}</span>
                {line.text}
              </td>
            </tr>
          ))
        : splitRows(lines).map((row) => (
            <tr key={`${row.left?.oldNo}:${row.right?.newNo}`}>
              <SplitCells line={row.left} side="old" />
              <SplitCells line={row.right} side="new" />
            </tr>
          ))}
    </>
  )
}

const SIGN = { context: ' ', add: '+', del: '-' } as const

function SplitCells({ line, side }: { line: DiffLine | null; side: 'old' | 'new' }) {
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
      <td className={`num ${line.kind}`}>{side === 'old' ? line.oldNo : line.newNo}</td>
      <td className={`code ${line.kind}`}>{line.text}</td>
    </>
  )
}
