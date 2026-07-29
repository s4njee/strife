import { For, Show } from 'solid-js'
import { useUploads, type UploadPanelItem } from '../uploads/UploadContext'
import './UploadProgressPanel.css'

export function UploadProgressPanel() {
  const uploads = useUploads()

  return (
    <Show when={uploads.items().length > 0}>
      <aside class="upload-panel" aria-label="Uploads">
        <header>
          <strong>Uploads</strong>
          <span>{uploads.items().length}</span>
        </header>
        <div class="upload-panel__items">
          <For each={uploads.items()}>
            {(item) => <UploadProgressItem item={item} />}
          </For>
        </div>
      </aside>
    </Show>
  )
}

function UploadProgressItem(props: { item: UploadPanelItem }) {
  const uploads = useUploads()
  let resumeInput!: HTMLInputElement
  const percentage = () =>
    props.item.totalBytes === 0
      ? props.item.state === 'completed'
        ? 100
        : 0
      : Math.min(
          100,
          Math.round((props.item.receivedBytes / props.item.totalBytes) * 100),
        )
  const handleResume = () => {
    const file = resumeInput.files?.[0]
    resumeInput.value = ''
    if (file) void uploads.resumeWithFile(props.item.id, file)
  }

  return (
    <article class="upload-panel__item" data-state={props.item.state}>
      <div class="upload-panel__summary">
        <strong title={props.item.path}>{props.item.name}</strong>
        <span>{statusLabel(props.item)}</span>
      </div>
      <progress
        value={percentage()}
        max="100"
        aria-label={`${props.item.name} upload progress`}
      />
      <div class="upload-panel__meta">
        <span>
          {formatBytes(props.item.receivedBytes)} /{' '}
          {formatBytes(props.item.totalBytes)}
        </span>
        <Show when={props.item.state === 'uploading'}>
          <span>{estimatedTime(props.item)}</span>
        </Show>
      </div>
      <Show when={props.item.error}>
        <p class="upload-panel__error" role="alert">
          {props.item.error}
        </p>
      </Show>
      <div class="upload-panel__actions">
        <Show when={props.item.state === 'needs_file'}>
          <input
            ref={resumeInput}
            class="upload-panel__file"
            type="file"
            aria-label={`Choose ${props.item.name} to resume`}
            onChange={handleResume}
          />
          <button type="button" onClick={() => resumeInput.click()}>
            Select file
          </button>
        </Show>
        <Show
          when={
            props.item.state === 'uploading' ||
            props.item.state === 'needs_file'
          }
        >
          <button
            type="button"
            onClick={() => void uploads.cancel(props.item.id)}
          >
            Cancel
          </button>
        </Show>
      </div>
    </article>
  )
}

function statusLabel(item: UploadPanelItem): string {
  switch (item.state) {
    case 'uploading':
      return `${Math.round((item.receivedBytes / Math.max(1, item.totalBytes)) * 100)}%`
    case 'needs_file':
      return 'Paused'
    case 'completed':
      return 'Complete'
    case 'error':
      return 'Failed'
  }
}

function estimatedTime(item: UploadPanelItem): string {
  const elapsedSeconds = Math.max(1, (Date.now() - item.startedAt) / 1000)
  const bytesPerSecond = item.receivedBytes / elapsedSeconds
  if (bytesPerSecond <= 0) return 'Estimating…'
  const seconds = Math.ceil(
    (item.totalBytes - item.receivedBytes) / bytesPerSecond,
  )
  return seconds <= 1 ? 'Almost done' : `${seconds}s remaining`
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}
