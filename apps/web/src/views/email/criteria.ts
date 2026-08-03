import type { EmailSearchCriteria } from '../../api/types'

/**
 * Search state lives in the URL so a search is bookmarkable and the browser's
 * back button behaves. The reader's open message is part of that state too:
 * returning from a message must restore the search that found it, not a blank
 * page.
 */
export const EMPTY_CRITERIA: EmailSearchCriteria = {
  q: '',
  from: [],
  participant: [],
  label: [],
  after: '',
  before: '',
  hasAttachment: null,
  includeTrashed: false,
  includeDuplicates: false,
  threadId: '',
  duplicateGroup: '',
}

export function criteriaFromSearch(search: string): EmailSearchCriteria {
  const params = new URLSearchParams(search)
  const attachment = params.get('has_attachment')
  return {
    q: params.get('q') ?? '',
    from: params.getAll('from'),
    participant: params.getAll('participant'),
    label: params.getAll('label'),
    after: params.get('after') ?? '',
    before: params.get('before') ?? '',
    hasAttachment: attachment === null ? null : attachment === 'true',
    includeTrashed: params.get('include_trashed') === 'true',
    includeDuplicates: params.get('include_duplicates') === 'true',
    threadId: params.get('thread_id') ?? '',
    duplicateGroup: params.get('duplicate_group') ?? '',
  }
}

/**
 * Serializes criteria back into a query string.
 *
 * Only non-default values are written, so a cleared search produces a clean URL
 * rather than a trail of empty parameters.
 */
export function criteriaToSearch(
  criteria: EmailSearchCriteria,
  openMessage?: string | null,
): string {
  const params = new URLSearchParams()
  if (criteria.q.trim()) params.set('q', criteria.q.trim())
  for (const value of criteria.from) params.append('from', value)
  for (const value of criteria.participant) params.append('participant', value)
  for (const value of criteria.label) params.append('label', value)
  if (criteria.after) params.set('after', criteria.after)
  if (criteria.before) params.set('before', criteria.before)
  if (criteria.hasAttachment !== null)
    params.set('has_attachment', String(criteria.hasAttachment))
  if (criteria.includeTrashed) params.set('include_trashed', 'true')
  if (criteria.includeDuplicates) params.set('include_duplicates', 'true')
  if (criteria.threadId) params.set('thread_id', criteria.threadId)
  if (criteria.duplicateGroup)
    params.set('duplicate_group', criteria.duplicateGroup)
  if (openMessage) params.set('message', openMessage)
  const query = params.toString()
  return query ? `?${query}` : ''
}

/**
 * Whether the criteria would produce a searchable request.
 *
 * The API rejects a request with neither text nor a structured filter, because
 * it would page the entire archive. The UI checks the same condition so that an
 * empty form shows a prompt instead of provoking a 400.
 */
export function isSearchable(criteria: EmailSearchCriteria): boolean {
  return (
    criteria.q.trim().length > 0 ||
    criteria.from.length > 0 ||
    criteria.participant.length > 0 ||
    criteria.label.length > 0 ||
    criteria.after !== '' ||
    criteria.before !== '' ||
    criteria.hasAttachment !== null ||
    criteria.threadId !== '' ||
    criteria.duplicateGroup !== ''
  )
}

/** Whether anything is set, including criteria that alone are not searchable. */
export function hasAnyCriteria(criteria: EmailSearchCriteria): boolean {
  return (
    isSearchable(criteria) ||
    criteria.includeTrashed ||
    criteria.includeDuplicates
  )
}
