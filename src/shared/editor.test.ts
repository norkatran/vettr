import { describe, expect, it } from 'vitest'
import { buildEditorCommand } from './editor'

describe('buildEditorCommand', () => {
  it('substitutes file and line into the arguments', () => {
    expect(buildEditorCommand('code -g {file}:{line}', '/a/b.ts', 7, '/p')).toEqual({
      command: 'code',
      args: ['-g', '/a/b.ts:7']
    })
  })

  it('keeps a path with spaces as one argument and honours quoted words', () => {
    expect(buildEditorCommand('"my editor" --line {line} {file}', '/a b/c.ts', 2, '/p')).toEqual({
      command: 'my editor',
      args: ['--line', '2', '/a b/c.ts']
    })
  })

  it('keeps a lone quote character as a literal word', () => {
    expect(buildEditorCommand('ed "', '/f', 1, '/p')).toEqual({ command: 'ed', args: ['"'] })
  })

  it('substitutes {project}', () => {
    expect(buildEditorCommand('code {project} -g {file}:{line}', '/p/a.ts', 3, '/p')).toEqual({
      command: 'code',
      args: ['/p', '-g', '/p/a.ts:3']
    })
  })

  it('does not interpret $ patterns in the path', () => {
    expect(buildEditorCommand('ed {file}', '/a/$&.ts', 1, '/p')?.args).toEqual(['/a/$&.ts'])
  })

  it('returns null for an empty template', () => {
    expect(buildEditorCommand('   ', '/f', 1, '/p')).toBeNull()
  })
})
