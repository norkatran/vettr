export type Theme = 'light' | 'dark'

/** The theme to show: an explicit choice wins, otherwise follow the OS setting. */
export function resolveTheme(choice: Theme | null, systemDark: boolean): Theme {
  return choice ?? (systemDark ? 'dark' : 'light')
}

/** Narrow a stored value to a theme choice; anything unrecognised means "follow the OS". */
export function parseThemeChoice(value: string | null): Theme | null {
  return value === 'light' || value === 'dark' ? value : null
}

export function otherTheme(theme: Theme): Theme {
  return theme === 'dark' ? 'light' : 'dark'
}
