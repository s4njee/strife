import { useParams, useSearchParams } from '@solidjs/router'
import {
  createEffect,
  createResource,
  createSignal,
  For,
  Show,
  type JSX,
} from 'solid-js'
import {
  addFavorite,
  createFolder,
  getFavorites,
  getFolderAncestors,
  getFolderChildren,
  moveFolders,
  removeFavorite,
  renameFolder,
  trashNodes,
} from '../api/client'
import type { FolderAncestor, FolderItem } from '../api/types'
import type { FileDetails, MediaStream } from '../api/types'
import { Breadcrumb } from '../components/Breadcrumb'
import { CreateFolderDialog } from '../components/CreateFolderDialog'
import { FileTable } from '../components/FileTable'
import { FileDetailsPanel } from '../components/FileDetailsPanel'
import { FileUploadControl } from '../components/FileUploadControl'
import { FolderUploadControl } from '../components/FolderUploadControl'
import { MoveFolderDialog } from '../components/MoveFolderDialog'
import { demoImage, PreviewModal } from '../components/PreviewModal'
import { RenameFolderDialog } from '../components/RenameFolderDialog'
import '../components/UploadDropZone.css'
import { useUploads } from '../uploads/UploadContext'
import { collectDroppedFiles } from '../uploads/dropFiles'
import type { FolderUploadResult } from '../uploads/folderUpload'

const ROOT_FOLDER_ID = '00000000-0000-0000-0000-000000000001'

interface WorkspaceViewProps {
  eyebrow: string
  title: string
  description: string
  breadcrumb?: JSX.Element
  children?: JSX.Element
}

function WorkspaceView(props: WorkspaceViewProps) {
  return (
    <section class="workspace-view">
      {props.breadcrumb}
      <header>
        <p class="workspace-view__eyebrow">{props.eyebrow}</p>
        <h1>{props.title}</h1>
        <p class="workspace-view__description">{props.description}</p>
      </header>
      <div class="workspace-view__surface">{props.children}</div>
    </section>
  )
}

export function RootFolderView() {
  const rootPath: FolderAncestor[] = [{ id: ROOT_FOLDER_ID, name: 'root' }]
  return (
    <WorkspaceView
      breadcrumb={<Breadcrumb items={rootPath} />}
      eyebrow="Library"
      title="All Files"
      description="Everything stored in your Strife drive."
    >
      <FolderContents folderId={ROOT_FOLDER_ID} />
    </WorkspaceView>
  )
}

export function FolderView() {
  const params = useParams<{ id: string }>()
  const [ancestors] = createResource<FolderAncestor[], string>(
    () => params.id,
    (folderId) => getFolderAncestors(folderId),
  )
  const title = () => ancestors()?.at(-1)?.name ?? 'Folder'

  return (
    <WorkspaceView
      breadcrumb={
        <Show
          when={ancestors()}
          fallback={
            <p class="breadcrumb breadcrumb--status">
              {ancestors.loading
                ? 'Loading folder path…'
                : 'Folder path unavailable'}
            </p>
          }
        >
          {(items) => <Breadcrumb items={items()} />}
        </Show>
      }
      eyebrow="Folder"
      title={title()}
      description="Browse this folder's contents."
    >
      <FolderContents folderId={params.id} />
    </WorkspaceView>
  )
}

type SortColumn = 'name' | 'kind' | 'size' | 'updated_at' | 'created_at'
type SortDir = 'asc' | 'desc' | undefined
type KindFilter = 'folder' | 'image' | 'document' | 'video' | 'audio'

