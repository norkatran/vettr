/**
 * Score how well `query` matches `text` as a case-insensitive subsequence, or null if it does not.
 * Higher is better: consecutive characters and matches at the start of a word score more.
 */
export function fuzzyScore(query: string, text: string): number | null {
  const q = query.toLowerCase()
  const t = text.toLowerCase()
  let score = 0
  let from = 0
  let previous = -2
  for (const char of q) {
    const index = t.indexOf(char, from)
    if (index === -1) return null
    score += 1
    if (index === previous + 1) score += 3
    if (index === 0 || t[index - 1] === ' ') score += 2
    previous = index
    from = index + 1
  }
  return score
}

/** Items matching `query`, best first (original order breaks ties); all of them for a blank query. */
export function fuzzyFilter<T>(items: T[], query: string, text: (item: T) => string): T[] {
  if (query.trim() === '') return items
  return items
    .map((item, index) => ({ item, index, score: fuzzyScore(query.trim(), text(item)) }))
    .filter((r): r is { item: T; index: number; score: number } => r.score !== null)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((r) => r.item)
}
