import { describe, expect, it } from 'vitest'
import {
  type Branch,
  deletableBranches,
  invalidBranchName,
  mergeTargets,
  parseBranches,
  planGitAction,
  switchName,
  switchTargets
} from './gitActions'

describe('invalidBranchName', () => {
  it('rejects blank names and names that look like options', () => {
    expect(invalidBranchName('  ')).toMatch(/Enter/)
    expect(invalidBranchName('-x')).toMatch(/cannot start/)
    expect(invalidBranchName('feature/x')).toBeNull()
  })
})

describe('planGitAction', () => {
  it('plans each simple action', () => {
    expect(planGitAction({ kind: 'fetch' }).steps).toEqual([['fetch', '--prune']])
    expect(planGitAction({ kind: 'pull' }).steps).toEqual([['pull']])
    expect(planGitAction({ kind: 'stageAll' }).steps).toEqual([['add', '--all']])
    expect(planGitAction({ kind: 'unstageAll' }).steps).toEqual([['reset', '-q']])
    expect(planGitAction({ kind: 'discardAll' }).steps).toEqual([
      ['reset', '-q', '--hard'],
      ['clean', '-fdq']
    ])
    expect(planGitAction({ kind: 'stash' }).steps).toEqual([
      ['stash', 'push', '--include-untracked']
    ])
    expect(planGitAction({ kind: 'stashPop' }).steps).toEqual([['stash', 'pop']])
  })

  it('plans branch actions', () => {
    expect(planGitAction({ kind: 'createBranch', name: 'a' }).steps).toEqual([
      ['switch', '-c', 'a']
    ])
    expect(planGitAction({ kind: 'checkout', name: 'a' }).steps).toEqual([['switch', 'a']])
    expect(planGitAction({ kind: 'deleteBranch', name: 'a' }).steps).toEqual([
      ['branch', '-d', 'a']
    ])
  })

  it('aborts a failed merge or rebase', () => {
    const merge = planGitAction({ kind: 'merge', name: 'main' })
    expect(merge.steps).toEqual([['merge', '--no-edit', 'main']])
    expect(merge.abortOnFailure).toEqual(['merge', '--abort'])
    expect(merge.abortNote).toMatch(/merge was aborted/)
    const rebase = planGitAction({ kind: 'rebase', name: 'main' })
    expect(rebase.steps).toEqual([['rebase', 'main']])
    expect(rebase.abortOnFailure).toEqual(['rebase', '--abort'])
    expect(rebase.abortNote).toMatch(/rebase was aborted/)
  })
})

const listing = [
  '*refs/heads/main',
  ' refs/heads/dev',
  ' refs/remotes/origin/HEAD',
  ' refs/remotes/origin/main',
  ' refs/remotes/origin/feature/x',
  ''
].join('\n')

describe('parseBranches', () => {
  it('parses local then remote branches, skipping HEAD pointers and blanks', () => {
    expect(parseBranches(listing)).toEqual([
      { ref: 'main', remote: false, current: true },
      { ref: 'dev', remote: false, current: false },
      { ref: 'origin/main', remote: true, current: false },
      { ref: 'origin/feature/x', remote: true, current: false }
    ])
  })

  it('ignores unrelated refs', () => {
    expect(parseBranches(' refs/tags/v1')).toEqual([])
  })
})

describe('branch selection', () => {
  const branches: Branch[] = parseBranches(listing)

  it('switchName drops the remote prefix', () => {
    expect(switchName({ ref: 'origin/feature/x', remote: true, current: false })).toBe('feature/x')
    expect(switchName({ ref: 'dev', remote: false, current: false })).toBe('dev')
  })

  it('switchTargets excludes the current branch and remote copies of local ones', () => {
    expect(switchTargets(branches).map((b) => b.ref)).toEqual(['dev', 'origin/feature/x'])
  })

  it('deletableBranches are local and not current', () => {
    expect(deletableBranches(branches).map((b) => b.ref)).toEqual(['dev'])
  })

  it('mergeTargets are everything but the current branch', () => {
    expect(mergeTargets(branches).map((b) => b.ref)).toEqual([
      'dev',
      'origin/main',
      'origin/feature/x'
    ])
  })
})