function FolderContents(props: { folderId: string }) {
  const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'
  const [searchParams, setSearchParams] = useSearchParams()
  const [showCreateDialog, setShowCreateDialog] = createSignal(false)
  const [renameItem, setRenameItem] = createSignal<FolderItem>()
  const [moveItems, setMoveItems] = createSignal<FolderItem[]>()
  const [staticItems, setStaticItems] = createSignal(previewItems)
  const [dragDepth, setDragDepth] = createSignal(0)
  const [dropResults, setDropResults] = createSignal<FolderUploadResult[]>([])
  const [detailsItem, setDetailsItem] = createSignal<FolderItem>()
  const [previewItem, setPreviewItem] = createSignal<FolderItem>()
  const uploads = useUploads()

  const sortColumn = (): SortColumn => {
    const value = searchParams.sort
    if (
      value === 'kind' ||
      value === 'size' ||
      value === 'updated_at' ||
      value === 'created_at'
    ) {
      return value
    }
    return 'name'
  }
  const sortOrder = (): SortDir => {
    const value = searchParams.order
    if (value === 'asc' || value === 'desc') return value
    return sortColumn() === 'name' ? undefined : 'asc'
  }
  const kindFilters = (): KindFilter[] => {
    const raw = searchParams.kind
    const values = Array.isArray(raw) ? raw : raw ? [raw] : []
    return values.filter((value): value is KindFilter =>
      ['folder', 'image', 'document', 'video', 'audio'].includes(value),
    )
  }

  const [children, { mutate, refetch }] = createResource(
    () =>
      staticPreview
        ? false
        : {
            folderId: props.folderId,
            sort: sortColumn(),
            order: sortOrder() ?? 'asc',
            kind: kindFilters(),
          },
    (query) =>
      getFolderChildren(query.folderId, undefined, {
        sort: query.sort,
        order: query.order,
        kind: query.kind,
      }),
  )
  createEffect(() => {
    if (!staticPreview) {
      void uploads.discover(props.folderId, () => void refetch())
    }
  })
  const items = () =>
    staticPreview ? staticItems() : (children()?.items ?? [])

  const cycleSort = (column: SortColumn) => {
    const current = sortColumn()
    const order = sortOrder()
    if (current !== column) {
      setSearchParams({ sort: column, order: 'asc' }, { resolve: false })
      return
    }
    if (order === 'asc' || order === undefined) {
      setSearchParams({ sort: column, order: 'desc' }, { resolve: false })
      return
    }
    // Third click returns to default name sort.
    setSearchParams({ sort: undefined, order: undefined }, { resolve: false })
  }

  const toggleKindFilter = (kind: KindFilter) => {
    const active = new Set(kindFilters())
    if (active.has(kind)) active.delete(kind)
    else active.add(kind)
    const next = [...active]
    setSearchParams(
      { kind: next.length > 0 ? next : undefined },
      { resolve: false },
    )
  }

  const handleCreate = async (name: string) => {
    if (staticPreview) {
      const now = new Date().toISOString()
      setStaticItems((current) => [
        ...current,
        {
          id: crypto.randomUUID(),
          name,
          kind: 'folder',
          size_bytes: null,
          created_at: now,
          updated_at: now,
        },
      ])
      return
    }

    await createFolder(props.folderId, name)
    await refetch()
  }

  const handleRename = async (folder: FolderItem, name: string) => {
    if (staticPreview) {
      setStaticItems((current) =>
        current.map((item) =>
          item.id === folder.id ? { ...item, name } : item,
        ),
      )
      return
    }

    const updated = await renameFolder(folder.id, name)
    mutate((current) =>
      current
        ? {
            ...current,
            items: current.items.map((item) =>
              item.id === updated.id ? updated : item,
            ),
          }
        : current,
    )
  }

  const handleMove = async (folders: FolderItem[], parentId: string) => {
    const movedIds = new Set(folders.map((folder) => folder.id))
    if (staticPreview) {
      setStaticItems((current) =>
        current.filter((item) => !movedIds.has(item.id)),
      )
      return
    }

    await moveFolders([...movedIds], parentId)
    mutate((current) =>
      current
        ? {
            ...current,
            items: current.items.filter((item) => !movedIds.has(item.id)),
          }
        : current,
    )
  }

  const handleToggleFavorite = async (item: FolderItem) => {
    const next = !item.is_favorite
    if (staticPreview) {
      setStaticItems((current) =>
        current.map((candidate) =>
          candidate.id === item.id
            ? { ...candidate, is_favorite: next }
            : candidate,
        ),
      )
      return
    }
    if (next) await addFavorite(item.id)
    else await removeFavorite(item.id)
    mutate((current) =>
      current
        ? {
            ...current,
            items: current.items.map((candidate) =>
              candidate.id === item.id
                ? { ...candidate, is_favorite: next }
                : candidate,
            ),
          }
        : current,
    )
  }

  const handleTrash = async (items: FolderItem[]) => {
    const ids = items.map((item) => item.id)
    if (staticPreview) {
      setStaticItems((current) =>
        current.filter((item) => !ids.includes(item.id)),
      )
      return
    }
    await trashNodes(ids)
    mutate((current) =>
      current
        ? {
            ...current,
            items: current.items.filter((item) => !ids.includes(item.id)),
          }
        : current,
    )
  }

  const loadPreviewFolders = async (folderId: string) => ({
    items:
      folderId === ROOT_FOLDER_ID
        ? staticItems().filter((item) => item.kind === 'folder')
        : [],
    next_cursor: null,
  })

  const handleDrop = async (event: DragEvent) => {
    event.preventDefault()
    setDragDepth(0)
    const candidates = await collectDroppedFiles(event.dataTransfer!)
    if (candidates.length === 0) return
    const results = await uploads.start(
      candidates,
      props.folderId,
      () => void refetch(),
    )
    setDropResults(results)
  }

  const dropFailures = () => dropResults().filter((result) => result.error)

  return (
    <div
      class="upload-drop-target"
      onDragEnter={(event) => {
        event.preventDefault()
        setDragDepth((depth) => depth + 1)
      }}
      onDragLeave={() => setDragDepth((depth) => Math.max(0, depth - 1))}
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => void handleDrop(event)}
    >
      <Show when={dragDepth() > 0}>
        <div class="upload-drop-overlay">Drop files or folders to upload</div>
      </Show>
      <div class="folder-toolbar">
        <FileUploadControl
          folderId={props.folderId}
          onComplete={() => void refetch()}
        />
        <FolderUploadControl
          folderId={props.folderId}
          onComplete={() => void refetch()}
        />
        <button type="button" onClick={() => setShowCreateDialog(true)}>
          New Folder
        </button>
      </div>
      <div class="folder-filters" role="toolbar" aria-label="Kind filters">
        {(
          [
            ['folder', 'Folders'],
            ['image', 'Images'],
            ['document', 'Documents'],
            ['video', 'Video'],
            ['audio', 'Audio'],
          ] as const
        ).map(([kind, label]) => (
          <button
            type="button"
            classList={{
              'folder-filters__chip': true,
              'is-active': kindFilters().includes(kind),
            }}
            aria-pressed={kindFilters().includes(kind)}
            onClick={() => toggleKindFilter(kind)}
          >
            {label}
          </button>
        ))}
      </div>
      <Show when={dropResults().length > 0}>
        <div class="upload-drop-report" role="status">
          {dropResults().length - dropFailures().length} of{' '}
          {dropResults().length} dropped files uploaded
          <Show when={dropFailures().length > 0}>
            <ul>
              <For each={dropFailures()}>
                {(result) => (
                  <li>
                    {result.path}: {result.error}
                  </li>
                )}
              </For>
            </ul>
          </Show>
        </div>
      </Show>
      <FileTable
        items={items()}
        loading={children.loading}
        error={
          children.error instanceof Error ? children.error.message : undefined
        }
        onRetry={() => void refetch()}
        sortColumn={sortColumn()}
        sortOrder={sortOrder() ?? (sortColumn() === 'name' ? 'asc' : 'asc')}
        onSort={cycleSort}
        onRename={setRenameItem}
        onMove={setMoveItems}
        onTrash={(selected) => void handleTrash(selected)}
        onToggleFavorite={(item) => void handleToggleFavorite(item)}
        onDetails={setDetailsItem}
        onPreview={setPreviewItem}
        onSelectionChange={(selected) => {
          if (
            detailsItem() &&
            selected.length === 1 &&
            selected[0].kind === 'file'
          ) {
            setDetailsItem(selected[0])
          }
        }}
      />
      <Show when={previewItem()}>
        {(item) => (
          <PreviewModal
            item={item()}
            files={items().filter((candidate) => candidate.kind === 'file')}
            staticDetails={
              staticPreview ? previewFileDetails.get(item().id) : undefined
            }
            staticSource={
              staticPreview ? previewSources.get(item().id) : undefined
            }
            onNavigate={setPreviewItem}
            onClose={() => setPreviewItem(undefined)}
          />
        )}
      </Show>
      <Show when={detailsItem()}>
        {(item) => (
          <FileDetailsPanel
            item={item()}
            staticDetails={
              staticPreview ? previewFileDetails.get(item().id) : undefined
            }
            staticStreams={
              staticPreview ? previewStreams.get(item().id) : undefined
            }
            onClose={() => setDetailsItem(undefined)}
          />
        )}
      </Show>
      <Show when={showCreateDialog()}>
        <CreateFolderDialog
          onCreate={handleCreate}
          onClose={() => setShowCreateDialog(false)}
        />
      </Show>
      <Show when={renameItem()}>
        {(folder) => (
          <RenameFolderDialog
            folder={folder()}
            onRename={(name) => handleRename(folder(), name)}
            onClose={() => setRenameItem(undefined)}
          />
        )}
      </Show>
      <Show when={moveItems()}>
        {(folders) => (
          <MoveFolderDialog
            items={folders()}
            currentFolderId={props.folderId}
            loadChildren={staticPreview ? loadPreviewFolders : undefined}
            onMove={(parentId) => handleMove(folders(), parentId)}
            onClose={() => setMoveItems(undefined)}
          />
        )}
      </Show>
    </div>
  )
}

