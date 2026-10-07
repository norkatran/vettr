/** A saved credential as the renderer sees it: the secret itself never leaves the main process. */
export interface ProfileInfo {
  id: string
  name: string
}

export interface ProfilesState {
  profiles: ProfileInfo[]
  /** The profile this app instance uses, or null when none is selected. */
  activeId: string | null
}

export const MAX_PROFILE_NAME = 40

/** The trimmed name, or an error message when it is empty, too long or already used by another profile. */
export function validateProfileName(
  name: string,
  existing: ProfileInfo[],
  selfId?: string
): { name: string } | { error: string } {
  const trimmed = name.trim()
  if (!trimmed) return { error: 'Give the profile a name.' }
  if (trimmed.length > MAX_PROFILE_NAME)
    return { error: `Use at most ${MAX_PROFILE_NAME} characters for the name.` }
  const lower = trimmed.toLowerCase()
  if (existing.some((p) => p.id !== selfId && p.name.toLowerCase() === lower))
    return { error: `A profile named "${trimmed}" already exists.` }
  return { name: trimmed }
}
