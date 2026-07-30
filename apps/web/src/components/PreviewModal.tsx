import {
  createResource,
  createSignal,
  Match,
  onCleanup,
  onMount,
  Show,
  Switch,
} from 'solid-js'
import { getFileDetails, prepareFilePreview } from '../api/client'
import type { FileDetails, FolderItem } from '../api/types'
import { formatFileSize } from './FileTable'
import './PreviewModal.css'

interface PreviewModalProps {
  item: FolderItem
  files: FolderItem[]
  staticDetails?: FileDetails
  staticSource?: string
  onNavigate: (item: FolderItem) => void
  onClose: () => void
}

export function PreviewModal(props: PreviewModalProps) {
  const [zoom, setZoom] = createSignal(1)
  const [playbackError, setPlaybackError] = createSignal(false)
  const [details] = createResource(
    () => (props.staticDetails ? false : props.item.id),
    (id) => getFileDetails(id),
  )
  const file = () => props.staticDetails ?? details()
  const [source] = createResource(
    () => props.item.id,
    async (id) => {
      setPlaybackError(false)
      setZoom(1)
      if (props.staticDetails) {
        await new Promise((resolve) => window.setTimeout(resolve, 260))
        return props.staticSource ?? ''
      }
      return prepareFilePreview(id)
    },
  )
  let dialog: HTMLDivElement | undefined

  const navigate = (offset: number) => {
    const index = props.files.findIndex((item) => item.id === props.item.id)
    const next = props.files[index + offset]
    if (next) props.onNavigate(next)
  }

  const keydown = (event: KeyboardEvent) => {
    if (event.key === 'Escape') props.onClose()
    if (event.key === 'ArrowLeft') navigate(-1)
    if (event.key === 'ArrowRight') navigate(1)
  }
  onMount(() => {
    window.addEventListener('keydown', keydown)
    dialog?.focus()
  })
  onCleanup(() => window.removeEventListener('keydown', keydown))

  const mime = () => file()?.detected_mime ?? mimeFromName(props.item.name)
  const kind = () => file()?.media_kind ?? kindFromMime(mime())
  const unsupported = () =>
    playbackError() || !['image', 'video', 'audio', 'document'].includes(kind())

  return (
    <div
      class="preview-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) props.onClose()
      }}
    >
      <div
        ref={dialog}
        class="preview-modal"
        role="dialog"
        aria-modal="true"
        aria-label={`Preview ${props.item.name}`}
        tabIndex={-1}
      >
        <header class="preview-modal__header">
          <div>
            <p>Preview</p>
            <h2>{props.item.name}</h2>
          </div>
          <button
            type="button"
            aria-label="Close preview"
            onClick={() => props.onClose()}
          >
            ×
          </button>
        </header>

        <main class="preview-modal__stage">
          <Show
            when={!source.loading && !details.loading}
            fallback={
              <div class="preview-modal__loading" role="status">
                <span aria-hidden="true" />
                Generating preview…
              </div>
            }
          >
            <Show
              when={!source.error && !unsupported()}
              fallback={
                <UnsupportedPreview
                  item={props.item}
                  message={source.error?.message}
                />
              }
            >
              <Switch>
                <Match when={kind() === 'image'}>
                  <div class="preview-modal__image-viewport">
                    <img
                      src={source()}
                      alt={props.item.name}
                      style={{ transform: `scale(${zoom()})` }}
                      onError={() => setPlaybackError(true)}
                    />
                  </div>
                  <div
                    class="preview-modal__zoom"
                    aria-label="Image zoom controls"
                  >
                    <button
                      type="button"
                      onClick={() =>
                        setZoom((value) => Math.max(0.5, value - 0.25))
                      }
                    >
                      −
                    </button>
                    <span>{Math.round(zoom() * 100)}%</span>
                    <button
                      type="button"
                      onClick={() =>
                        setZoom((value) => Math.min(3, value + 0.25))
                      }
                    >
                      +
                    </button>
                  </div>
                </Match>
                <Match when={kind() === 'video'}>
                  <video
                    src={source() || undefined}
                    poster={props.staticDetails ? demoImage : undefined}
                    controls
                    onError={() => setPlaybackError(true)}
                  />
                </Match>
                <Match when={kind() === 'audio'}>
                  <div class="preview-modal__audio">
                    <div aria-hidden="true">♫</div>
                    <audio
                      src={source() || undefined}
                      controls
                      onError={() => setPlaybackError(true)}
                    />
                  </div>
                </Match>
                <Match when={kind() === 'document'}>
                  <iframe
                    src={source()}
                    title={`Document preview: ${props.item.name}`}
                  />
                </Match>
              </Switch>
            </Show>
          </Show>
        </main>

        <button
          class="preview-modal__nav preview-modal__nav--previous"
          type="button"
          aria-label="Previous file"
          disabled={props.files[0]?.id === props.item.id}
          onClick={() => navigate(-1)}
        >
          ‹
        </button>
        <button
          class="preview-modal__nav preview-modal__nav--next"
          type="button"
          aria-label="Next file"
          disabled={props.files.at(-1)?.id === props.item.id}
          onClick={() => navigate(1)}
        >
          ›
        </button>
      </div>
    </div>
  )
}

function UnsupportedPreview(props: { item: FolderItem; message?: string }) {
  return (
    <div class="preview-modal__unsupported">
      <div aria-hidden="true">
        {props.item.name.split('.').at(-1)?.toUpperCase() ?? 'FILE'}
      </div>
      <h3>Preview unavailable</h3>
      <p>
        {props.message ??
          `${formatFileSize(props.item.size_bytes, 'file')} · This format opens after download.`}
      </p>
      <a href={`/api/files/${props.item.id}/download`}>Download</a>
    </div>
  )
}

function mimeFromName(name: string): string {
  const extension = name.split('.').at(-1)?.toLowerCase()
  return (
    (
      {
        jpg: 'image/jpeg',
        jpeg: 'image/jpeg',
        png: 'image/png',
        gif: 'image/gif',
        nef: 'image/x-nikon-nef',
        mp4: 'video/mp4',
        mp3: 'audio/mpeg',
        pdf: 'application/pdf',
        zip: 'application/zip',
        docx: 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
      } as Record<string, string>
    )[extension ?? ''] ?? 'application/octet-stream'
  )
}

function kindFromMime(mime: string): NonNullable<FileDetails['media_kind']> {
  if (mime.startsWith('image/')) return 'image'
  if (mime.startsWith('video/')) return 'video'
  if (mime.startsWith('audio/')) return 'audio'
  if (mime === 'application/pdf' || mime.includes('wordprocessingml'))
    return 'document'
  return 'other'
}

export const demoImage = `data:image/svg+xml,${encodeURIComponent(`
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 760">
  <defs><linearGradient id="sky" x2="0" y2="1"><stop stop-color="#425d89"/><stop offset=".52" stop-color="#f0a068"/><stop offset="1" stop-color="#182431"/></linearGradient></defs>
  <rect width="1200" height="760" fill="url(#sky)"/><circle cx="790" cy="310" r="82" fill="#ffd79c" opacity=".9"/>
  <path d="M0 420 210 245 390 410 560 210 795 430 1010 265 1200 420V760H0Z" fill="#172631"/>
  <path d="M0 475 Q300 430 600 475T1200 475V760H0Z" fill="#294c61"/><path d="M0 500 Q300 455 600 500T1200 500" fill="none" stroke="#f7bf80" stroke-width="14" opacity=".35"/>
</svg>`)}`