const previewItems: FolderItem[] = [
  {
    id: '20000000-0000-0000-0000-000000000001',
    name: 'Family Photos',
    kind: 'folder',
    size_bytes: null,
    created_at: '2026-07-01T14:30:00Z',
    updated_at: '2026-07-27T18:42:00Z',
    is_favorite: true,
  },
  {
    id: '20000000-0000-0000-0000-000000000002',
    name: 'Projects',
    kind: 'folder',
    size_bytes: null,
    created_at: '2026-06-14T11:12:00Z',
    updated_at: '2026-07-26T09:18:00Z',
  },
  {
    id: '20000000-0000-0000-0000-000000000003',
    name: 'Home inventory.pdf',
    kind: 'file',
    size_bytes: 2_480_000,
    created_at: '2026-07-18T20:05:00Z',
    updated_at: '2026-07-18T20:05:00Z',
    is_favorite: true,
  },
  {
    id: '20000000-0000-0000-0000-000000000004',
    name: 'Lake sunrise.jpg',
    kind: 'file',
    size_bytes: 18_740_000,
    created_at: '2026-07-20T10:14:00Z',
    updated_at: '2026-07-20T10:14:00Z',
  },
  {
    id: '20000000-0000-0000-0000-000000000005',
    name: 'Backyard concert.mp4',
    kind: 'file',
    size_bytes: 384_200_000,
    created_at: '2026-07-22T02:11:00Z',
    updated_at: '2026-07-22T02:11:00Z',
  },
  {
    id: '20000000-0000-0000-0000-000000000006',
    name: 'Neighborhood newsletter.docx',
    kind: 'file',
    size_bytes: 846_000,
    created_at: '2026-07-23T16:40:00Z',
    updated_at: '2026-07-24T01:12:00Z',
  },
  {
    id: '20000000-0000-0000-0000-000000000007',
    name: 'Porch interview.mp3',
    kind: 'file',
    size_bytes: 12_840_000,
    created_at: '2026-07-24T19:08:00Z',
    updated_at: '2026-07-24T19:08:00Z',
  },
  {
    id: '20000000-0000-0000-0000-000000000008',
    name: 'Mountain sequence.gif',
    kind: 'file',
    size_bytes: 6_240_000,
    created_at: '2026-07-25T13:54:00Z',
    updated_at: '2026-07-25T13:54:00Z',
  },
  {
    id: '20000000-0000-0000-0000-000000000009',
    name: 'DSC_4821.nef',
    kind: 'file',
    size_bytes: 62_400_000,
    created_at: '2026-07-26T03:22:00Z',
    updated_at: '2026-07-26T03:22:00Z',
  },
  {
    id: '20000000-0000-0000-0000-000000000010',
    name: 'Scanned hardware receipt.png',
    kind: 'file',
    size_bytes: 4_190_000,
    created_at: '2026-07-27T21:31:00Z',
    updated_at: '2026-07-27T21:34:00Z',
  },
  {
    id: '20000000-0000-0000-0000-000000000011',
    name: 'Cold storage archive.zip',
    kind: 'file',
    size_bytes: 1_482_000_000,
    created_at: '2026-07-28T04:05:00Z',
    updated_at: '2026-07-28T04:05:00Z',
  },
]

