import { createSignal, onCleanup, onMount, Show } from 'solid-js'
import { getReadiness } from '../api/client'
import './StorageWarning.css'

const CHECK_INTERVAL_MS = 60_000
const previewUsage = Number(import.meta.env.VITE_PREVIEW_DISK_USAGE_PERCENT)
const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'

export function StorageWarning() {
  const [usagePercent, setUsagePercent] = createSignal<number | undefined>(
    Number.isFinite(previewUsage)
      ? previewUsage
      : staticPreview
        ? 27.4
        : undefined,
  )

  onMount(() => {
    if (Number.isFinite(previewUsage) || staticPreview) return
    const controller = new AbortController()
    const refresh = () =>
      void getReadiness(controller.signal)
        .then((readiness) =>
          setUsagePercent(readiness.details.disk_usage_percent),
        )
        .catch(() => undefined)
    refresh()
    const interval = window.setInterval(refresh, CHECK_INTERVAL_MS)
    onCleanup(() => {
      controller.abort()
      window.clearInterval(interval)
    })
  })

  const warning = () => storageWarningFor(usagePercent())

  return (
    <Show when={warning()}>
      {(current) => (
        <div
          class="storage-warning"
          classList={{ 'is-error': current().severity === 'error' }}
          role={current().severity === 'error' ? 'alert' : 'status'}
        >
          {current().message}
        </div>
      )}
    </Show>
  )
}

export function storageWarningFor(
  usagePercent: number | undefined,
): { severity: 'warning' | 'error'; message: string } | undefined {
  if (usagePercent === undefined || usagePercent < 80) return undefined
  if (usagePercent >= 90) {
    return {
      severity: 'error',
      message: 'Storage is full. Uploads and imports are disabled.',
    }
  }
  return {
    severity: 'warning',
    message: `Storage is almost full (${usagePercent}% used)`,
  }
}
