import { describe, expect, it } from 'vitest'
import { transcriptsDir } from './transcripts'

describe('transcriptsDir', () => {
  it('nests under projects/<name>-<hash>/transcripts', () => {
    expect(transcriptsDir('/data', '/work/my app')).toMatch(
      /^\/data\/projects\/my_app-[0-9a-f]{12}\/transcripts$/
    )
  })

  it('is stable and distinguishes projects with the same folder name', () => {
    expect(transcriptsDir('/d', '/a/p')).toBe(transcriptsDir('/d', '/a/p'))
    expect(transcriptsDir('/d', '/a/p')).not.toBe(transcriptsDir('/d', '/b/p'))
  })
})

describe('transcriptsDir fallback', () => {
  it('falls back to "project" when the path has no folder name', () => {
    expect(transcriptsDir('/data', '/')).toMatch(/^\/data\/projects\/project-[0-9a-f]{12}\//)
  })
})
