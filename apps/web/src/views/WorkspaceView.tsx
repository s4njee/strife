import { useParams } from '@solidjs/router'
import { createResource, createSignal, Show, type JSX } from 'solid-js'
import {
  createFolder,
  getFolderAncestors,
  getFolderChildren,
} from '../api/client'
import type { FolderAncestor, FolderItem } from '../api/types'
import { Breadcrumb } from '../components/Breadcrumb'
import { CreateFolderDialog } from '../components/CreateFolderDialog'
import { FileTable } from '../components/FileTable'

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

function FolderContents(props: { folderId: string }) {
  const staticPreview = import.meta.env.VITE_STATIC_PREVIEW === 'true'
  const [showCreateDialog, setShowCreateDialog] = createSignal(false)
  const [staticItems, setStaticItems] = createSignal(previewItems)
  const [children, { refetch }] = createResource(
    () => (staticPreview ? false : props.folderId),
    (folderId) => getFolderChildren(folderId),
  )
  const items = () =>
    staticPreview ? staticItems() : (children()?.items ?? [])

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

  return (
    <>
      <div class="folder-toolbar">
        <button type="button" onClick={() => setShowCreateDialog(true)}>
          New Folder
        </button>
      </div>
      <FileTable
        items={items()}
        loading={children.loading}
        error={
          children.error instanceof Error ? children.error.message : undefined
        }
        onRetry={() => void refetch()}
      />
      <Show when={showCreateDialog()}>
        <CreateFolderDialog
          onCreate={handleCreate}
          onClose={() => setShowCreateDialog(false)}
        />
      </Show>
    </>
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
  },
]

export function FavoritesView() {
  return (
    <WorkspaceView
      eyebrow="Library"
      title="Favorites"
      description="Frequently used files and folders will appear here."
    />
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
