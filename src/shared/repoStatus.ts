/** A snapshot of the repository state shown in the status bar. */
export interface RepoStatus {
  /** Current branch, or null when HEAD is detached. */
  branch: string | null
  /** Abbreviated commit SHA of HEAD, or null before the first commit. */
  sha: string | null
  /** Upstream branch (for example `origin/main`), or null when none is configured. */
  upstream: string | null
  /** Commits ahead of and behind the upstream; both 0 when there is no upstream. */
  ahead: number
  behind: number
  /** Number of changed paths, including untracked files. */
  changes: number
}

const SHA_LENGTH = 7

/** Parse the output of `git status --porcelain=v2 --branch`. */
export function parseRepoStatus(output: string): RepoStatus {
  const status: RepoStatus = {
    branch: null,
    sha: null,
    upstream: null,
    ahead: 0,
    behind: 0,
    changes: 0
  }
  for (const line of output.split('\n')) {
    if (line.startsWith('# branch.oid ')) {
      const oid = line.slice('# branch.oid '.length)
      status.sha = oid === '(initial)' ? null : oid.slice(0, SHA_LENGTH)
    } else if (line.startsWith('# branch.head ')) {
      const head = line.slice('# branch.head '.length)
      status.branch = head === '(detached)' ? null : head
    } else if (line.startsWith('# branch.upstream ')) {
      status.upstream = line.slice('# branch.upstream '.length)
    } else if (line.startsWith('# branch.ab ')) {
      const match = /^# branch\.ab \+(\d+) -(\d+)$/.exec(line)
      if (match) {
        status.ahead = Number(match[1])
        status.behind = Number(match[2])
      }
    } else if (/^[12u?] /.test(line)) {
      status.changes++
    }
  }
  return status
}
