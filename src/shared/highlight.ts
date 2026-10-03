import hljs from 'highlight.js/lib/core'
import bash from 'highlight.js/lib/languages/bash'
import c from 'highlight.js/lib/languages/c'
import cpp from 'highlight.js/lib/languages/cpp'
import csharp from 'highlight.js/lib/languages/csharp'
import css from 'highlight.js/lib/languages/css'
import diff from 'highlight.js/lib/languages/diff'
import dockerfile from 'highlight.js/lib/languages/dockerfile'
import go from 'highlight.js/lib/languages/go'
import ini from 'highlight.js/lib/languages/ini'
import java from 'highlight.js/lib/languages/java'
import javascript from 'highlight.js/lib/languages/javascript'
import json from 'highlight.js/lib/languages/json'
import kotlin from 'highlight.js/lib/languages/kotlin'
import markdown from 'highlight.js/lib/languages/markdown'
import php from 'highlight.js/lib/languages/php'
import python from 'highlight.js/lib/languages/python'
import ruby from 'highlight.js/lib/languages/ruby'
import rust from 'highlight.js/lib/languages/rust'
import scss from 'highlight.js/lib/languages/scss'
import sql from 'highlight.js/lib/languages/sql'
import swift from 'highlight.js/lib/languages/swift'
import typescript from 'highlight.js/lib/languages/typescript'
import xml from 'highlight.js/lib/languages/xml'
import yaml from 'highlight.js/lib/languages/yaml'
import type { DiffLine } from './diff'

const LANGUAGES = {
  bash,
  c,
  cpp,
  csharp,
  css,
  diff,
  dockerfile,
  go,
  ini,
  java,
  javascript,
  json,
  kotlin,
  markdown,
  php,
  python,
  ruby,
  rust,
  scss,
  sql,
  swift,
  typescript,
  xml,
  yaml
}
for (const [name, definition] of Object.entries(LANGUAGES)) {
  hljs.registerLanguage(name, definition)
}

const EXTENSIONS: Record<string, string> = {
  sh: 'bash',
  bash: 'bash',
  zsh: 'bash',
  c: 'c',
  h: 'c',
  cc: 'cpp',
  cpp: 'cpp',
  cxx: 'cpp',
  hpp: 'cpp',
  hh: 'cpp',
  cs: 'csharp',
  css: 'css',
  diff: 'diff',
  patch: 'diff',
  go: 'go',
  ini: 'ini',
  toml: 'ini',
  conf: 'ini',
  java: 'java',
  js: 'javascript',
  jsx: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  json: 'json',
  jsonc: 'json',
  kt: 'kotlin',
  kts: 'kotlin',
  md: 'markdown',
  markdown: 'markdown',
  php: 'php',
  py: 'python',
  rb: 'ruby',
  rs: 'rust',
  scss: 'scss',
  sql: 'sql',
  swift: 'swift',
  ts: 'typescript',
  tsx: 'typescript',
  mts: 'typescript',
  cts: 'typescript',
  html: 'xml',
  htm: 'xml',
  xml: 'xml',
  svg: 'xml',
  vue: 'xml',
  yaml: 'yaml',
  yml: 'yaml'
}

/** The highlight.js language for a file path, or null when it has no known grammar. */
export function languageFor(path: string): string | null {
  const name = path.slice(path.lastIndexOf('/') + 1).toLowerCase()
  if (name === 'dockerfile' || name.startsWith('dockerfile.')) return 'dockerfile'
  const dot = name.lastIndexOf('.')
  if (dot < 0) return null
  return EXTENSIONS[name.slice(dot + 1)] ?? null
}

const OPEN_TAG = /^<span [^>]*>/

/**
 * Split highlighted HTML into one HTML string per source line. Spans that cross a newline
 * (block comments, template strings) are closed at the end of each line and reopened on the next.
 */
export function splitHighlightedLines(html: string): string[] {
  const lines: string[] = []
  const open: string[] = []
  let current = ''
  let i = 0
  while (i < html.length) {
    const ch = html[i]
    if (ch === '\n') {
      lines.push(current + '</span>'.repeat(open.length))
      current = open.join('')
      i++
    } else if (ch === '<') {
      if (html.startsWith('</span>', i)) {
        open.pop()
        current += '</span>'
        i += 7
      } else {
        const tag = OPEN_TAG.exec(html.slice(i))?.[0]
        if (!tag) {
          current += ch
          i++
          continue
        }
        open.push(tag)
        current += tag
        i += tag.length
      }
    } else {
      current += ch
      i++
    }
  }
  lines.push(current + '</span>'.repeat(open.length))
  return lines
}

function highlightSide(language: string, texts: string[]): string[] | null {
  try {
    const html = hljs.highlight(texts.join('\n'), { language, ignoreIllegals: true }).value
    const lines = splitHighlightedLines(html)
    return lines.length === texts.length ? lines : null
  } catch {
    return null
  }
}

/**
 * Highlight a hunk, returning HTML for each line (keyed by the line object). The old side
 * (context + deletions) and the new side (context + additions) are tokenized separately so
 * each reads as coherent source. Returns an empty map for unknown languages or on failure.
 */
export function highlightHunk(path: string, lines: DiffLine[]): Map<DiffLine, string> {
  const result = new Map<DiffLine, string>()
  const language = languageFor(path)
  if (!language) return result
  const old = lines.filter((l) => l.kind !== 'add')
  const next = lines.filter((l) => l.kind !== 'del')
  const oldHtml = highlightSide(
    language,
    old.map((l) => l.text)
  )
  const newHtml = highlightSide(
    language,
    next.map((l) => l.text)
  )
  // Context lines take the new-side markup; deletions only exist on the old side.
  if (oldHtml) {
    for (const [n, l] of old.entries()) if (l.kind === 'del') result.set(l, oldHtml[n] as string)
  }
  if (newHtml) {
    for (const [n, l] of next.entries()) result.set(l, newHtml[n] as string)
  }
  return result
}
