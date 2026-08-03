import { describe, expect, it } from 'vitest'
import { parseSnippet } from './snippet'

/**
 * The parser is the reason a message body can be highlighted without ever being
 * treated as markup, so its edge cases are worth pinning down: every run it
 * emits becomes a text node, and anything it mis-splits becomes visible text
 * rather than an injection.
 */
describe('parseSnippet', () => {
  it('splits marked terms out of surrounding text', () => {
    expect(parseSnippet('the [[quarterly]] report')).toEqual([
      { text: 'the ', marked: false },
      { text: 'quarterly', marked: true },
      { text: ' report', marked: false },
    ])
  })

  it('handles several marks and a mark at each end', () => {
    expect(parseSnippet('[[a]] and [[b]]')).toEqual([
      { text: 'a', marked: true },
      { text: ' and ', marked: false },
      { text: 'b', marked: true },
    ])
  })

  it('treats an unclosed marker as literal text', () => {
    // A body containing "[[" must not silently highlight to the end of the
    // fragment; it degrades to plain text instead.
    expect(parseSnippet('see [[ref for details')).toEqual([
      { text: 'see [[ref for details', marked: false },
    ])
  })

  it('returns a single plain run when nothing is marked', () => {
    expect(parseSnippet('no markers here')).toEqual([
      { text: 'no markers here', marked: false },
    ])
  })

  it('never yields markup, only text', () => {
    // HTML in the body stays data. It is emitted as text and becomes a text
    // node at render time, so it can never open an element.
    const runs = parseSnippet('<script>alert(1)</script> [[hit]]')
    expect(runs[0]).toEqual({
      text: '<script>alert(1)</script> ',
      marked: false,
    })
    expect(runs[1]).toEqual({ text: 'hit', marked: true })
  })

  it('handles an empty snippet', () => {
    expect(parseSnippet('')).toEqual([{ text: '', marked: false }])
  })
})
