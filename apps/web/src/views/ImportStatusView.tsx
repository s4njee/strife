import {
  createEffect,
  createMemo,
  createResource,
  createSignal,
  For,
  onCleanup,
  Show,
} from 'solid-js'
import {
  getImportSources,
  scanImportSource,
  setImportSourceEnabled,
} from '../api/client'
import type { ImportEntry, ImportScanResult, ImportSource } from '../api/types'
import './ImportStatusView.css'

const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'

export function ImportStatusView() {
  const [previewSources, setPreviewSources] = createSignal(sampleSources)
  const [busyId, setBusyId] = createSignal<string>()
  const [notice, setNotice] = createSignal<string>()
  const [consoleEntries, setConsoleEntries] = createSignal<ImportEntry[]>(
    staticPreview ? sampleConsoleEntries : [],
  )
  const [streamStatus, setStreamStatus] = createSignal<
    'connecting' | 'live' | 'reconnecting'
  >(staticPreview ? 'live' : 'connecting')
  const [sources, { mutate: mutateSources }] = createResource(
    () => !staticPreview,
    () => getImportSources(),
  )
  const visibleSources = () =>
    staticPreview ? previewSources() : (sources() ?? [])
  const sourceId = createMemo(() => visibleSources()[0]?.id)

  createEffect(() => {
    if (sources.error instanceof Error) setNotice(sources.error.message)
  })
  createEffect(() => {
    if (staticPreview) return
    const id = sourceId()
    if (!id) return

    setStreamStatus('connecting')
    const events = new EventSource(`/api/import-sources/${id}/events`)
    events.onopen = () => setStreamStatus('live')
    events.onerror = () => setStreamStatus('reconnecting')
    events.addEventListener('entry', (event) => {
      const entry = parseImportEntry((event as MessageEvent<string>).data)
      if (!entry) return
      setConsoleEntries((entries) => [entry, ...entries.slice(0, 199)])
    })
    events.addEventListener('status', (event) => {
      const source = parseImportSource((event as MessageEvent<string>).data)
      if (!source) return
      mutateSources((sources) =>
        (sources ?? []).map((item) => (item.id === source.id ? source : item)),
      )
    })
    onCleanup(() => events.close())
  })

  const toggleSource = async (source: ImportSource) => {
    setBusyId(source.id)
    setNotice(undefined)
    try {
      if (staticPreview) {
        setPreviewSources((items) =>
          items.map((item) =>
            item.id === source.id ? { ...item, enabled: !item.enabled } : item,
          ),
        )
      } else {
        const updated = await setImportSourceEnabled(source.id, !source.enabled)
        mutateSources((sources) =>
          (sources ?? []).map((item) =>
            item.id === updated.id ? updated : item,
          ),
        )
      }
    } catch (error) {
      setNotice(
        error instanceof Error ? error.message : 'Source update failed.',
      )
    } finally {
      setBusyId(undefined)
    }
  }

  const scan = async (source: ImportSource) => {
    setBusyId(source.id)
    setNotice(undefined)
    setConsoleEntries([])
    try {
      const result: ImportScanResult = staticPreview
        ? {
            job_id: 'preview-import-job',
            status: 'pending',
          }
        : await scanImportSource(source.id)
      setNotice(
        result.status === 'leased'
          ? 'Import scan is already running in the background.'
          : 'Import scan queued. It will continue in the background.',
      )
    } catch (error) {
      setNotice(error instanceof Error ? error.message : 'Scan failed.')
    } finally {
      setBusyId(undefined)
    }
  }

  return (
    <section class="workspace-view import-view">
      <header>
        <p class="workspace-view__eyebrow">Ingestion</p>
        <h1>Watched-folder imports</h1>
        <p class="workspace-view__description">
          Move ready files from the server inbox into your drive.
        </p>
      </header>

      <Show when={notice()}>
        {(message) => (
          <p class="import-view__notice" role="status">
            {message()}
          </p>
        )}
      </Show>

      <div class="import-view__sources">
        <Show when={!sources.loading} fallback={<p>Loading import source…</p>}>
          <For
            each={visibleSources()}
            fallback={<p>No import source found.</p>}
          >
            {(source) => (
              <article class="import-source">
                <div class="import-source__heading">
                  <div>
                    <span
                      class="import-source__status"
                      classList={{ 'is-disabled': !source.enabled }}
                    >
                      {source.enabled ? 'Enabled' : 'Disabled'}
                    </span>
                    <h2>{source.watch_path}</h2>
                    <p>Destination: Root folder</p>
                  </div>
                  <div class="folder-toolbar import-source__actions">
                    <button
                      class="btn--primary"
                      type="button"
                      disabled={
                        busyId() === source.id || streamStatus() !== 'live'
                      }
                      onClick={() => void scan(source)}
                    >
                      {busyId() === source.id ? 'Working…' : 'Scan now'}
                    </button>
                    <button
                      type="button"
                      disabled={busyId() === source.id}
                      onClick={() => void toggleSource(source)}
                    >
                      {source.enabled ? 'Disable' : 'Enable'}
                    </button>
                  </div>
                </div>
                <dl class="import-source__counts">
                  <Count label="Discovered" value={source.counts.discovered} />
                  <Count label="Importing" value={source.counts.importing} />
                  <Count label="Imported" value={source.counts.imported} />
                  <Count label="Failed" value={source.counts.failed} error />
                </dl>
                <p class="import-source__last-scan">
                  Last scan:{' '}
                  {source.last_scan_at
                    ? new Date(source.last_scan_at).toLocaleString()
                    : 'Never'}
                </p>
                <section
                  class="import-console"
                  aria-labelledby={`import-console-${source.id}`}
                >
                  <div class="import-console__heading">
                    <h3 id={`import-console-${source.id}`}>Import activity</h3>
                    <span class={`is-${streamStatus()}`}>
                      <i aria-hidden="true" />
                      {streamStatus() === 'live'
                        ? 'Live'
                        : streamStatus() === 'connecting'
                          ? 'Connecting'
                          : 'Reconnecting'}
                    </span>
                  </div>
                  <div
                    class="import-console__output"
                    role="log"
                    aria-live="polite"
                  >
                    <Show
                      when={consoleEntries().length > 0}
                      fallback={
                        <p class="import-console__empty">
                          Waiting for import activity…
                        </p>
                      }
                    >
                      <For each={consoleEntries()}>
                        {(entry) => (
                          <p class={`is-${entry.state}`}>
                            <time dateTime={entry.updated_at}>
                              {new Date(entry.updated_at).toLocaleTimeString()}
                            </time>
                            <span>{consoleLabel(entry.state)}</span>
                            <strong>{entry.source_path}</strong>
                          </p>
                        )}
                      </For>
                    </Show>
                  </div>
                </section>
              </article>
            )}
          </For>
        </Show>
      </div>
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

const sampleSources: ImportSource[] = [
  {
    id: '00000000-0000-0000-0000-000000000003',
    watch_path: '/mnt/ext/watch',
    destination_folder_id: '00000000-0000-0000-0000-000000000001',
    enabled: true,
    last_scan_at: '2026-07-29T22:42:00Z',
    counts: {
      discovered: 3,
      stable: 0,
      importing: 1,
      imported: 128,
      failed: 2,
    },
  },
]

const sampleConsoleEntries: ImportEntry[] = [
  {
    id: '30000000-0000-0000-0000-000000000001',
    source_path: 'photos/2026/family.jpg',
    source_size: 4_821_771,
    source_modified_at: '2026-07-29T22:39:00Z',
    state: 'imported',
    resulting_node_id: '30000000-0000-0000-0000-000000000011',
    error_message: null,
    updated_at: '2026-07-29T22:42:00Z',
  },
  {
    id: '30000000-0000-0000-0000-000000000002',
    source_path: 'documents/taxes.pdf',
    source_size: 1_205_144,
    source_modified_at: '2026-07-29T22:38:00Z',
    state: 'failed',
    resulting_node_id: null,
    error_message: 'Storage does not have enough safe capacity',
    updated_at: '2026-07-29T22:41:00Z',
  },
]

function consoleLabel(state: ImportEntry['state']): string {
  switch (state) {
    case 'discovered':
      return 'FOUND'
    case 'stable':
      return 'STAGED'
    case 'importing':
      return 'IMPORT'
    case 'imported':
      return 'DONE'
    case 'failed':
      return 'FAILED'
  }
}

function parseImportEntry(data: string): ImportEntry | undefined {
  try {
    const value: unknown = JSON.parse(data)
    if (
      typeof value !== 'object' ||
      value === null ||
      !('id' in value) ||
      typeof value.id !== 'string' ||
      !('source_path' in value) ||
      typeof value.source_path !== 'string' ||
      !('state' in value) ||
      !['discovered', 'stable', 'importing', 'imported', 'failed'].includes(
        String(value.state),
      ) ||
      !('updated_at' in value) ||
      typeof value.updated_at !== 'string'
    ) {
      return undefined
    }
    return value as ImportEntry
  } catch {
    return undefined
  }
}

function parseImportSource(data: string): ImportSource | undefined {
  try {
    const value: unknown = JSON.parse(data)
    if (
      typeof value !== 'object' ||
      value === null ||
      !('id' in value) ||
      typeof value.id !== 'string' ||
      !('counts' in value) ||
      typeof value.counts !== 'object' ||
      value.counts === null
    ) {
      return undefined
    }
    return value as ImportSource
  } catch {
    return undefined
  }
}
