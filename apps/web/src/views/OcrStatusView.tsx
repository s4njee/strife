import {
  createEffect,
  createResource,
  createSignal,
  For,
  onCleanup,
  Show,
} from 'solid-js'
import {
  actOnBackfill,
  createOcrCampaign,
  getBackfillMetrics,
  getOcrPreflight,
  getOcrStatus,
  listBackfills,
  listBackfillCanaryResults,
  reprocessOcr,
} from '../api/client'
import type {
  BackfillCampaign,
  OcrEvent,
  OcrPreflight,
  OcrStatus,
} from '../api/types'
import './ImportStatusView.css'
import './OcrStatusView.css'

const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'

const BATCH_SIZE = 100
const MAX_QUEUED = 100
const MAX_RUNNING = 1
const INITIAL_CANARY_LIMIT = 100

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

function formatThroughput(value: number | null): string {
  return value === null ? 'measuring throughput' : `${value.toFixed(1)}/hour`
}

function formatEta(value: string | null): string {
  return value ? ` · ETA ${new Date(value).toLocaleString()}` : ''
}

/** Campaign states in which historical work can still be claimed. */
function isActive(campaign: BackfillCampaign): boolean {
  return (
    campaign.state === 'running' ||
    campaign.state === 'paused' ||
    campaign.state === 'draining'
  )
}

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
  const [preflight, setPreflight] = createSignal<OcrPreflight>()
  const [campaigns, { refetch: refetchCampaigns }] = createResource(
    () => !staticPreview,
    () => listBackfills(),
  )
  const ocrCampaign = (): BackfillCampaign | undefined => {
    if (staticPreview) return sampleCampaign
    return campaigns()?.find(
      (campaign) => campaign.kind === 'ocr' && isActive(campaign),
    )
  }
  const [campaignMetrics, { refetch: refetchCampaignMetrics }] = createResource(
    () => (!staticPreview ? ocrCampaign()?.id : undefined),
    (id) => getBackfillMetrics(id),
  )
  const [canaryResults, { refetch: refetchCanaryResults }] = createResource(
    () => (!staticPreview ? ocrCampaign()?.id : undefined),
    (id) => listBackfillCanaryResults(id),
  )

  createEffect(() => {
    if (status.error instanceof Error) setNotice(status.error.message)
  })
  createEffect(() => {
    if (staticPreview || !ocrCampaign()?.id) return
    const timer = window.setInterval(() => {
      void refetchCampaignMetrics()
      void refetchCanaryResults()
    }, 15_000)
    onCleanup(() => window.clearInterval(timer))
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

  const runPreflight = async () => {
    setBusy(true)
    setNotice(undefined)
    try {
      const report = staticPreview ? samplePreflight : await getOcrPreflight()
      setPreflight(report)
      setNotice(
        `Preflight found ${report.candidates} candidate${report.candidates === 1 ? '' : 's'} (${formatBytes(report.total_candidate_bytes)}). No work was queued.`,
      )
    } catch (error) {
      setNotice(error instanceof Error ? error.message : 'Preflight failed.')
    } finally {
      setBusy(false)
    }
  }

  const startCampaign = async () => {
    const report = preflight()
    if (!report) return
    if (
      !window.confirm(
        `Create a paused OCR campaign for ${report.candidates} files (${formatBytes(report.total_candidate_bytes)})?\n\n` +
          `Canary ${INITIAL_CANARY_LIMIT} · batch ${BATCH_SIZE} · max ${MAX_QUEUED} queued · ${MAX_RUNNING} running · heavy_cpu.\n\n` +
          'The campaign starts paused. Nothing runs until you resume it.',
      )
    )
      return
    setBusy(true)
    setNotice(undefined)
    try {
      if (!staticPreview) {
        await createOcrCampaign({
          candidateCount: report.candidates,
          snapshotBefore: report.snapshot_before,
          batchSize: BATCH_SIZE,
          maxQueued: MAX_QUEUED,
          maxRunning: MAX_RUNNING,
          canaryLimit: INITIAL_CANARY_LIMIT,
        })
        await refetchCampaigns()
      }
      setNotice('Campaign created and paused. Resume it to begin processing.')
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : 'Campaign creation failed.',
      )
    } finally {
      setBusy(false)
    }
  }

  const campaignAction = async (
    campaign: BackfillCampaign,
    action: 'resume' | 'pause' | 'cancel',
  ) => {
    if (
      action === 'cancel' &&
      !window.confirm(
        'Cancel this campaign? Queued historical work is dropped. Completed OCR text is kept and leased jobs finish.',
      )
    )
      return
    if (
      action === 'resume' &&
      !window.confirm(
        `Resume historical OCR for ${campaign.candidate_count} files?\n\n` +
          'This starts sustained CPU work. Foreground uploads keep priority.',
      )
    )
      return
    setBusy(true)
    setNotice(undefined)
    try {
      if (!staticPreview) {
        await actOnBackfill(campaign.id, action)
        await refetchCampaigns()
        await Promise.all([refetchCampaignMetrics(), refetchCanaryResults()])
      }
      setNotice(`Campaign ${action}d.`)
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : `Campaign ${action} failed.`,
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
            <section class="ocr-campaign" aria-labelledby="ocr-campaign-title">
              <div class="import-console__heading">
                <h3 id="ocr-campaign-title">Historical backfill</h3>
                <span
                  class={`ocr-campaign__state is-${ocrCampaign()?.state ?? 'none'}`}
                >
                  {ocrCampaign()?.state ?? 'no campaign'}
                </span>
              </div>

              <Show
                when={ocrCampaign()}
                fallback={
                  <div class="ocr-campaign__setup">
                    <p>
                      New uploads are OCR'd automatically. Files already in the
                      library are only processed through an explicit campaign.
                    </p>
                    <Show when={preflight()}>
                      {(report) => (
                        <dl class="import-source__counts ocr-campaign__counts">
                          <Count
                            label="Candidates"
                            value={report().candidates}
                          />
                          <Count
                            label="Awaiting metadata"
                            value={report().awaiting_metadata}
                          />
                          <Count
                            label="Already done"
                            value={report().already_completed}
                          />
                          <Count
                            label="Already failed"
                            value={report().already_failed}
                            error
                          />
                        </dl>
                      )}
                    </Show>
                    <Show when={preflight()}>
                      {(report) => (
                        <Show when={report().families.length > 0}>
                          <table class="ocr-campaign__families">
                            <caption class="sr-only">
                              Candidate files by detected type
                            </caption>
                            <thead>
                              <tr>
                                <th scope="col">Type</th>
                                <th scope="col">Files</th>
                                <th scope="col">Total</th>
                                <th scope="col">p50</th>
                                <th scope="col">p95</th>
                              </tr>
                            </thead>
                            <tbody>
                              <For each={report().families}>
                                {(family) => (
                                  <tr>
                                    <td>{family.detected_mime}</td>
                                    <td>{family.candidates}</td>
                                    <td>{formatBytes(family.total_bytes)}</td>
                                    <td>{formatBytes(family.p50_bytes)}</td>
                                    <td>{formatBytes(family.p95_bytes)}</td>
                                  </tr>
                                )}
                              </For>
                            </tbody>
                          </table>
                        </Show>
                      )}
                    </Show>
                    <div class="folder-toolbar">
                      <button
                        type="button"
                        disabled={busy()}
                        onClick={() => void runPreflight()}
                      >
                        Run preflight
                      </button>
                      <button
                        class="btn--primary"
                        type="button"
                        disabled={busy() || !preflight()}
                        onClick={() => void startCampaign()}
                      >
                        Create paused campaign
                      </button>
                    </div>
                  </div>
                }
              >
                {(campaign) => (
                  <div class="ocr-campaign__active">
                    <dl class="import-source__counts ocr-campaign__counts">
                      <Count
                        label="Candidates"
                        value={campaign().candidate_count}
                      />
                      <Count label="Queued" value={campaign().enqueued_count} />
                      <Count
                        label="Completed"
                        value={campaign().completed_count}
                      />
                      <Count
                        label="Failed"
                        value={campaign().failed_count}
                        error
                      />
                    </dl>
                    <p class="ocr-campaign__limits">
                      Batch {campaign().batch_size} · max{' '}
                      {campaign().max_queued} queued · {campaign().max_running}{' '}
                      running · {campaign().resource_class} · foreground
                      priority every {campaign().foreground_fairness} claims
                    </p>
                    <Show when={campaignMetrics()}>
                      {(metrics) => (
                        <p class="ocr-campaign__limits">
                          {metrics().pending} pending · {metrics().running}{' '}
                          running · {metrics().remaining} remaining ·{' '}
                          {formatThroughput(metrics().throughput_per_hour)}
                          {formatEta(metrics().estimated_completion_at)}
                        </p>
                      )}
                    </Show>
                    <Show when={(canaryResults()?.length ?? 0) > 0}>
                      <section aria-labelledby="ocr-canary-results-title">
                        <h4 id="ocr-canary-results-title">Canary results</h4>
                        <ul class="ocr-campaign__results">
                          <For each={canaryResults()}>
                            {(result) => (
                              <li>
                                <strong>{result.details.stage} files</strong> ·{' '}
                                {result.details.throughput_per_hour.toFixed(1)}
                                /hour · p95{' '}
                                {result.details.p95_seconds.toFixed(1)}s ·{' '}
                                {formatBytes(result.details.peak_memory_bytes)}{' '}
                                peak memory ·{' '}
                                {result.details.approved ? 'approved' : 'held'}
                              </li>
                            )}
                          </For>
                        </ul>
                      </section>
                    </Show>
                    <Show when={campaign().snapshot_before}>
                      {(snapshot) => (
                        <p class="ocr-campaign__limits">
                          Snapshot{' '}
                          <time dateTime={snapshot()}>
                            {new Date(snapshot()).toLocaleString()}
                          </time>
                          . Files added after this stay foreground work.
                        </p>
                      )}
                    </Show>
                    <Show when={campaign().last_error}>
                      {(error) => <p class="ocr-console__warning">{error()}</p>}
                    </Show>
                    <div class="folder-toolbar">
                      <Show when={campaign().state === 'paused'}>
                        <button
                          class="btn--primary"
                          type="button"
                          disabled={busy()}
                          onClick={() =>
                            void campaignAction(campaign(), 'resume')
                          }
                        >
                          Resume
                        </button>
                      </Show>
                      <Show when={campaign().state === 'running'}>
                        <button
                          type="button"
                          disabled={busy()}
                          onClick={() =>
                            void campaignAction(campaign(), 'pause')
                          }
                        >
                          Pause
                        </button>
                      </Show>
                      <button
                        type="button"
                        disabled={busy()}
                        onClick={() =>
                          void campaignAction(campaign(), 'cancel')
                        }
                      >
                        Cancel
                      </button>
                    </div>
                  </div>
                )}
              </Show>
            </section>

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

const samplePreflight: OcrPreflight = {
  snapshot_before: '2026-08-03T09:00:00Z',
  engine_version: 'tesseract 5.5.0',
  candidates: 691_402,
  already_completed: 842,
  already_skipped: 319,
  already_failed: 2,
  already_unsupported: 7,
  awaiting_metadata: 118,
  total_candidate_bytes: 2_140_000_000_000,
  families: [
    {
      detected_mime: 'application/pdf',
      candidates: 402_118,
      total_bytes: 1_480_000_000_000,
      p50_bytes: 1_800_000,
      p95_bytes: 24_000_000,
      max_bytes: 512_000_000,
    },
    {
      detected_mime: 'image/jpeg',
      candidates: 264_902,
      total_bytes: 610_000_000_000,
      p50_bytes: 2_200_000,
      p95_bytes: 9_400_000,
      max_bytes: 88_000_000,
    },
    {
      detected_mime: 'image/tiff',
      candidates: 24_382,
      total_bytes: 50_000_000_000,
      p50_bytes: 1_400_000,
      p95_bytes: 12_000_000,
      max_bytes: 240_000_000,
    },
  ],
}

const sampleCampaign: BackfillCampaign = {
  id: '00000000-0000-0000-0000-0000000000c1',
  kind: 'ocr',
  state: 'paused',
  snapshot_before: '2026-08-03T09:00:00Z',
  candidate_definition: { canary_limit: 100 },
  cursor_created_at: '2024-02-11T04:12:00Z',
  cursor_node_id: '00000000-0000-0000-0000-0000000000a7',
  batch_size: 100,
  max_queued: 500,
  max_running: 1,
  resource_class: 'heavy_cpu',
  foreground_fairness: 20,
  candidate_count: 691_402,
  enqueued_count: 1_200,
  completed_count: 1_142,
  failed_count: 3,
  skipped_count: 55,
  created_by_version: '0.1.0',
  last_error: null,
  created_at: '2026-08-03T09:00:00Z',
  updated_at: '2026-08-03T09:42:00Z',
  started_at: '2026-08-03T09:05:00Z',
  paused_at: '2026-08-03T09:42:00Z',
  completed_at: null,
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
