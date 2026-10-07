import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import type { KeyCrypto } from './apiKey'
import { createProfileStore, MIGRATED_PROFILE_NAME } from './profileStore'

const reverse = (s: string): string => [...s].reverse().join('')
const crypto: KeyCrypto = {
  isAvailable: () => true,
  encrypt: (plain) => Buffer.from(reverse(plain)),
  decrypt: (encrypted) => reverse(encrypted.toString())
}

let dir = ''
let file = ''
beforeEach(async () => {
  dir = await mkdtemp(join(tmpdir(), 'vettr-profiles-'))
  file = join(dir, 'profiles.json')
})
afterEach(() => rm(dir, { recursive: true, force: true }))

describe('createProfileStore', () => {
  it('starts empty', async () => {
    const store = createProfileStore(file, crypto)
    expect(await store.list()).toEqual([])
    expect(await store.lastUsedId()).toBeNull()
  })

  it('adds profiles, keeping the credential encrypted on disk', async () => {
    const store = createProfileStore(file, crypto)
    const id = await store.add(' Work ', 'sk-ant-secret')
    expect(await store.list()).toEqual([{ id, name: 'Work' }])
    expect(await store.credential(id)).toBe('sk-ant-secret')
    expect(await readFile(file, 'utf8')).not.toContain('sk-ant-secret')
  })

  it('rejects empty, duplicate and unencryptable entries', async () => {
    const store = createProfileStore(file, crypto)
    await store.add('Work', 'k1')
    await expect(store.add('work', 'k2')).rejects.toThrow(/already exists/)
    await expect(store.add('', 'k2')).rejects.toThrow(/name/)
    await expect(store.add('Home', ' ')).rejects.toThrow(/empty/)
    const noCrypto = createProfileStore(join(dir, 'x.json'), {
      ...crypto,
      isAvailable: () => false
    })
    await expect(noCrypto.add('A', 'k')).rejects.toThrow(/Secure storage/)
  })

  it('renames and replaces the credential independently', async () => {
    const store = createProfileStore(file, crypto)
    const id = await store.add('Work', 'k1')
    await store.update(id, { name: 'Job' })
    expect(await store.credential(id)).toBe('k1')
    await store.update(id, { credential: 'k2' })
    expect(await store.credential(id)).toBe('k2')
    expect(await store.list()).toEqual([{ id, name: 'Job' }])
    await expect(store.update('missing', { name: 'x' })).rejects.toThrow(/no longer exists/)
  })

  it('removes profiles and forgets a removed last-used id', async () => {
    const store = createProfileStore(file, crypto)
    const a = await store.add('A', 'k1')
    const b = await store.add('B', 'k2')
    await store.setLastUsed(b)
    expect(await store.lastUsedId()).toBe(b)
    await store.remove(b)
    expect(await store.lastUsedId()).toBeNull()
    expect(await store.credential(b)).toBeNull()
    expect((await store.list()).map((p) => p.id)).toEqual([a])
  })

  it('sees changes made by another instance', async () => {
    const one = createProfileStore(file, crypto)
    const two = createProfileStore(file, crypto)
    const id = await one.add('A', 'k1')
    expect(await two.credential(id)).toBe('k1')
    await two.add('B', 'k2')
    expect(await one.list()).toHaveLength(2)
  })

  it('migrates the legacy single key once', async () => {
    const legacy = join(dir, 'apikey')
    await writeFile(legacy, crypto.encrypt('sk-old'))
    const store = createProfileStore(file, crypto, legacy)
    const [profile] = await store.list()
    expect(profile?.name).toBe(MIGRATED_PROFILE_NAME)
    expect(await store.credential(profile?.id ?? '')).toBe('sk-old')
    expect(await store.lastUsedId()).toBe(profile?.id)
    await expect(readFile(legacy)).rejects.toThrow()
    expect(await store.list()).toHaveLength(1)
  })

  it('returns null when a credential cannot be decrypted', async () => {
    const store = createProfileStore(file, crypto)
    const id = await store.add('A', 'k')
    const broken = createProfileStore(file, {
      ...crypto,
      decrypt: () => {
        throw new Error('keychain changed')
      }
    })
    expect(await broken.credential(id)).toBeNull()
  })

  it('ignores malformed files and entries', async () => {
    const store = createProfileStore(file, crypto)
    for (const raw of ['not json', 'null', '{}', '{"profiles":"x"}']) {
      await writeFile(file, raw)
      expect(await store.list()).toEqual([])
    }
    await writeFile(
      file,
      JSON.stringify({
        lastUsedId: 5,
        profiles: [null, { id: 'a', name: 'A' }, { id: 'b', name: 'B', credential: 'eA==' }]
      })
    )
    expect(await store.list()).toEqual([{ id: 'b', name: 'B' }])
    expect(await store.lastUsedId()).toBeNull()
  })

  it('does not migrate without a legacy key or without encryption', async () => {
    expect(await createProfileStore(file, crypto, join(dir, 'missing')).list()).toEqual([])
    const legacy = join(dir, 'apikey')
    await writeFile(legacy, crypto.encrypt('sk-old'))
    const noCrypto = { ...crypto, isAvailable: () => false }
    expect(await createProfileStore(file, noCrypto, legacy).list()).toEqual([])
  })

  it('ignores setLastUsed for an unknown profile', async () => {
    const store = createProfileStore(file, crypto)
    await store.add('A', 'k')
    await store.setLastUsed('missing')
    expect(await store.lastUsedId()).toBeNull()
  })

  it('keeps the last-used id when removing another profile', async () => {
    const store = createProfileStore(file, crypto)
    const a = await store.add('A', 'k1')
    const b = await store.add('B', 'k2')
    await store.setLastUsed(a)
    await store.remove(b)
    expect(await store.lastUsedId()).toBe(a)
  })

  it('does not rename when only the credential changes, and rejects a bad new name', async () => {
    const store = createProfileStore(file, crypto)
    const id = await store.add('A', 'k1')
    await store.add('B', 'k2')
    await expect(store.update(id, { name: 'b' })).rejects.toThrow(/already exists/)
  })
})
