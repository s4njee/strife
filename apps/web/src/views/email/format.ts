/**
 * Display helpers for archived mail.
 *
 * Archived messages routinely lack a subject, a sender, or a usable date, and a
 * ten-year import contains plenty of all three. Every helper here returns
 * something a screen reader can announce rather than an empty string, because a
 * blank cell in a result list is indistinguishable from a rendering bug.
 */

export const MISSING_SUBJECT = '(no subject)'
export const MISSING_SENDER = '(no sender recorded)'
export const MISSING_DATE = '(no date recorded)'

export function subjectOf(subject: string | null): string {
  const trimmed = subject?.trim()
  return trimmed ? trimmed : MISSING_SUBJECT
}

/** Prefers a display name, falls back to the address, then to a stated gap. */
export function senderOf(
  displayName: string | null,
  address: string | null,
): string {
  const name = displayName?.trim()
  if (name) return name
  const value = address?.trim()
  return value ? value : MISSING_SENDER
}

/** Localized date for display. The machine-readable value goes in `datetime`. */
export function formatDate(value: string | null): string {
  if (!value) return MISSING_DATE
  const parsed = new Date(value)
  if (Number.isNaN(parsed.getTime())) return MISSING_DATE
  return parsed.toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  })
}

export function formatDateTime(value: string | null): string {
  if (!value) return MISSING_DATE
  const parsed = new Date(value)
  if (Number.isNaN(parsed.getTime())) return MISSING_DATE
  return parsed.toLocaleString()
}

export function formatBytes(bytes: number | null): string {
  if (bytes === null) return 'size unknown'
  if (bytes < 1000) return `${bytes} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let value = bytes / 1000
  let unit = 0
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000
    unit += 1
  }
  return `${value.toFixed(value >= 10 ? 0 : 1)} ${units[unit]}`
}

/** Address roles in the order a reader expects to see them. */
export const ROLE_ORDER = [
  'from',
  'sender',
  'reply_to',
  'to',
  'cc',
  'bcc',
] as const

export const ROLE_LABELS: Record<string, string> = {
  from: 'From',
  sender: 'Sender',
  reply_to: 'Reply to',
  to: 'To',
  cc: 'Cc',
  bcc: 'Bcc',
}
