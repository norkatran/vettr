import { describe, expect, it } from 'vitest'
import { validateProfileName } from './profiles'

const existing = [{ id: 'a', name: 'Work' }]

describe('validateProfileName', () => {
  it('trims', () => expect(validateProfileName('  Home ', existing)).toEqual({ name: 'Home' }))
  it('rejects empty names', () =>
    expect(validateProfileName('  ', existing)).toHaveProperty('error'))
  it('rejects long names', () =>
    expect(validateProfileName('x'.repeat(41), existing)).toHaveProperty('error'))
  it('rejects duplicates ignoring case', () =>
    expect(validateProfileName('work', existing)).toHaveProperty('error'))
  it('lets a profile keep its own name', () =>
    expect(validateProfileName('Work', existing, 'a')).toEqual({ name: 'Work' }))
})
