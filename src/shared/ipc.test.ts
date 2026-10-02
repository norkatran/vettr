import { describe, expect, it } from 'vitest'
import { IpcChannel } from './ipc'

describe('IpcChannel', () => {
  it('has a unique string name for every channel', () => {
    const names = Object.values(IpcChannel)
    expect(new Set(names).size).toBe(names.length)
    for (const name of names) expect(name).toMatch(/^[a-z]+:[a-z]+$/)
  })
})
