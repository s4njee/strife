/**
 * Splits a server-marked snippet into plain and highlighted runs.
 *
 * PostgreSQL's `ts_headline` wraps matched terms in `[[` and `]]`. Those markers
 * are turned into structured data here so the result list can build real text
 * and `<mark>` nodes; the snippet text itself is never handed to `innerHTML`.
 * A message body is attacker-controlled, so the only safe thing to do with it is
 * to never let it be parsed as markup.
 *
 * Unbalanced markers are treated as literal text rather than as an implicit
 * highlight to the end of the snippet, so a body that happens to contain `[[`
 * degrades to plain text instead of swallowing the rest of the fragment.
 */
export interface SnippetRun {
  text: string
  marked: boolean
}

const OPEN = '[['
const CLOSE = ']]'

export function parseSnippet(snippet: string): SnippetRun[] {
  const runs: SnippetRun[] = []
  let index = 0

  while (index < snippet.length) {
    const open = snippet.indexOf(OPEN, index)
    if (open === -1) break
    const close = snippet.indexOf(CLOSE, open + OPEN.length)
    if (close === -1) break

    if (open > index) {
      runs.push({ text: snippet.slice(index, open), marked: false })
    }
    const marked = snippet.slice(open + OPEN.length, close)
    if (marked) runs.push({ text: marked, marked: true })
    index = close + CLOSE.length
  }

  if (index < snippet.length) {
    runs.push({ text: snippet.slice(index), marked: false })
  }
  return runs.length > 0 ? runs : [{ text: snippet, marked: false }]
}
