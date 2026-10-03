/**
 * Turn an editor command template (for example `code -g {file}:{line}`) into an argv for
 * `execFile`. The template is split on whitespace (double quotes group words) before `{file}`
 * and `{line}` are substituted, so a path with spaces or shell characters stays one argument
 * and no shell is involved. Returns null when the template is empty.
 */
export function buildEditorCommand(
  template: string,
  file: string,
  line: number,
  project: string
): { command: string; args: string[] } | null {
  const words = (template.match(/"[^"]*"|\S+/g) ?? []).map((w) =>
    w.startsWith('"') && w.endsWith('"') && w.length >= 2 ? w.slice(1, -1) : w
  )
  const [command, ...args] = words.map((w) =>
    w.replace(/\{(file|line|project)\}/g, (_, key: string) =>
      key === 'file' ? file : key === 'line' ? String(line) : project
    )
  )
  return command ? { command, args } : null
}
