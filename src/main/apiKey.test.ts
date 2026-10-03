import { mkdtemp, readFile, rm, stat } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { createApiKeyStore, type KeyCrypto } from './apiKey'

const reverse = (s: string): string => [...s].reverse().join('')
const crypto = (available = true): KeyCrypto => ({
  isAvailable: () => available,
  encrypt: (plain) => Buffer.from(reverse(plain)),
  decrypt: (encrypted) => reverse(encrypted.toString())
})

let dir = ''
let file = ''
beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), 'agentide-key-'))
  file = join(dir, 'nested', 'apikey')
})
afterEach(() => rm(dir, { recursive: true, force: true }))

describe('createApiKeyStore', () => {
  it('returns null when nothing is saved', async () => {
    expect(await createApiKeyStore(file, crypto()).get()).toBeNull()
  })

  it('saves the key encrypted and reads it back', async () => {
    const store = createApiKeyStore(file, crypto())
    await store.set('sk-ant-secret')
    expect(await store.get()).toBe('sk-ant-secret')
    expect((await readFile(file)).toString()).not.toContain('sk-ant-secret')
    expect((await stat(file)).mode & 0o777).toBe(0o600)
  })

  it('returns null when the saved key cannot be decrypted', async () => {
    await createApiKeyStore(file, crypto()).set('k')
    const broken: KeyCrypto = {
      ...crypto(),
      decrypt: () => {
        throw new Error('keychain changed')
      }
    }
    expect(await createApiKeyStore(file, broken).get()).toBeNull()
  })

  it('refuses to save when secure storage is unavailable', async () => {
    const store = createApiKeyStore(file, crypto(false))
    await expect(store.set('k')).rejects.toThrow('Secure storage')
    expect(await store.get()).toBeNull()
  })

  it('clears the key, and clearing twice is fine', async () => {
    const store = createApiKeyStore(file, crypto())
    await store.set('k')
    await store.clear()
    await store.clear()
    expect(await store.get()).toBeNull()
  })
})
