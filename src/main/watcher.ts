import { relative, sep } from 'node:path'
import chokidar from 'chokidar'
import { listIgnored } from './git'

const DEBOUNCE_MS = 250

/** Errors that mean the system is out of watch handles: keep going and it only gets worse. */
const EXHAUSTED = new Set(['EMFILE', 'ENFILE', 'ENOSPC'])

/**
 * Whether the watcher should skip `path`. `ignored` holds the paths (relative to `root`, with
 * `/` separators) that git ignores; a path is skipped when it or any parent is listed. Nothing
 * is skipped by name, so a project that tracks `node_modules` gets it watched. Inside `.git`
 * only `HEAD` and `index` matter: they change when the user stages, commits or switches
 * branch, which changes what the diff against `HEAD` shows.
 */
export function isIgnored(root: string, path: string, ignored: ReadonlySet<string> = new Set()) {
  const parts = relative(root, path).split(sep)
  if (parts[0] === '.git') return parts.length > 1 && !(parts.length === 2 && isGitState(parts[1]))
  let prefix = ''
  for (const part of parts) {
    prefix = prefix ? `${prefix}/${part}` : part
    if (ignored.has(prefix)) return true
  }
  return false
}

const isGitState = (name: string | undefined): boolean => name === 'HEAD' || name === 'index'

/**
 * Call `onChange` (debounced, so a burst of edits fires once) whenever something in the
 * working tree at `root` changes, leaving out what git ignores. Resolves once the watcher is
 * ready, with a function that stops it.
 */
export async function watchTree(
  root: string,
  onChange: () => void,
  debounceMs = DEBOUNCE_MS
): Promise<() => Promise<void>> {
  const list = await listIgnored(root)
  if (!list) console.error(`vettr: could not list ignored paths in ${root}; watching everything`)
  const ignored = new Set(list ?? [])
  let timer: NodeJS.Timeout | undefined
  const watcher = chokidar.watch(root, {
    ignored: (path) => isIgnored(root, path, ignored),
    ignoreInitial: true
  })
  watcher.on('all', () => {
    clearTimeout(timer)
    timer = setTimeout(onChange, debounceMs)
  })
  // Errors must not crash the main process. Running out of handles is not survivable for
  // the machine, so stop watching; the window-focus reload still refreshes the view.
  watcher.on('error', (error) => {
    console.error('vettr: file watcher error', error)
    const code = (error as NodeJS.ErrnoException).code
    if (code && EXHAUSTED.has(code)) void watcher.close()
  })
  await new Promise<void>((resolve) => watcher.once('ready', () => resolve()))
  return async () => {
    clearTimeout(timer)
    await watcher.close()
  }
}

type StartWatch = typeof watchTree

/**
 * Keeps at most one project watcher alive. Switches run one at a time through a queue, and a
 * watcher that finished starting after the project changed again is closed straight away, so
 * quick project switches cannot leak a full-tree watcher.
 */
export function createProjectWatcher(onChange: () => void, start: StartWatch = watchTree) {
  let stop: (() => Promise<void>) | null = null
  let generation = 0
  let tail: Promise<void> = Promise.resolve()

  const enqueue = (job: (current: number) => Promise<void>): Promise<void> => {
    const current = ++generation
    tail = tail.then(() => job(current)).catch((error) => console.error('vettr: watcher', error))
    return tail
  }

  const halt = async (): Promise<void> => {
    const previous = stop
    stop = null
    await previous?.()
  }

  return {
    /** Watch `path` instead of whatever was watched before. */
    watch: (path: string): Promise<void> =>
      enqueue(async (current) => {
        await halt()
        if (current !== generation) return
        const next = await start(path, onChange)
        if (current === generation) stop = next
        else await next()
      }),
    /** Stop watching (app quit). */
    close: (): Promise<void> => enqueue(halt)
  }
}
