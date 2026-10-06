import { describe, expect, it, vi } from 'vitest'
import { sendReviewRound } from './reviewSend'

const steps = (deliver: () => Promise<string | null>) => {
  const order: string[] = []
  return {
    order,
    steps: {
      snapshot: async () => {
        order.push('snapshot')
        return 'tree1'
      },
      deliver: async () => {
        order.push('deliver')
        return deliver()
      },
      markSent: vi.fn((baseline: string | null) => {
        order.push(`markSent:${baseline}`)
      })
    }
  }
}

describe('sendReviewRound', () => {
  it('snapshots, delivers, then marks the comments sent with the baseline', async () => {
    const { order, steps: s } = steps(async () => null)
    expect(await sendReviewRound(s)).toBeNull()
    expect(order).toEqual(['snapshot', 'deliver', 'markSent:tree1'])
  })

  it('leaves the comments pending when delivery fails', async () => {
    const { steps: s } = steps(async () => 'The agent is not running.')
    expect(await sendReviewRound(s)).toBe('The agent is not running.')
    expect(s.markSent).not.toHaveBeenCalled()
  })
})
