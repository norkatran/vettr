import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { parseResolved, withResolved } from '@shared/resolution'

const FILE = 'resolved-comments.json'

/** The ids of the comments the user resolved in a project, stored in its data dir. */
export function readResolved(dataDir: string): string[] {
  try {
    return parseResolved(JSON.parse(readFileSync(join(dataDir, FILE), 'utf8')))
  } catch {
    return []
  }
}

/** Mark a comment resolved or reopened; resolves to the ids now resolved (unchanged on failure). */
export function setResolved(dataDir: string, id: string, resolved: boolean): string[] {
  const before = readResolved(dataDir)
  const after = withResolved(before, id, resolved)
  if (after === before) return before
  try {
    mkdirSync(dataDir, { recursive: true })
    writeFileSync(join(dataDir, FILE), JSON.stringify(after, null, 2))
    return after
  } catch (err) {
    console.error('Failed to save resolved comments', err)
    return before
  }
}
