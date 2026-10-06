import { describe, expect, it, vi } from 'vitest'
import { CANCELLED_WHILE_BUSY, confirmIfBusy } from './busyGuard'

describe('confirmIfBusy', () => {
  it('does not ask when the agent is idle', async () => {
    const ask = vi.fn()
    await confirmIfBusy(false, ask)
    expect(ask).not.toHaveBeenCalled()
  })

  it('lets the action proceed when the user confirms', async () => {
    await expect(confirmIfBusy(true, async () => true)).resolves.toBeUndefined()
  })

  it('throws when the user declines', async () => {
    await expect(confirmIfBusy(true, async () => false)).rejects.toThrow(CANCELLED_WHILE_BUSY)
  })
})
