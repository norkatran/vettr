import { randomUUID } from 'node:crypto'
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises'
import { dirname } from 'node:path'
import { type ProfileInfo, validateProfileName } from '@shared/profiles'
import { createApiKeyStore, type KeyCrypto } from './apiKey'

interface StoredProfile {
  id: string
  name: string
  /** Base64 of the encrypted credential. */
  credential: string
}

interface StoreFile {
  lastUsedId: string | null
  profiles: StoredProfile[]
}

export interface ProfileStore {
  list(): Promise<ProfileInfo[]>
  /** The profile id new app instances start with, if it still exists. */
  lastUsedId(): Promise<string | null>
  setLastUsed(id: string): Promise<void>
  /** The decrypted credential, or null if the profile is gone or cannot be decrypted. */
  credential(id: string): Promise<string | null>
  /** Save a new profile and resolve to its id. Throws a user-facing message on a bad name. */
  add(name: string, credential: string): Promise<string>
  /** Rename and/or replace the credential. Throws a user-facing message. */
  update(id: string, changes: { name?: string; credential?: string }): Promise<void>
  remove(id: string): Promise<void>
}

/** Name given to the credential saved before profiles existed. */
export const MIGRATED_PROFILE_NAME = 'Default'

function parse(raw: unknown): StoreFile {
  const empty: StoreFile = { lastUsedId: null, profiles: [] }
  if (typeof raw !== 'object' || raw === null) return empty
  const { lastUsedId, profiles } = raw as Record<string, unknown>
  if (!Array.isArray(profiles)) return empty
  return {
    lastUsedId: typeof lastUsedId === 'string' ? lastUsedId : null,
    profiles: profiles.filter(
      (p): p is StoredProfile =>
        typeof p?.id === 'string' && typeof p.name === 'string' && typeof p.credential === 'string'
    )
  }
}

/**
 * Named credentials on the host, encrypted with the OS keychain. The file is re-read for every
 * operation (and writes are serialised within this process) so several running instances share it
 * without clobbering each other from stale memory. `legacyFile` is the old single-key file, which
 * becomes a profile the first time there is no profiles file.
 */
export function createProfileStore(
  file: string,
  crypto: KeyCrypto,
  legacyFile?: string
): ProfileStore {
  let queue: Promise<unknown> = Promise.resolve()
  const serial = <T>(task: () => Promise<T>): Promise<T> => {
    const result = queue.then(task)
    queue = result.catch(() => undefined)
    return result
  }

  const read = async (): Promise<StoreFile> => {
    try {
      return parse(JSON.parse(await readFile(file, 'utf8')))
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code !== 'ENOENT') return parse(null)
    }
    return migrate()
  }

  const write = async (data: StoreFile): Promise<void> => {
    await mkdir(dirname(file), { recursive: true })
    const temp = `${file}.${process.pid}.tmp`
    await writeFile(temp, JSON.stringify(data, null, 2), { mode: 0o600 })
    await rename(temp, file)
  }

  const migrate = async (): Promise<StoreFile> => {
    const data: StoreFile = { lastUsedId: null, profiles: [] }
    if (!legacyFile) return data
    const legacy = createApiKeyStore(legacyFile, crypto)
    const key = await legacy.get()
    if (key === null || !crypto.isAvailable()) return data
    const id = randomUUID()
    data.profiles.push({
      id,
      name: MIGRATED_PROFILE_NAME,
      credential: crypto.encrypt(key).toString('base64')
    })
    data.lastUsedId = id
    await write(data)
    await legacy.clear()
    return data
  }

  const encrypt = (credential: string): string => {
    const trimmed = credential.trim()
    if (!trimmed) throw new Error('The API key is empty')
    if (!crypto.isAvailable()) throw new Error('Secure storage is not available on this system')
    return crypto.encrypt(trimmed).toString('base64')
  }

  return {
    list: () => serial(async () => (await read()).profiles.map(({ id, name }) => ({ id, name }))),
    async lastUsedId() {
      const data = await serial(read)
      return data.profiles.some((p) => p.id === data.lastUsedId) ? data.lastUsedId : null
    },
    setLastUsed: (id) =>
      serial(async () => {
        const data = await read()
        if (data.profiles.some((p) => p.id === id)) await write({ ...data, lastUsedId: id })
      }),
    async credential(id) {
      const profile = (await serial(read)).profiles.find((p) => p.id === id)
      if (!profile) return null
      try {
        return crypto.decrypt(Buffer.from(profile.credential, 'base64'))
      } catch {
        return null
      }
    },
    add: (name, credential) =>
      serial(async () => {
        const data = await read()
        const checked = validateProfileName(name, data.profiles)
        if ('error' in checked) throw new Error(checked.error)
        const id = randomUUID()
        data.profiles.push({ id, name: checked.name, credential: encrypt(credential) })
        await write(data)
        return id
      }),
    update: (id, changes) =>
      serial(async () => {
        const data = await read()
        const profile = data.profiles.find((p) => p.id === id)
        if (!profile) throw new Error('That profile no longer exists.')
        if (changes.name !== undefined) {
          const checked = validateProfileName(changes.name, data.profiles, id)
          if ('error' in checked) throw new Error(checked.error)
          profile.name = checked.name
        }
        if (changes.credential !== undefined) profile.credential = encrypt(changes.credential)
        await write(data)
      }),
    remove: (id) =>
      serial(async () => {
        const data = await read()
        await write({
          lastUsedId: data.lastUsedId === id ? null : data.lastUsedId,
          profiles: data.profiles.filter((p) => p.id !== id)
        })
      })
  }
}
