import { relative, sep } from 'node:path'
import chokidar from 'chokidar'

const DEBOUNCE_MS = 250

/**
 * Whether the watcher should skip `path`. Dependencies are noise, and inside `.git` only
 * `HEAD` and `index` matter: they change when the user stages, commits or switches branch,
 * which changes what the diff against `HEAD` shows.
 */
export function isIgnored(root: string, path: string): boolean {
  const parts = relative(root, path).split(sep)
  if (parts.includes('node_modules')) return true
  if (parts[0] !== '.git' || parts.length === 1) return false
  return !(parts.length === 2 && (parts[1] === 'HEAD' || parts[1] === 'index'))
}

/**
 * Call `onChange` (debounced, so a burst of edits fires once) whenever something in the
 * working tree at `root` changes. Resolves once the watcher is ready, with a function that
 * stops it.
 */
export async function watchTree(
  root: string,
  onChange: () => void,
  debounceMs = DEBOUNCE_MS
): Promise<() => Promise<void>> {
  let timer: NodeJS.Timeout | undefined
  const watcher = chokidar.watch(root, {
    ignored: (path) => isIgnored(root, path),
    ignoreInitial: true
  })
  watcher.on('all', () => {
    clearTimeout(timer)
    timer = setTimeout(onChange, debounceMs)
  })
  // Errors (for example hitting the inotify watch limit) must not crash the main process
  watcher.on('error', () => undefined)
  await new Promise<void>((resolve) => watcher.once('ready', () => resolve()))
  return async () => {
    clearTimeout(timer)
    await watcher.close()
  }
}
