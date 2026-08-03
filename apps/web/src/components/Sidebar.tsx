import { A, useLocation } from '@solidjs/router'
import {
  createEffect,
  createResource,
  createSignal,
  For,
  onCleanup,
  Show,
} from 'solid-js'
import {
  getEmailStatus,
  getImportEntries,
  getMetadataStatus,
  getOcrStatus,
  getStorageUsage,
} from '../api/client'
import type { StorageUsage } from '../api/client'
import type { EmailStatus } from '../api/types'
import { useTheme } from '../theme/ThemeProvider'
import './Sidebar.css'

const navigation = [
  { href: '/', label: 'All Files', icon: 'grid', end: true },
  { href: '/favorites', label: 'Favorites', icon: 'star', end: false },
  { href: '/imports', label: 'Imports', icon: 'inbox', end: false },
  { href: '/console', label: 'Console', icon: 'terminal', end: false },
  { href: '/email', label: 'Email', icon: 'mail', end: false },
  { href: '/ocr', label: 'OCR', icon: 'scan', end: false },
  { href: '/errors', label: 'Errors', icon: 'alert', end: false },
  { href: '/trash', label: 'Trash', icon: 'trash', end: false },
] as const

/** Deterministic counts for the static preview build. */
const previewEmailPending = { foreground: 3, backfill: 128 }

const previewStorageUsage: StorageUsage = {
  total_bytes: 5_000_000_000_000,
  used_bytes: 1_370_000_000_000,
  available_bytes: 3_630_000_000_000,
  originals_bytes: 1_260_000_000_000,
  artifacts_bytes: 85_000_000_000,
  trash_bytes: 25_000_000_000,
  usage_percent: 27.4,
}

