import { createResource, createSignal, For, Show } from 'solid-js'
import {
  getAllFileText,
  getFileDetails,
  getFileStreams,
  getFileText,
} from '../api/client'
import type {
  DocumentTextPage,
  FileDetails,
  FileText,
  FolderItem,
  MediaStream,
} from '../api/types'
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
  const [remoteText] = createResource(
    () => (props.staticDetails ? false : props.item.id),
    (fileId) => getFileText(fileId),
  )
  const [morePages, setMorePages] = createSignal<DocumentTextPage[]>([])
  const [nextPage, setNextPage] = createSignal<number | null>()
  const details = () => props.staticDetails ?? remoteDetails()
  const streams = () => props.staticStreams ?? remoteStreams() ?? []
  const text = () => (props.staticDetails ? sampleText : remoteText())
  const textPages = () => [...(text()?.pages ?? []), ...morePages()]

  const loadMoreText = async () => {
    const cursor = nextPage() ?? text()?.next_page
    if (!cursor) return
    const result = await getFileText(props.item.id, cursor)
    setMorePages((pages) => [...pages, ...result.pages])
    setNextPage(result.next_page)
  }

  const copyDocument = async () => {
    const document = props.staticDetails
      ? sampleText
      : await getAllFileText(props.item.id)
    await navigator.clipboard.writeText(
      document.pages.map((page) => page.content).join('\n\n'),
    )
  }

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

            <TextSection
              text={text()}
              pages={textPages()}
              loading={!props.staticDetails && remoteText.loading}
              error={
                remoteText.error instanceof Error
                  ? remoteText.error.message
                  : undefined
              }
              hasMore={
                (nextPage() ?? text()?.next_page) !== null &&
                (nextPage() ?? text()?.next_page) !== undefined
              }
              onLoadMore={() => void loadMoreText()}
              onCopyDocument={() => void copyDocument()}
            />
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

function TextSection(props: {
  text?: FileText
  pages: DocumentTextPage[]
  loading: boolean
  error?: string
  hasMore: boolean
  onLoadMore: () => void
  onCopyDocument: () => void
}) {
  const label = () =>
    ({
      not_processed: 'Not yet processed',
      in_progress: 'OCR in progress',
      completed: 'Extracted text',
      failed: 'OCR failed',
      skipped: 'OCR skipped',
      skipped_embedded: 'Embedded text used; OCR skipped',
      unsupported: 'OCR unsupported',
    })[props.text?.status ?? 'not_processed']
  return (
    <section class="file-details__section file-text">
      <div class="file-text__heading">
        <h3>Extracted text</h3>
        <Show when={props.pages.length > 0}>
          <button type="button" onClick={() => props.onCopyDocument()}>
            Copy document
          </button>
        </Show>
      </div>
      <Show when={!props.loading} fallback={<p>Loading extracted text…</p>}>
        <p
          class={`file-text__status is-${props.text?.status ?? 'not_processed'}`}
        >
          {label()}
        </p>
        <Show when={props.error}>
          <p class="file-text__warning">{props.error}</p>
        </Show>
        <Show when={props.text?.engine_name}>
          <p class="file-text__engine">
            {props.text?.engine_name} · {props.text?.engine_version} ·{' '}
            {props.text?.language}
          </p>
        </Show>
        <For each={props.text?.warnings}>
          {(warning) => <p class="file-text__warning">{warning}</p>}
        </For>
        <div class="file-text__pages">
          <For each={props.pages}>
            {(page) => (
              <article
                classList={{
                  'file-text__page': true,
                  'is-low-confidence':
                    page.confidence !== null && page.confidence < 70,
                }}
              >
                <header>
                  <strong>Page {page.page_number}</strong>
                  <span>
                    {page.confidence === null
                      ? 'Confidence unavailable'
                      : `${page.confidence.toFixed(1)}% confidence`}
                  </span>
                  <button
                    type="button"
                    onClick={() =>
                      void navigator.clipboard.writeText(page.content)
                    }
                  >
                    Copy page
                  </button>
                </header>
                <pre>{page.content || 'No recognized text on this page.'}</pre>
              </article>
            )}
          </For>
        </div>
        <Show when={props.hasMore}>
          <button
            class="file-text__more"
            type="button"
            onClick={() => props.onLoadMore()}
          >
            Load more pages
          </button>
        </Show>
      </Show>
    </section>
  )
}

const sampleText: FileText = {
  status: 'completed',
  source: 'ocr',
  language: 'eng',
  engine_name: 'tesseract',
  engine_version: 'tesseract 5.5.0',
  mean_confidence: 91.8,
  warnings: ['Page 2 has lower recognition confidence.'],
  pages: [
    {
      page_number: 1,
      content: 'Quarterly field report\nSurvey completed on schedule.',
      confidence: 96.4,
      width: 1700,
      height: 2200,
    },
    {
      page_number: 2,
      content: 'Handwritten annotation was not recognized.',
      confidence: 66.2,
      width: 1700,
      height: 2200,
    },
  ],
  next_page: null,
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