function makePreviewDetails(
  item: FolderItem,
  metadata: Partial<FileDetails>,
): FileDetails {
  return {
    id: item.id,
    parent_id: ROOT_FOLDER_ID,
    name: item.name,
    byte_size: item.size_bytes ?? 0,
    checksum_sha256:
      '8ed3f6ad685b959ead7022518e1af76cd816f8e8ec7ccdda1ed4018e8f2223f8',
    created_at: item.created_at,
    updated_at: item.updated_at,
    detected_mime: 'application/octet-stream',
    media_kind: 'other',
    duration_ms: null,
    width: null,
    height: null,
    capture_time: null,
    page_count: null,
    orientation: null,
    has_gps: false,
    gps_latitude: null,
    gps_longitude: null,
    camera_make: null,
    camera_model: null,
    document_title: null,
    document_author: null,
    document_created_at: null,
    document_modified_at: null,
    processing_status: 'ready',
    ...metadata,
  }
}

const previewFileDetails = new Map<string, FileDetails>([
  [
    previewItems[2].id,
    makePreviewDetails(previewItems[2], {
      detected_mime: 'application/pdf',
      media_kind: 'document',
      page_count: 24,
      document_title: 'Home Inventory 2026',
      document_author: 'Sanjee',
      document_created_at: '2026-07-18T20:01:00Z',
      document_modified_at: '2026-07-18T20:05:00Z',
    }),
  ],
  [
    previewItems[3].id,
    makePreviewDetails(previewItems[3], {
      detected_mime: 'image/jpeg',
      media_kind: 'image',
      width: 8256,
      height: 5504,
      orientation: 1,
      camera_make: 'Nikon',
      camera_model: 'Z 8',
      capture_time: '2026-07-20T05:14:00Z',
      has_gps: true,
      gps_latitude: 41.8819,
      gps_longitude: -87.6278,
    }),
  ],
  [
    previewItems[4].id,
    makePreviewDetails(previewItems[4], {
      detected_mime: 'video/mp4',
      media_kind: 'video',
      duration_ms: 212_000,
      width: 3840,
      height: 2160,
      processing_status: 'partially_processed',
    }),
  ],
  [
    previewItems[5].id,
    makePreviewDetails(previewItems[5], {
      detected_mime:
        'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
      media_kind: 'document',
      page_count: 8,
      document_title: 'Lakeside Neighborhood Newsletter',
      document_author: 'Community Association',
      document_created_at: '2026-07-23T16:40:00Z',
      document_modified_at: '2026-07-24T01:12:00Z',
    }),
  ],
  [
    previewItems[6].id,
    makePreviewDetails(previewItems[6], {
      detected_mime: 'audio/mpeg',
      media_kind: 'audio',
      duration_ms: 401_000,
    }),
  ],
  [
    previewItems[7].id,
    makePreviewDetails(previewItems[7], {
      detected_mime: 'image/gif',
      media_kind: 'image',
      width: 1280,
      height: 720,
      orientation: 1,
    }),
  ],
  [
    previewItems[8].id,
    makePreviewDetails(previewItems[8], {
      detected_mime: 'image/x-nikon-nef',
      media_kind: 'image',
      width: 8256,
      height: 5504,
      orientation: 1,
      camera_make: 'Nikon',
      camera_model: 'Z 8',
      capture_time: '2026-07-25T22:22:00Z',
      has_gps: true,
      gps_latitude: 46.8523,
      gps_longitude: -121.7603,
    }),
  ],
  [
    previewItems[9].id,
    makePreviewDetails(previewItems[9], {
      detected_mime: 'image/png',
      media_kind: 'image',
      width: 1800,
      height: 2600,
      orientation: 1,
      processing_status: 'partially_processed',
    }),
  ],
  [
    previewItems[10].id,
    makePreviewDetails(previewItems[10], {
      detected_mime: 'application/zip',
      media_kind: 'other',
    }),
  ],
])

