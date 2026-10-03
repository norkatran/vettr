import { describe, expect, it } from 'vitest'
import { parseRepoStatus } from './repoStatus'

describe('parseRepoStatus', () => {
  it('parses a branch with an upstream, divergence and changes', () => {
    const out = [
      '# branch.oid 0123456789abcdef',
      '# branch.head main',
      '# branch.upstream origin/main',
      '# branch.ab +2 -3',
      '1 .M N... 100644 100644 100644 aaa bbb src/a.ts',
      '2 R. N... 100644 100644 100644 aaa bbb R100 new.ts\told.ts',
      'u UU N... 100644 100644 100644 100644 aaa bbb ccc conflict.ts',
      '? untracked.ts',
      '! ignored.log',
      ''
    ].join('\n')
    expect(parseRepoStatus(out)).toEqual({
      branch: 'main',
      sha: '0123456',
      upstream: 'origin/main',
      ahead: 2,
      behind: 3,
      changes: 4
    })
  })

  it('reports no upstream and a clean tree', () => {
    const out = '# branch.oid 0123456789abcdef\n# branch.head topic\n'
    expect(parseRepoStatus(out)).toEqual({
      branch: 'topic',
      sha: '0123456',
      upstream: null,
      ahead: 0,
      behind: 0,
      changes: 0
    })
  })

  it('reports a detached HEAD as no branch', () => {
    const out = '# branch.oid 0123456789abcdef\n# branch.head (detached)\n'
    expect(parseRepoStatus(out)).toMatchObject({ branch: null, sha: '0123456' })
  })

  it('reports no sha before the first commit', () => {
    const out = '# branch.oid (initial)\n# branch.head main\n'
    expect(parseRepoStatus(out)).toMatchObject({ branch: 'main', sha: null })
  })

  it('ignores a malformed ahead/behind line', () => {
    expect(parseRepoStatus('# branch.ab nonsense\n')).toMatchObject({ ahead: 0, behind: 0 })
  })

  it('handles empty output', () => {
    expect(parseRepoStatus('')).toMatchObject({ branch: null, sha: null, changes: 0 })
  })
})
