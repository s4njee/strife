import { describe, expect, it } from 'vitest'
import {
  criteriaFromSearch,
  criteriaToSearch,
  EMPTY_CRITERIA,
  hasAnyCriteria,
  isSearchable,
} from './criteria'

describe('criteria URL round trip', () => {
  it('preserves repeated values rather than collapsing them', () => {
    // Repeated keys are the whole reason the API parses its query string by
    // hand; losing them here would silently drop every label but one.
    const search = criteriaToSearch({
      ...EMPTY_CRITERIA,
      label: ['Work', 'Receipts'],
      from: ['ada@example.test', 'bob@example.test'],
    })
    const parsed = criteriaFromSearch(search)
    expect(parsed.label).toEqual(['Work', 'Receipts'])
    expect(parsed.from).toEqual(['ada@example.test', 'bob@example.test'])
  })

  it('round trips every field', () => {
    const original = {
      q: 'quarterly report',
      from: ['ada@example.test'],
      participant: ['bob@example.test'],
      label: ['Work'],
      after: '2019-01-01',
      before: '2020-01-01',
      hasAttachment: true,
      includeTrashed: true,
      includeDuplicates: true,
    }
    expect(criteriaFromSearch(criteriaToSearch(original))).toEqual(original)
  })

  it('distinguishes an unset attachment filter from an explicit false', () => {
    expect(criteriaFromSearch('').hasAttachment).toBeNull()
    expect(
      criteriaFromSearch(
        criteriaToSearch({ ...EMPTY_CRITERIA, hasAttachment: false }),
      ).hasAttachment,
    ).toBe(false)
  })

  it('writes a clean URL when nothing is set', () => {
    expect(criteriaToSearch(EMPTY_CRITERIA)).toBe('')
  })

  it('carries the open message alongside the search', () => {
    const search = criteriaToSearch({ ...EMPTY_CRITERIA, q: 'invoice' }, 'abc')
    expect(new URLSearchParams(search).get('message')).toBe('abc')
    // The message is reader state, not a search criterion.
    expect(criteriaFromSearch(search).q).toBe('invoice')
  })

  it('refuses to call an unconstrained request searchable', () => {
    // The API rejects a query with neither text nor a filter because it would
    // page the whole archive; the UI must not provoke that 400.
    expect(isSearchable(EMPTY_CRITERIA)).toBe(false)
    expect(isSearchable({ ...EMPTY_CRITERIA, q: '   ' })).toBe(false)
    expect(isSearchable({ ...EMPTY_CRITERIA, q: 'x' })).toBe(true)
    expect(isSearchable({ ...EMPTY_CRITERIA, label: ['Work'] })).toBe(true)
    expect(isSearchable({ ...EMPTY_CRITERIA, hasAttachment: false })).toBe(true)
  })

  it('treats trash and duplicate toggles as clearable but not searchable', () => {
    const toggled = { ...EMPTY_CRITERIA, includeTrashed: true }
    expect(isSearchable(toggled)).toBe(false)
    expect(hasAnyCriteria(toggled)).toBe(true)
  })
})
