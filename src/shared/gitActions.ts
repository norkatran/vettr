/** Global git commands run from the command palette. Each maps to one or more `git` invocations. */
export type GitAction =
  | { kind: 'fetch' }
  | { kind: 'pull' }
  | { kind: 'stageAll' }
  | { kind: 'unstageAll' }
  | { kind: 'discardAll' }
  | { kind: 'stash' }
  | { kind: 'stashPop' }
  | { kind: 'createBranch'; name: string }
  | { kind: 'checkout'; name: string }
  | { kind: 'deleteBranch'; name: string }
  | { kind: 'merge'; name: string }
  | { kind: 'rebase'; name: string }

/** What to run for an action: commands in order (stopping at the first failure) and a cleanup. */
export interface GitPlan {
  steps: string[][]
  /** Run (ignoring errors) when a step fails, so a half-done merge or rebase is not left behind. */
  abortOnFailure?: string[]
  /** Appended to git's output when the cleanup ran, so the user knows what state the repo is in. */
  abortNote?: string
}

/** Why a branch name cannot be used, or null. A leading `-` would be read as an option by git. */
export function invalidBranchName(name: string): string | null {
  if (name.trim() === '') return 'Enter a branch name.'
  if (name.startsWith('-')) return 'A branch name cannot start with "-".'
  return null
}

/** The git commands for `action`. Names must pass `invalidBranchName` first. */
export function planGitAction(action: GitAction): GitPlan {
  switch (action.kind) {
    case 'fetch':
      return { steps: [['fetch', '--prune']] }
    case 'pull':
      return { steps: [['pull']] }
    case 'stageAll':
      return { steps: [['add', '--all']] }
    case 'unstageAll':
      return { steps: [['reset', '-q']] }
    case 'discardAll':
      // Reset tracked files, then remove untracked ones (ignored files are kept)
      return {
        steps: [
          ['reset', '-q', '--hard'],
          ['clean', '-fdq']
        ]
      }
    case 'stash':
      return { steps: [['stash', 'push', '--include-untracked']] }
    case 'stashPop':
      return { steps: [['stash', 'pop']] }
    case 'createBranch':
      return { steps: [['switch', '-c', action.name]] }
    case 'checkout':
      return { steps: [['switch', action.name]] }
    case 'deleteBranch':
      // `-d` refuses to delete a branch with unmerged work
      return { steps: [['branch', '-d', action.name]] }
    case 'merge':
      return {
        steps: [['merge', '--no-edit', action.name]],
        abortOnFailure: ['merge', '--abort'],
        abortNote: 'The merge was aborted. Resolve conflicts in a terminal with `git merge`.'
      }
    case 'rebase':
      return {
        steps: [['rebase', action.name]],
        abortOnFailure: ['rebase', '--abort'],
        abortNote: 'The rebase was aborted. Resolve conflicts in a terminal with `git rebase`.'
      }
  }
}

/** A branch or remote-tracking ref, as listed by `listBranches`. */
export interface Branch {
  /** `main` for a local branch, `origin/main` for a remote one. */
  ref: string
  remote: boolean
  current: boolean
}

/** Parse `git for-each-ref --format=%(HEAD)%(refname)` output over refs/heads and refs/remotes. */
export function parseBranches(stdout: string): Branch[] {
  const branches: Branch[] = []
  for (const line of stdout.split('\n')) {
    const current = line.startsWith('*')
    const refname = line.slice(1).trim()
    if (refname.startsWith('refs/heads/')) {
      branches.push({ ref: refname.slice('refs/heads/'.length), remote: false, current })
    } else if (refname.startsWith('refs/remotes/')) {
      const ref = refname.slice('refs/remotes/'.length)
      // `origin/HEAD` is a pointer to another branch, not a branch
      if (!ref.endsWith('/HEAD')) branches.push({ ref, remote: true, current: false })
    }
  }
  return branches.sort((a, b) => Number(a.remote) - Number(b.remote))
}

/** The name `git switch` takes for a branch: remote ones drop the remote prefix (it then tracks). */
export function switchName(branch: Branch): string {
  return branch.remote ? branch.ref.slice(branch.ref.indexOf('/') + 1) : branch.ref
}

/** Branches to offer for Change Branch: all but the current one, hiding remote copies of local branches. */
export function switchTargets(branches: Branch[]): Branch[] {
  const local = new Set(branches.filter((b) => !b.remote).map((b) => b.ref))
  return branches.filter((b) => !b.current && (!b.remote || !local.has(switchName(b))))
}

/** Local branches that can be deleted (not the checked-out one). */
export function deletableBranches(branches: Branch[]): Branch[] {
  return branches.filter((b) => !b.remote && !b.current)
}

/** Refs to offer for merge and rebase: everything except the current branch. */
export function mergeTargets(branches: Branch[]): Branch[] {
  return branches.filter((b) => !b.current)
}
