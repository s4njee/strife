const STORAGE_KEY = 'strife.commandHistory'
const MAX_HISTORY = 50

export function loadCommandHistory(): string[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []
    return parsed.filter((item): item is string => typeof item === 'string').slice(0, MAX_HISTORY)
  } catch {
    return []
  }
}

export function pushCommandHistory(command: string): string[] {
  const next = [
    command,
    ...loadCommandHistory().filter((entry) => entry !== command),
  ].slice(0, MAX_HISTORY)
  localStorage.setItem(STORAGE_KEY, JSON.stringify(next))
  return next
}
