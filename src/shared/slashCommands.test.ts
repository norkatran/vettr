import { describe, expect, it } from 'vitest'
import type { SlashCommandInfo } from './agent'
import { completeCommand, filterCommands, slashQuery } from './slashCommands'

const cmd = (name: string, aliases?: string[]): SlashCommandInfo => ({
  name,
  description: '',
  argumentHint: '',
  ...(aliases ? { aliases } : {})
})

describe('slashQuery', () => {
  it('reads a command being typed', () => {
    expect(slashQuery('/')).toBe('')
    expect(slashQuery('/rev')).toBe('rev')
  })
  it('ignores prose, arguments and multi-line text', () => {
    expect(slashQuery('fix /rev')).toBeNull()
    expect(slashQuery('/review file.ts')).toBeNull()
    expect(slashQuery('/review\nmore')).toBeNull()
    expect(slashQuery('')).toBeNull()
  })
})

describe('filterCommands', () => {
  const all = [cmd('review'), cmd('init'), cmd('preview'), cmd('usage', ['cost'])]
  it('lists everything for an empty query', () => {
    expect(filterCommands(all, '')).toHaveLength(4)
  })
  it('puts prefix matches before substring matches', () => {
    expect(filterCommands(all, 'rev').map((c) => c.name)).toEqual(['review', 'preview'])
  })
  it('matches aliases and ignores case', () => {
    expect(filterCommands(all, 'COS').map((c) => c.name)).toEqual(['usage'])
  })
  it('returns nothing when nothing matches', () => {
    expect(filterCommands(all, 'zzz')).toEqual([])
  })
})

describe('completeCommand', () => {
  it('adds the slash and a space for arguments', () => {
    expect(completeCommand(cmd('init'))).toBe('/init ')
  })
})
