import {
  createEffect,
  createResource,
  createSignal,
  For,
  onCleanup,
  Show,
} from 'solid-js'
import { getMetadataStatus, getRecentMetadataEvents } from '../api/client'
import type { MetadataEvent, MetadataStatus } from '../api/types'
import './ImportStatusView.css'
import './ConsoleView.css'

const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'

export function ConsoleView() {
  const [entries, setEntries] = createSignal<MetadataEvent[]>(
    staticPreview ? sampleEvents : [],
  )
  const [streamStatus, setStreamStatus] = createSignal<
    'connecting' | 'live' | 'reconnecting'
  >(staticPreview ? 'live' : 'connecting')
  const [status, { mutate }] = createResource(
    () => !staticPreview,
    () => getMetadataStatus(),
  )
  const [recent] = createResource(
    () => !staticPreview,
    () => getRecentMetadataEvents(50),
  )
  const current = () => (staticPreview ? sampleStatus : status())

  createEffect(() => {
    const initial = recent()
    if (initial) setEntries(initial)
  })
  createEffect(() => {
    if (staticPreview) return
    const events = new EventSource('/api/metadata/events')
    events.onopen = () => setStreamStatus('live')
    events.onerror = () => setStreamStatus('reconnecting')
    events.addEventListener('entry', (event) => {
      const parsed = parseEvent((event as MessageEvent<string>).data)
      if (!parsed) return
      setEntries((items) => [
        parsed,
        ...items.filter((item) => item.id !== parsed.id).slice(0, 198),
      ])
    })
    events.addEventListener('status', (event) => {
      const parsed = parseStatus((event as MessageEvent<string>).data)
      if (parsed) mutate(parsed)
    })
    onCleanup(() => events.close())
  })

  const processed = () => {
    const counts = current()?.counts
    if (!counts) return 0
    return counts.completed + counts.failed + counts.skipped + counts.cancelled
  }
  const percent = () => {
    const total = current()?.counts.total ?? 0
    return total > 0 ? Math.min(100, (processed() / total) * 100) : 0
  }

  return (
    <section class="workspace-view import-view console-view">
      <header>
        <p class="workspace-view__eyebrow">Processing</p>
        <h1>Console</h1>
        <p class="workspace-view__description">
          Live metadata extraction progress and worker activity.
        </p>
      </header>

      <Show when={current()} fallback={<p>Loading metadata status…</p>}>
        {(data) => (
          <article class="import-source console-summary">
            <div class="import-source__heading">
              <div>
                <span class="import-source__status">
                  {data().counts.remaining.toLocaleString()} remaining
                </span>
                <h2>Metadata extraction</h2>
                <p>
                  {data().completed_per_hour.toLocaleString()} files/hour
                  {' · '}
                  {formatEta(data().eta_seconds)}
                </p>
              </div>
              <strong class="console-summary__percent">
                {percent().toFixed(1)}%
              </strong>
            </div>

            <div class="console-progress" aria-label="Metadata progress">
              <span style={{ width: `${percent()}%` }} />
            </div>

            <dl class="import-source__counts console-view__counts">
              <Count label="Pending" value={data().counts.pending} />
              <Count label="Running" value={data().counts.running} />
              <Count label="Completed" value={data().counts.completed} />
              <Count label="Failed" value={data().counts.failed} error />
              <Count label="Skipped" value={data().counts.skipped} />
              <Count label="Total" value={data().counts.total} />
            </dl>

            <section
              class="import-console console-output"
              aria-labelledby="metadata-console-title"
            >
              <div class="import-console__heading">
                <h3 id="metadata-console-title">Metadata activity</h3>
                <span class={`is-${streamStatus()}`}>
                  <i aria-hidden="true" />
                  {streamStatus() === 'live'
                    ? 'Live'
                    : streamStatus() === 'connecting'
                      ? 'Connecting'
                      : 'Reconnecting'}
                </span>
              </div>
              <div class="import-console__output" role="log" aria-live="polite">
                <Show
                  when={entries().length > 0}
                  fallback={
                    <p class="import-console__empty">
                      Waiting for metadata activity…
                    </p>
                  }
                >
                  <For each={entries()}>
                    {(entry) => (
                      <div class={`console-entry is-${entry.state}`}>
                        <p>
                          <time dateTime={entry.created_at}>
                            {new Date(entry.created_at).toLocaleTimeString()}
                          </time>
                          <span>{entry.state}</span>
                          <strong title={entry.name}>{entry.name}</strong>
                          <small>
                            {entry.extractor_name ?? 'metadata'}
                            {entry.attempt > 0 ? ` · try ${entry.attempt}` : ''}
                            {entry.duration_ms !== null
                              ? ` · ${formatDuration(entry.duration_ms)}`
                              : ''}
                          </small>
                        </p>
                        <Show when={entry.error}>
                          <p class="console-entry__error">{entry.error}</p>
                        </Show>
                      </div>
                    )}
                  </For>
                </Show>
              </div>
            </section>
          </article>
        )}
      </Show>
    </section>
  )
}

function Count(props: { label: string; value: number; error?: boolean }) {
  return (
    <div classList={{ 'is-error': props.error && props.value > 0 }}>
      <dt>{props.label}</dt>
      <dd>{props.value.toLocaleString()}</dd>
    </div>
  )
}

function formatEta(seconds: number | null): string {
  if (seconds === null) return 'ETA unavailable'
  if (seconds < 60) return 'less than a minute remaining'
  const hours = seconds / 3600
  if (hours < 1) return `${Math.ceil(seconds / 60)} min remaining`
  if (hours < 48) return `${hours.toFixed(hours >= 10 ? 0 : 1)} hr remaining`
  return `${(hours / 24).toFixed(1)} days remaining`
}

function formatDuration(milliseconds: number): string {
  if (milliseconds < 1000) return `${milliseconds}ms`
  return `${(milliseconds / 1000).toFixed(milliseconds >= 10_000 ? 0 : 1)}s`
}

function parseEvent(value: string): MetadataEvent | undefined {
  try {
    return JSON.parse(value) as MetadataEvent
  } catch {
    return undefined
  }
}

function parseStatus(value: string): MetadataStatus | undefined {
  try {
    return JSON.parse(value) as MetadataStatus
  } catch {
    return undefined
  }
}

const sampleStatus: MetadataStatus = {
  counts: {
    pending: 451_713,
    running: 1,
    completed: 226_699,
    failed: 371,
    skipped: 0,
    cancelled: 0,
    remaining: 451_714,
    total: 678_784,
  },
  completed_per_hour: 7_180,
  eta_seconds: 226_488,
}

const sampleEvents: MetadataEvent[] = [
  {
    id: 3,
    job_id: '00000000-0000-0000-0000-000000000013',
    node_id: '00000000-0000-0000-0000-000000000023',
    name: 'archive-photo.jpg',
    state: 'completed',
    attempt: 1,
    extractor_name: 'exiftool',
    duration_ms: 842,
    error: null,
    created_at: '2026-08-03T12:03:00Z',
  },
  {
    id: 2,
    job_id: '00000000-0000-0000-0000-000000000012',
    node_id: '00000000-0000-0000-0000-000000000022',
    name: 'project-notes.pdf',
    state: 'running',
    attempt: 1,
    extractor_name: null,
    duration_ms: null,
    error: null,
    created_at: '2026-08-03T12:02:59Z',
  },
]
