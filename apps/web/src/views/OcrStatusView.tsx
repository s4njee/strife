import {
  createEffect,
  createResource,
  createSignal,
  For,
  onCleanup,
  Show,
} from 'solid-js'
import { getOcrStatus, reprocessOcr } from '../api/client'
import type { OcrEvent, OcrStatus } from '../api/types'
import './ImportStatusView.css'
import './OcrStatusView.css'

const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'

export function OcrStatusView() {
  const [notice, setNotice] = createSignal<string>()
  const [busy, setBusy] = createSignal(false)
  const [entries, setEntries] = createSignal<OcrEvent[]>(
    staticPreview ? sampleEntries : [],
  )
  const [streamStatus, setStreamStatus] = createSignal<
    'connecting' | 'live' | 'reconnecting'
  >(staticPreview ? 'live' : 'connecting')
  const [status, { mutate }] = createResource(
    () => !staticPreview,
    () => getOcrStatus(),
  )
  const current = () => (staticPreview ? sampleStatus : status())

  createEffect(() => {
    if (status.error instanceof Error) setNotice(status.error.message)
  })
  createEffect(() => {
    if (staticPreview) return
    const events = new EventSource('/api/ocr/events')
    events.onopen = () => setStreamStatus('live')
    events.onerror = () => setStreamStatus('reconnecting')
    events.addEventListener('entry', (event) => {
      const parsed = parseEvent((event as MessageEvent<string>).data)
      if (parsed) setEntries((items) => [parsed, ...items.slice(0, 199)])
    })
    events.addEventListener('status', (event) => {
      const parsed = parseStatus((event as MessageEvent<string>).data)
      if (parsed) mutate(parsed)
    })
    onCleanup(() => events.close())
  })

  const reprocess = async (scope: 'failed' | 'node', nodeId?: string) => {
    if (
      scope === 'failed' &&
      !window.confirm('Reprocess every failed OCR file?')
    )
      return
    setBusy(true)
    setNotice(undefined)
    try {
      const enqueued = staticPreview
        ? scope === 'failed'
          ? sampleStatus.counts.failed
          : 1
        : await reprocessOcr({ scope, nodeId })
      setNotice(`${enqueued} OCR job${enqueued === 1 ? '' : 's'} queued.`)
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : 'OCR reprocessing failed.',
      )
    } finally {
      setBusy(false)
    }
  }

  return (
    <section class="workspace-view import-view ocr-view">
      <header>
        <p class="workspace-view__eyebrow">Processing</p>
        <h1>OCR</h1>
        <p class="workspace-view__description">
          Extracted document text and live recognition activity.
        </p>
      </header>

      <Show when={notice()}>
        {(message) => (
          <p class="import-view__notice" role="status">
            {message()}
          </p>
        )}
      </Show>

      <Show when={current()} fallback={<p>Loading OCR status…</p>}>
        {(data) => (
          <article class="import-source">
            <div class="import-source__heading">
              <div>
                <span class="import-source__status">
                  {data().counts.remaining} remaining
                </span>
                <h2>{data().engine_name ?? 'OCR engine unavailable'}</h2>
                <p>
                  {data().engine_version ?? 'No worker has reported a version'}
                  {data().language ? ` · ${data().language}` : ''}
                </p>
              </div>
              <div class="folder-toolbar import-source__actions">
                <button
                  class="btn--primary"
                  type="button"
                  disabled={busy() || data().counts.failed === 0}
                  onClick={() => void reprocess('failed')}
                >
                  Reprocess failed
                </button>
              </div>
            </div>
            <dl class="import-source__counts ocr-view__counts">
              <Count label="Pending" value={data().counts.pending} />
              <Count label="Running" value={data().counts.running} />
              <Count label="Completed" value={data().counts.completed} />
              <Count label="Failed" value={data().counts.failed} error />
              <Count label="Skipped" value={data().counts.skipped} />
              <Count label="Unsupported" value={data().counts.unsupported} />
            </dl>
            <section class="import-console" aria-labelledby="ocr-console-title">
              <div class="import-console__heading">
                <h3 id="ocr-console-title">OCR activity</h3>
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
                      Waiting for OCR activity…
                    </p>
                  }
                >
                  <For each={entries()}>
                    {(entry) => (
                      <div class={`ocr-console__entry is-${entry.state}`}>
                        <p>
                          <time dateTime={entry.created_at}>
                            {new Date(entry.created_at).toLocaleTimeString()}
                          </time>
                          <span>{entry.state}</span>
                          <strong>{entry.name}</strong>
                          <Show when={entry.page_count !== null}>
                            <small>{entry.page_count} pages</small>
                          </Show>
                          <Show when={entry.mean_confidence !== null}>
                            <small>{entry.mean_confidence?.toFixed(1)}%</small>
                          </Show>
                          <Show
                            when={entry.state === 'failed' && entry.node_id}
                          >
                            <button
                              type="button"
                              disabled={busy()}
                              onClick={() =>
                                void reprocess('node', entry.node_id!)
                              }
                            >
                              Retry
                            </button>
                          </Show>
                        </p>
                        <Show when={entry.warning}>
                          <p class="ocr-console__warning">{entry.warning}</p>
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
      <dd>{props.value}</dd>
    </div>
  )
}

function parseEvent(value: string): OcrEvent | undefined {
  try {
    return JSON.parse(value) as OcrEvent
  } catch {
    return undefined
  }
}

function parseStatus(value: string): OcrStatus | undefined {
  try {
    return JSON.parse(value) as OcrStatus
  } catch {
    return undefined
  }
}

const sampleStatus: OcrStatus = {
  counts: {
    pending: 14,
    running: 1,
    completed: 842,
    failed: 2,
    skipped: 319,
    unsupported: 7,
    remaining: 15,
  },
  engine_name: 'tesseract',
  engine_version: 'tesseract 5.5.0',
  language: 'eng',
}

const sampleEntries: OcrEvent[] = [
  {
    id: 3,
    node_id: 'preview-3',
    name: 'receipt-scan.pdf',
    state: 'completed',
    page_count: 2,
    mean_confidence: 94.2,
    warning: null,
    created_at: '2026-08-02T18:42:15Z',
  },
  {
    id: 2,
    node_id: 'preview-2',
    name: 'archive.tiff',
    state: 'failed',
    page_count: null,
    mean_confidence: null,
    warning: 'OCR page limit exceeded: 140 pages is greater than 100',
    created_at: '2026-08-02T18:41:09Z',
  },
  {
    id: 1,
    node_id: 'preview-1',
    name: 'manual.pdf',
    state: 'skipped',
    page_count: 38,
    mean_confidence: null,
    warning: null,
    created_at: '2026-08-02T18:40:31Z',
  },
]