const previewStreams = new Map<string, MediaStream[]>([
  [
    previewItems[4].id,
    [
      {
        id: 'stream-video',
        stream_index: 0,
        stream_type: 'video',
        codec: 'h264',
        width: 3840,
        height: 2160,
        duration_ms: 212_000,
        bitrate_bps: 12_400_000,
        frame_rate: '30/1',
        language: null,
        created_at: previewItems[4].created_at,
      },
      {
        id: 'stream-audio',
        stream_index: 1,
        stream_type: 'audio',
        codec: 'aac',
        width: null,
        height: null,
        duration_ms: 212_000,
        bitrate_bps: 192_000,
        frame_rate: null,
        language: 'eng',
        created_at: previewItems[4].created_at,
      },
    ],
  ],
  [
    previewItems[6].id,
    [
      {
        id: 'stream-interview-audio',
        stream_index: 0,
        stream_type: 'audio',
        codec: 'mp3',
        width: null,
        height: null,
        duration_ms: 401_000,
        bitrate_bps: 256_000,
        frame_rate: null,
        language: 'eng',
        created_at: previewItems[6].created_at,
      },
    ],
  ],
])

const demoDocument = `data:text/html,${encodeURIComponent(`
<!doctype html><html><body style="font-family:system-ui;margin:0;background:#d9d9dc;padding:3rem">
<main style="box-sizing:border-box;max-width:48rem;min-height:62rem;margin:auto;padding:5rem;background:white;color:#202024;box-shadow:0 1rem 3rem #0003">
<p style="color:#667;text-transform:uppercase;letter-spacing:.12em">Home inventory · 2026</p><h1 style="font-size:2.6rem">Household Inventory</h1><hr><h2>Living room</h2><p>Furniture, electronics, art, and insured valuables.</p><table style="width:100%;border-collapse:collapse"><tr><th style="text-align:left;border-bottom:1px solid #bbb;padding:.8rem">Item</th><th style="text-align:right;border-bottom:1px solid #bbb">Value</th></tr><tr><td style="padding:.8rem">Camera kit</td><td style="text-align:right">$4,200</td></tr><tr><td style="padding:.8rem">Stereo system</td><td style="text-align:right">$1,800</td></tr></table>
</main></body></html>`)}`

