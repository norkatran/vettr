import { mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { dirname } from 'node:path'

/** The slice of Electron's `safeStorage` the store needs, injected so it can be tested. */
export interface KeyCrypto {
  isAvailable(): boolean
  encrypt(plain: string): Buffer
  decrypt(encrypted: Buffer): string
}

export interface ApiKeyStore {
  /** The stored key, or null if none is saved or it cannot be decrypted. */
  get(): Promise<string | null>
  /** Encrypt and save the key. Throws if the OS cannot encrypt, rather than storing plain text. */
  set(key: string): Promise<void>
  clear(): Promise<void>
}

/** Keeps the Anthropic API key on the host, encrypted with the OS keychain (never plain text). */
export function createApiKeyStore(file: string, crypto: KeyCrypto): ApiKeyStore {
  return {
    async get() {
      try {
        return crypto.decrypt(await readFile(file))
      } catch {
        return null
      }
    },
    async set(key) {
      if (!key) throw new Error('The API key is empty')
      if (!crypto.isAvailable()) throw new Error('Secure storage is not available on this system')
      await mkdir(dirname(file), { recursive: true })
      await writeFile(file, crypto.encrypt(key), { mode: 0o600 })
    },
    async clear() {
      await rm(file, { force: true })
    }
  }
}