function formatBytes(bytes: number): string {
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

export function Sidebar() {
  const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'
  const location = useLocation()
  const { theme, toggleTheme } = useTheme()
  const [showDetails, setShowDetails] = createSignal(false)
  const [usage] = createResource(
    () => !staticPreview,
    () => getStorageUsage(),
  )
  const [errorCount] = createResource(
    () => !staticPreview,
    async () => {
      try {
        const entries = await getImportEntries(
          '00000000-0000-0000-0000-000000000003',
          'failed',
        )
        return entries.length
      } catch {
        return 0
      }
    },
  )
  const [ocrPending] = createResource(
    () => !staticPreview,
    async () => {
      try {
        return (await getOcrStatus()).counts.remaining
      } catch {
        return 0
      }
    },
  )
  const [metadataPending] = createResource(
    () => !staticPreview,
    async () => {
      try {
        return (await getMetadataStatus()).counts.remaining
      } catch {
        return 0
      }
    },
  )
  // Only foreground work counts toward the badge. A paused ten-year backfill
  // sitting at 600,000 queued would otherwise permanently pin a number to the
  // navigation that no user action can clear.
  const [emailPending, setEmailPending] = createSignal(previewEmailPending)

  // Seeded once, then kept current by the same status stream the Email page
  // uses, so the badge changes as jobs finish rather than only on navigation.
  createEffect(() => {
    if (staticPreview) return
    const apply = (status: EmailStatus) =>
      setEmailPending({
        foreground:
          status.counts.foreground_pending + status.counts.foreground_running,
        backfill:
          status.counts.backfill_pending + status.counts.backfill_running,
      })
    void getEmailStatus()
      .then(apply)
      .catch(() => setEmailPending({ foreground: 0, backfill: 0 }))

    const events = new EventSource('/api/email/events')
    events.addEventListener('status', (event) => {
      try {
        apply(JSON.parse((event as MessageEvent<string>).data) as EmailStatus)
      } catch {
        // A malformed frame leaves the last good counts in place.
      }
    })
    onCleanup(() => events.close())
  })
  const emailBadge = () => emailPending()
  const currentUsage = () => (staticPreview ? previewStorageUsage : usage())

  const percent = () => Math.round(currentUsage()?.usage_percent ?? 0)
  const meterClass = () => {
    const value = percent()
    if (value >= 90) return 'is-critical'
    if (value >= 80) return 'is-warning'
    return undefined
  }

  return (
    <aside class="sidebar">
      <div class="sidebar__brand">
        <span class="sidebar__brand-mark" aria-hidden="true">
          S
        </span>
        <span>
          <strong>Strife</strong>
          <small>Home drive</small>
        </span>
      </div>

      <nav class="sidebar__nav" aria-label="Primary navigation">
        <For each={navigation}>
          {(item) => (
            <A
              href={item.href}
              end={item.end}
              class="sidebar__nav-link"
              activeClass="is-active"
              aria-current={
                location.pathname === item.href ||
                (item.href === '/' && location.pathname.startsWith('/folder/'))
                  ? 'page'
                  : undefined
              }
            >
              <SidebarIcon name={item.icon} />
              <span>{item.label}</span>
              <Show when={item.href === '/errors' && (errorCount() ?? 0) > 0}>
                <span class="sidebar__badge">{errorCount()}</span>
              </Show>
              <Show when={item.href === '/ocr' && (ocrPending() ?? 0) > 0}>
                <span class="sidebar__badge">{ocrPending()}</span>
              </Show>
              <Show
                when={item.href === '/console' && (metadataPending() ?? 0) > 0}
              >
                <span class="sidebar__badge">{metadataPending()}</span>
              </Show>
              <Show
                when={
                  item.href === '/email' && (emailBadge()?.foreground ?? 0) > 0
                }
              >
                <span class="sidebar__badge">{emailBadge()?.foreground}</span>
              </Show>
              {/* Historical work gets its own muted treatment so a paused
                  campaign never reads as new mail stuck in the queue. */}
              <Show
                when={
                  item.href === '/email' && (emailBadge()?.backfill ?? 0) > 0
                }
              >
                <span
                  class="sidebar__badge is-backfill"
                  title={`${emailBadge()?.backfill} queued by a historical backfill`}
                >
                  {emailBadge()?.backfill}
                </span>
              </Show>
            </A>
          )}
        </For>
      </nav>

      <div class="sidebar__spacer" />

      <section class="storage-summary" aria-labelledby="storage-summary-title">
        <div class="storage-summary__heading">
          <strong id="storage-summary-title">Storage</strong>
          <span>{percent()}%</span>
        </div>
        <button
          type="button"
          class="storage-summary__meter-button"
          onClick={() => setShowDetails((open) => !open)}
          aria-expanded={showDetails()}
        >
          <div
            class="storage-summary__meter"
            classList={{
              'is-warning': meterClass() === 'is-warning',
              'is-critical': meterClass() === 'is-critical',
            }}
            role="progressbar"
            aria-valuenow={percent()}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-label="Storage used"
          >
            <span style={{ width: `${Math.min(100, percent())}%` }} />
          </div>
        </button>
        <p>
          {formatBytes(currentUsage()?.used_bytes ?? 0)} of{' '}
          {formatBytes(currentUsage()?.total_bytes ?? 0)} used
        </p>
        <Show when={showDetails() && currentUsage()}>
          {(data) => (
            <ul class="storage-summary__breakdown">
              <li>Originals · {formatBytes(data().originals_bytes)}</li>
              <li>Previews · {formatBytes(data().artifacts_bytes)}</li>
              <li>Trash · {formatBytes(data().trash_bytes)}</li>
            </ul>
          )}
        </Show>
      </section>

      <button class="sidebar__theme-toggle" type="button" onClick={toggleTheme}>
        <SidebarIcon name={theme() === 'dark' ? 'sun' : 'moon'} />
        Use {theme() === 'dark' ? 'light' : 'dark'} theme
      </button>
    </aside>
  )
}

type IconName =
  | 'grid'
  | 'star'
  | 'inbox'
  | 'terminal'
  | 'mail'
  | 'scan'
  | 'trash'
  | 'sun'
  | 'moon'
  | 'alert'

function SidebarIcon(props: { name: IconName }) {
  const paths: Record<IconName, string> = {
    grid: 'M4 4h5v5H4zM15 4h5v5h-5zM4 15h5v5H4zM15 15h5v5h-5z',
    star: 'm12 3 2.7 5.5 6.1.9-4.4 4.3 1 6.1-5.4-2.9-5.4 2.9 1-6.1-4.4-4.3 6.1-.9z',
    inbox: 'M4 5h16v14H4zM4 14h4l2 2h4l2-2h4',
    terminal: 'M4 5h16v14H4zM7 9l3 3-3 3m5 0h5',
    mail: 'M3 6h18v12H3zM3 7l9 6 9-6',
    scan: 'M7 3H3v4m14-4h4v4M7 21H3v-4m14 4h4v-4M7 9h10v6H7z',
    trash: 'M5 7h14M9 7V4h6v3m2 0-1 13H8L7 7m3 4v5m4-5v5',
    sun: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8m0-5v2m0 14v2M3 12h2m14 0h2M5.6 5.6 7 7m10 10 1.4 1.4M18.4 5.6 17 7M7 17l-1.4 1.4',
    moon: 'M20 15.2A8.5 8.5 0 0 1 8.8 4 8.5 8.5 0 1 0 20 15.2',
    alert: 'M12 3 2 21h20zm0 6v5m0 3h.01',
  }

  return (
    <svg class="sidebar__icon" viewBox="0 0 24 24" aria-hidden="true">
      <path d={paths[props.name]} />
    </svg>
  )
}
