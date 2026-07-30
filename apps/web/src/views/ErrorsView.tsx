import { createResource, For, Show } from 'solid-js'
import {
  getImportEntries,
  getImportSources,
  retryImportEntry,
} from '../api/client'
import type { ImportEntry } from '../api/types'
import './ErrorsView.css'

const DEFAULT_SOURCE = '00000000-0000-0000-0000-000000000003'

export function ErrorsView() {
  const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'
  const [entries, { refetch }] = createResource(async () => {
    if (staticPreview) {
      return [
        {
          id: 'err-1',
          source_path: 'photos/duplicate.jpg',
          error_message: 'Name conflict: photos/duplicate.jpg already exists',
          updated_at: '2026-07-28T12:00:00Z',
          state: 'failed',
        },
      ] as ImportEntry[]
    }
    try {
      const sources = await getImportSources()
      const sourceId = sources[0]?.id ?? DEFAULT_SOURCE
      return getImportEntries(sourceId, 'failed')
    } catch {
      return getImportEntries(DEFAULT_SOURCE, 'failed')
    }
  })

  const retry = async (entry: ImportEntry) => {
    if (staticPreview) return
    const sources = await getImportSources()
    const sourceId = sources[0]?.id ?? DEFAULT_SOURCE
    await retryImportEntry(sourceId, entry.id)
    await refetch()
  }

  return (
    <section class="errors-view">
      <header>
        <p class="workspace-view__eyebrow">Operations</p>
        <h1>Errors</h1>
        <p class="workspace-view__description">
          Persistent import conflicts and failures that need attention.
        </p>
      </header>
      <div class="errors-view__surface">
        <Show when={!entries.loading} fallback={<p>Loading errors…</p>}>
          <Show
            when={(entries() ?? []).length > 0}
            fallback={
              <p class="errors-view__empty">No unresolved errors.</p>
            }
          >
            <ul class="errors-view__list">
              <For each={entries() ?? []}>
                {(entry) => (
                  <li class="errors-view__item">
                    <div>
                      <strong>{entry.source_path}</strong>
                      <p>{entry.error_message ?? 'Import failed'}</p>
                      <time datetime={entry.updated_at}>
                        {new Date(entry.updated_at).toLocaleString()}
                      </time>
                    </div>
                    <button type="button" onClick={() => void retry(entry)}>
                      Retry
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </Show>
      </div>
    </section>
  )
}
