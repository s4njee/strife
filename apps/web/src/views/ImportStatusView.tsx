import {
  createEffect,
  createResource,
  createSignal,
  For,
  onCleanup,
  onMount,
  Show,
} from 'solid-js'
import {
  getImportEntries,
  getImportSources,
  retryImportEntry,
  scanImportSource,
  setImportSourceEnabled,
} from '../api/client'
import type { ImportEntry, ImportScanResult, ImportSource } from '../api/types'
import './ImportStatusView.css'

const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'

export function ImportStatusView() {
  const [refreshVersion, setRefreshVersion] = createSignal(0)
  const [previewSources, setPreviewSources] = createSignal(sampleSources)
  const [previewFailures, setPreviewFailures] = createSignal(sampleFailures)
  const [busyId, setBusyId] = createSignal<string>()
  const [notice, setNotice] = createSignal<string>()
  const [sources] = createResource(
    () => (staticPreview ? false : refreshVersion()),
    () => getImportSources(),
  )
  const visibleSources = () =>
    staticPreview ? previewSources() : (sources() ?? [])
  const sourceId = () => visibleSources()[0]?.id
  const [failures] = createResource(
    () => (staticPreview ? false : `${sourceId() ?? ''}:${refreshVersion()}`),
    (key) => {
      const id = key.split(':')[0]
      return id ? getImportEntries(id, 'failed') : Promise.resolve([])
    },
  )
  const visibleFailures = () =>
    staticPreview ? previewFailures() : (failures() ?? [])

  onMount(() => {
    const interval = window.setInterval(
      () => setRefreshVersion((version) => version + 1),
      30_000,
    )
    onCleanup(() => window.clearInterval(interval))
  })
  createEffect(() => {
    if (sources.error instanceof Error) setNotice(sources.error.message)
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
        await setImportSourceEnabled(source.id, !source.enabled)
        setRefreshVersion((version) => version + 1)
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
    try {
      const result: ImportScanResult = staticPreview
        ? {
            discovered: 3,
            imported: 2,
            failed: 1,
            skipped_hidden: 0,
            skipped_special: 0,
          }
        : await scanImportSource(source.id)
      setNotice(
        `Scan complete: ${result.imported} imported, ${result.failed} failed.`,
      )
      setRefreshVersion((version) => version + 1)
    } catch (error) {
      setNotice(error instanceof Error ? error.message : 'Scan failed.')
    } finally {
      setBusyId(undefined)
    }
  }

  const retry = async (entry: ImportEntry) => {
    const id = sourceId()
    if (!id) return
    setBusyId(entry.id)
    setNotice(undefined)
    try {
      if (staticPreview) {
        setPreviewFailures((items) =>
          items.filter((item) => item.id !== entry.id),
        )
      } else {
        await retryImportEntry(id, entry.id)
        setRefreshVersion((version) => version + 1)
      }
      setNotice(`${entry.source_path} is ready to retry on the next scan.`)
    } catch (error) {
      setNotice(error instanceof Error ? error.message : 'Retry failed.')
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
                  <div class="import-source__actions">
                    <button
                      type="button"
                      disabled={busyId() === source.id}
                      onClick={() => void scan(source)}
                    >
                      {busyId() === source.id ? 'Working…' : 'Scan now'}
                    </button>
                    <button
                      class="secondary"
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
              </article>
            )}
          </For>
        </Show>
      </div>

      <section class="import-errors" aria-labelledby="import-errors-title">
        <div class="import-errors__heading">
          <div>
            <p class="workspace-view__eyebrow">Needs attention</p>
            <h2 id="import-errors-title">Import errors</h2>
          </div>
          <span>{visibleFailures().length}</span>
        </div>
        <Show
          when={visibleFailures().length > 0}
          fallback={
            <p class="import-errors__empty">No unresolved import errors.</p>
          }
        >
          <div class="import-errors__list">
            <For each={visibleFailures()}>
              {(entry) => (
                <article class="import-error">
                  <div>
                    <h3>{entry.source_path}</h3>
                    <p>{entry.error_message ?? 'Import failed.'}</p>
                    <time dateTime={entry.updated_at}>
                      {new Date(entry.updated_at).toLocaleString()}
                    </time>
                  </div>
                  <button
                    type="button"
                    disabled={busyId() === entry.id}
                    onClick={() => void retry(entry)}
                  >
                    {busyId() === entry.id ? 'Retrying…' : 'Retry'}
                  </button>
                </article>
              )}
            </For>
          </div>
        </Show>
      </section>
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

const sampleFailures: ImportEntry[] = [
  {
    id: '30000000-0000-0000-0000-000000000001',
    source_path: 'photos/2026/family.jpg',
    source_size: 4_821_771,
    source_modified_at: '2026-07-29T22:39:00Z',
    state: 'failed',
    resulting_node_id: null,
    error_message: 'An active sibling already has this name',
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
