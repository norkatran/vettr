import type { ProfileInfo, ProfilesState } from '@shared/profiles'
import { useCallback, useEffect, useState } from 'react'

export interface Profiles {
  /** Null until known. Credentials never reach the renderer. */
  state: ProfilesState | null
  /** The profile this app instance uses, if any. */
  active: ProfileInfo | null
  /** Whether any profile is saved; null until known. */
  hasProfiles: boolean | null
  /** Each resolves to null on success, or to an error message. */
  add(name: string, credential: string): Promise<string | null>
  update(id: string, changes: { name?: string; credential?: string }): Promise<string | null>
  remove(id: string): Promise<string | null>
  use(id: string): Promise<string | null>
}

/** The saved credential profiles and this instance's active one, shared by every view. */
export function useProfiles(): Profiles {
  const [state, setState] = useState<ProfilesState | null>(null)
  useEffect(() => {
    void window.vettr.getProfiles().then(setState)
    return window.vettr.onProfilesChanged(setState)
  }, [])

  const add = useCallback((name: string, key: string) => window.vettr.addProfile(name, key), [])
  const update = useCallback(
    (id: string, changes: { name?: string; credential?: string }) =>
      window.vettr.updateProfile(id, changes),
    []
  )
  const remove = useCallback((id: string) => window.vettr.removeProfile(id), [])
  const use = useCallback((id: string) => window.vettr.setActiveProfile(id), [])

  return {
    state,
    active: state?.profiles.find((p) => p.id === state.activeId) ?? null,
    hasProfiles: state ? state.profiles.length > 0 : null,
    add,
    update,
    remove,
    use
  }
}
