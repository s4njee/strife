import { createResource, For, Show } from 'solid-js'
import { getFileDetails, getFileStreams } from '../api/client'
import type { FileDetails, FolderItem, MediaStream } from '../api/types'
import { formatFileSize } from './FileTable'
import './FileDetailsPanel.css'

interface FileDetailsPanelProps {
  item: FolderItem
  staticDetails?: FileDetails
  staticStreams?: MediaStream[]
  onClose: () => void
}

const dateFormatter = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'medium',
  timeStyle: 'short',
})

export function FileDetailsPanel(props: FileDetailsPanelProps) {
  const [remoteDetails] = createResource(
    () => (props.staticDetails ? false : props.item.id),
    (fileId) => getFileDetails(fileId),
  )
  const [remoteStreams] = createResource(
    () => (props.staticDetails ? false : props.item.id),
    (fileId) => getFileStreams(fileId),
  )
  const details = () => props.staticDetails ?? remoteDetails()
  const streams = () => props.staticStreams ?? remoteStreams() ?? []

  return (
    <aside class="file-details" aria-label={`Details for ${props.item.name}`}>
      <header class="file-details__header">
        <div class="file-details__file-icon" aria-hidden="true">
          {fileGlyph(props.item.name)}
        </div>
        <div>
          <p>File details</p>
          <h2>{props.item.name}</h2>
        </div>
        <button
          type="button"
          class="file-details__close"
          aria-label="Close details"
          onClick={() => props.onClose()}
        >
          ×
        </button>
      </header>

      <Show
        when={details()}
        fallback={<div class="file-details__state">Loading metadata…</div>}
      >
        {(file) => (
          <div class="file-details__content">
            <Status status={file().processing_status} />
            <Section title="General">
              <Fact
                label="Size"
                value={formatFileSize(file().byte_size, 'file')}
              />
              <Fact
                label="MIME type"
                value={file().detected_mime ?? 'Unknown'}
                mono
              />
              <Fact label="Created" value={formatDate(file().created_at)} />
              <Fact label="Modified" value={formatDate(file().updated_at)} />
              <Checksum value={file().checksum_sha256} />
            </Section>

            <Show when={file().media_kind === 'image'}>
              <Section title="Image">
                <Fact label="Dimensions" value={dimensions(file())} />
                <Fact
                  label="Orientation"
                  value={file().orientation?.toString() ?? '—'}
                />
                <Fact
                  label="Camera"
                  value={
                    [file().camera_make, file().camera_model]
                      .filter(Boolean)
                      .join(' ') || '—'
                  }
                />
                <Fact
                  label="Captured"
                  value={formatDate(file().capture_time)}
                />
                <Show when={file().has_gps}>
                  <Fact
                    label="GPS"
                    value={`${file().gps_latitude}, ${file().gps_longitude}`}
                    mono
                  />
                </Show>
              </Section>
            </Show>

            <Show
              when={
                file().media_kind === 'video' || file().media_kind === 'audio'
              }
            >
              <Section title="Media">
                <Fact
                  label="Duration"
                  value={formatDuration(file().duration_ms)}
                />
                <Fact label="Resolution" value={dimensions(file())} />
                <For each={streams()}>
                  {(stream) => <StreamRow stream={stream} />}
                </For>
              </Section>
            </Show>

            <Show when={file().media_kind === 'document'}>
              <Section title="Document">
                <Fact label="Title" value={file().document_title ?? '—'} />
                <Fact label="Author" value={file().document_author ?? '—'} />
                <Fact
                  label="Pages"
                  value={file().page_count?.toString() ?? '—'}
                />
                <Fact
                  label="Created"
                  value={formatDate(file().document_created_at)}
                />
                <Fact
                  label="Modified"
                  value={formatDate(file().document_modified_at)}
                />
              </Section>
            </Show>
          </div>
        )}
      </Show>
      <Show when={remoteDetails.error}>
        <div class="file-details__state file-details__state--error">
          Metadata could not be loaded.
        </div>
      </Show>
    </aside>
  )
}

function Section(props: {
  title: string
  children: import('solid-js').JSX.Element
}) {
  return (
    <section class="file-details__section">
      <h3>{props.title}</h3>
      <dl>{props.children}</dl>
    </section>
  )
}

function Fact(props: { label: string; value: string; mono?: boolean }) {
  return (
    <div class="file-details__fact">
      <dt>{props.label}</dt>
      <dd classList={{ 'is-mono': props.mono }}>{props.value}</dd>
    </div>
  )
}

function Checksum(props: { value: string | null }) {
  const copy = () =>
    props.value && void navigator.clipboard.writeText(props.value)
  return (
    <div class="file-details__fact">
      <dt>Checksum</dt>
      <dd class="file-details__checksum">
        <span>{props.value ? `${props.value.slice(0, 14)}…` : '—'}</span>
        <Show when={props.value}>
          <button type="button" onClick={copy}>
            Copy
          </button>
        </Show>
      </dd>
    </div>
  )
}

function Status(props: { status: FileDetails['processing_status'] }) {
  const label = () =>
    ({
      processing: 'Processing metadata',
      ready: 'Metadata ready',
      partially_processed: 'Partially processed',
      failed: 'Processing failed',
    })[props.status]
  return (
    <div class={`file-details__status is-${props.status}`}>
      <span aria-hidden="true">
        {props.status === 'ready'
          ? '✓'
          : props.status === 'processing'
            ? '◌'
            : '!'}
      </span>
      {label()}
    </div>
  )
}

function StreamRow(props: { stream: MediaStream }) {
  const details = () =>
    [
      props.stream.codec,
      props.stream.width && props.stream.height
        ? `${props.stream.width}×${props.stream.height}`
        : null,
      props.stream.bitrate_bps
        ? `${Math.round(props.stream.bitrate_bps / 1000)} kbps`
        : null,
      props.stream.language,
    ]
      .filter(Boolean)
      .join(' · ')
  return (
    <div class="file-details__stream">
      <strong>
        {props.stream.stream_type} {props.stream.stream_index}
      </strong>
      <span>{details()}</span>
    </div>
  )
}

function dimensions(file: FileDetails) {
  return file.width && file.height ? `${file.width} × ${file.height}` : '—'
}

function formatDate(value: string | null) {
  if (!value) return '—'
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? '—' : dateFormatter.format(date)
}

function formatDuration(milliseconds: number | null) {
  if (milliseconds === null) return '—'
  const totalSeconds = Math.round(milliseconds / 1000)
  return `${Math.floor(totalSeconds / 60)}:${String(totalSeconds % 60).padStart(2, '0')}`
}

function fileGlyph(name: string) {
  const extension = name.split('.').pop()?.toUpperCase()
  return extension && extension.length <= 4 ? extension : 'FILE'
}
