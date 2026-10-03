import { createHash } from 'node:crypto'
import { basename, join } from 'node:path'

/**
 * Where the Claude config dir (and so the SDK's session transcripts) for a project lives on the
 * host: `<userData>/projects/<name>-<hash>/transcripts`. The hash of the full path keeps projects
 * with the same folder name apart; moving a project makes it a new one.
 */
export function transcriptsDir(userData: string, project: string): string {
  const hash = createHash('sha256').update(project).digest('hex').slice(0, 12)
  const name = basename(project).replace(/[^\w.-]+/g, '_') || 'project'
  return join(userData, 'projects', `${name}-${hash}`, 'transcripts')
}
