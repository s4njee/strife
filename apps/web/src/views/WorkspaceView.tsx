import { useParams } from '@solidjs/router'
import { createResource, Show, type JSX } from 'solid-js'
import { getFolderAncestors } from '../api/client'
import type { FolderAncestor } from '../api/types'
import { Breadcrumb } from '../components/Breadcrumb'

const ROOT_FOLDER_ID = '00000000-0000-0000-0000-000000000001'

interface WorkspaceViewProps {
  eyebrow: string
  title: string
  description: string
  breadcrumb?: JSX.Element
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
      <div class="workspace-view__surface" />
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
    />
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
    />
  )
}

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