const demoNewsletter = `data:text/html,${encodeURIComponent(`
<!doctype html><html><body style="font-family:Georgia,serif;margin:0;background:#d8dad3;padding:3rem">
<main style="box-sizing:border-box;max-width:48rem;min-height:62rem;margin:auto;padding:4rem;background:#fffdf6;color:#273027;box-shadow:0 1rem 3rem #0003">
<p style="color:#58705b;text-transform:uppercase;letter-spacing:.16em">July 2026 · Volume 12</p><h1 style="font-size:2.8rem;color:#244b35">Lakeside Notes</h1><hr style="border-color:#9bb29f"><h2>Summer on the block</h2><p>Meet your neighbors at the annual picnic, browse the garden tour, and help stock the community pantry.</p><blockquote style="border-left:.3rem solid #d39745;margin:2rem 0;padding:1rem 1.5rem;background:#faf1df">Saturday, August 8 · Lakeside Park Pavilion</blockquote><h2>In this issue</h2><ol><li>Picnic schedule and volunteer list</li><li>Water-wise gardening</li><li>Library book exchange</li></ol>
</main></body></html>`)}`

const demoReceipt = `data:image/svg+xml,${encodeURIComponent(`
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 900 1300">
  <rect width="900" height="1300" fill="#d9d3c8"/><path d="M150 65h600v1170l-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12-22 12-22-12 12V65Z" fill="#fffdf6"/>
  <g fill="#242424" font-family="monospace" text-anchor="middle"><text x="450" y="150" font-size="42" font-weight="bold">NORTHSIDE HARDWARE</text><text x="450" y="202" font-size="25">THANK YOU FOR SHOPPING LOCAL</text></g>
  <g fill="#333" font-family="monospace" font-size="28"><text x="205" y="310">DECK SCREWS 3 IN.</text><text x="690" y="310" text-anchor="end">24.98</text><text x="205" y="365">CEDAR BOARD 8 FT.</text><text x="690" y="365" text-anchor="end">52.40</text><text x="205" y="420">EXTERIOR STAIN</text><text x="690" y="420" text-anchor="end">38.99</text><path d="M205 470h485" stroke="#333" stroke-dasharray="12 10"/><text x="205" y="535">SUBTOTAL</text><text x="690" y="535" text-anchor="end">116.37</text><text x="205" y="590">TAX</text><text x="690" y="590" text-anchor="end">11.06</text><text x="205" y="660" font-weight="bold">TOTAL</text><text x="690" y="660" text-anchor="end" font-weight="bold">$127.43</text><text x="450" y="790" text-anchor="middle">VISA •••• 4821</text><text x="450" y="850" text-anchor="middle">07/27/2026  4:31 PM</text></g>
