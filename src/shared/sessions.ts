/** A stored agent session of a project, as listed in the Session view. */
export interface SessionInfo {
  /** The Agent SDK session ID, passed back to resume it. */
  id: string
  /** Display title: the SDK's custom title, summary or first prompt. */
  title: string
  /** Last modified time in milliseconds since epoch. */
  lastModified: number
}

const pad = (n: number): string => String(n).padStart(2, '0')

/** Local `YYYY-MM-DD HH:mm`, the timestamp shown in the session list. */
export function formatSessionTime(ms: number): string {
  const d = new Date(ms)
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}

/** The session list row text: `[timestamp] title`. */
export function sessionLabel(session: SessionInfo): string {
  return `[${formatSessionTime(session.lastModified)}] ${session.title}`
}
