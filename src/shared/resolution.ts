/**
 * Resolving a comment resolves its whole thread (the comment and the agent's replies to it), as on
 * GitHub or GitLab. Only the user resolves: the agent's "resolved" reply is advisory. The ids of the
 * resolved comments are kept per project by the app (comment ids are UUIDs, unique across sessions).
 */

/** The resolved ids from stored data; anything that is not a list of strings is dropped. */
export function parseResolved(raw: unknown): string[] {
  const list = Array.isArray(raw) ? raw : []
  return [...new Set(list.filter((id): id is string => typeof id === 'string' && id !== ''))]
}

/** `ids` with `id` marked resolved or reopened; the same array when nothing changes. */
export function withResolved(ids: string[], id: string, resolved: boolean): string[] {
  if (ids.includes(id) === resolved) return ids
  return resolved ? [...ids, id] : ids.filter((other) => other !== id)
}
