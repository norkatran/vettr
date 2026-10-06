import { createHash } from 'node:crypto'
import { basename, join } from 'node:path'

/**
 * Where the app keeps its own data for a project on the host: `<userData>/projects/<name>-<hash>`.
 * The hash of the full path keeps projects with the same folder name apart; moving a project makes
 * it a new one.
 */
export function projectDataDir(userData: string, project: string): string {
  const hash = createHash('sha256').update(project).digest('hex').slice(0, 12)
  const name = basename(project).replace(/[^\w.-]+/g, '_') || 'project'
  return join(userData, 'projects', `${name}-${hash}`)
}

/** The project's Claude config dir (and so the SDK's session transcripts) on the host. */
export function transcriptsDir(userData: string, project: string): string {
  return join(projectDataDir(userData, project), 'transcripts')
}
