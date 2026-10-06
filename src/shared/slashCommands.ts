import type { SlashCommandInfo } from './agent'

/**
 * The text after a leading slash when `text` is a command being typed (no whitespace yet), or
 * null once it is ordinary prose or the command has its arguments.
 */
export function slashQuery(text: string): string | null {
  const match = /^\/(\S*)$/.exec(text)
  return match ? match[1] : null
}

/** Commands matching what has been typed: name prefixes first, then other substrings. */
export function filterCommands(commands: SlashCommandInfo[], query: string): SlashCommandInfo[] {
  const q = query.toLowerCase()
  const names = (c: SlashCommandInfo): string[] => [c.name, ...(c.aliases ?? [])]
  const rank = (c: SlashCommandInfo): number => {
    if (names(c).some((n) => n.toLowerCase().startsWith(q))) return 0
    return names(c).some((n) => n.toLowerCase().includes(q)) ? 1 : 2
  }
  return commands
    .map((c) => ({ c, r: rank(c) }))
    .filter(({ r }) => r < 2)
    .sort((a, b) => a.r - b.r || a.c.name.localeCompare(b.c.name))
    .map(({ c }) => c)
}

/** The composer text after choosing `command`: ready for its arguments. */
export function completeCommand(command: SlashCommandInfo): string {
  return `/${command.name} `
}