</svg>`)}`

const previewSources = new Map<string, string>([
  [previewItems[2].id, demoDocument],
  [previewItems[3].id, demoImage],
  [previewItems[4].id, ''],
  [previewItems[5].id, demoNewsletter],
  [previewItems[6].id, ''],
  [previewItems[7].id, demoImage],
  [previewItems[8].id, demoImage],
  [previewItems[9].id, demoReceipt],
  [previewItems[10].id, ''],
])

export function FavoritesView() {
  const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'
  const [favorites, { refetch }] = createResource(
    () => (staticPreview ? false : 'favorites'),
    () => getFavorites(),
  )
  const items = (): FolderItem[] => {
    if (staticPreview) {
      return previewItems
        .filter((item) => item.is_favorite)
        .map((item) => ({ ...item, is_favorite: true }))
    }
    return (
      favorites()?.items.map((item) => ({
        id: item.id,
        name: item.name,
        kind: item.kind,
        size_bytes: null,
        created_at: item.created_at,
        updated_at: item.updated_at,
        is_favorite: true,
      })) ?? []
    )
  }

  const handleToggleFavorite = async (item: FolderItem) => {
    if (staticPreview) return
    await removeFavorite(item.id)
    await refetch()
  }

  return (
    <WorkspaceView
      eyebrow="Library"
      title="Favorites"
      description="Frequently used files and folders."
    >
      <FileTable
        items={items()}
        loading={favorites.loading}
        error={
          favorites.error instanceof Error
            ? favorites.error.message
            : undefined
        }
        onRetry={() => void refetch()}
        onToggleFavorite={(item) => void handleToggleFavorite(item)}
      />
    </WorkspaceView>
  )
}

export function TrashView() {
  return (
    <WorkspaceView
      eyebrow="Library"
      title="Trash"
      description="Deleted items are retained for 30 days."
    />
  )
}
